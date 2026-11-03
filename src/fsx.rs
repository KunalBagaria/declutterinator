//! Filesystem plumbing: parallel sizing, guarded deletion, Trash, disk stats.
//!
//! Sizes are *allocated* bytes (`st_blocks * 512`), i.e. what the file really
//! costs on disk — sparse files and iCloud placeholders count as ~0.
//! Nothing here ever follows a symlink.

use rayon::prelude::*;
use std::fs::{self, Metadata};
use std::io;
use std::os::unix::fs::{MetadataExt, PermissionsExt};
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

/// macOS `SF_DATALESS`: file content lives in iCloud, not on this disk.
pub const SF_DATALESS: u32 = 0x4000_0000;

pub fn now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

pub fn days_since(ts: i64, now: i64) -> u64 {
    if ts <= 0 {
        return 0;
    }
    ((now - ts).max(0) / 86_400) as u64
}

pub fn allocated(md: &Metadata) -> u64 {
    md.blocks() * 512
}

pub fn lstat(p: &Path) -> Option<Metadata> {
    fs::symlink_metadata(p).ok()
}

pub fn is_real_dir(p: &Path) -> bool {
    lstat(p).is_some_and(|m| m.is_dir())
}

pub fn exists(p: &Path) -> bool {
    lstat(p).is_some()
}

pub fn is_denied(e: &io::Error) -> bool {
    matches!(e.raw_os_error(), Some(libc::EPERM) | Some(libc::EACCES))
}

/// Children of a directory (never follows the final symlink).
pub fn children(dir: &Path) -> io::Result<Vec<PathBuf>> {
    if !is_real_dir(dir) {
        return Ok(Vec::new());
    }
    Ok(fs::read_dir(dir)?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .collect())
}

pub fn file_name(p: &Path) -> String {
    p.file_name()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| p.display().to_string())
}

// ---------------------------------------------------------------------------
// measuring
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, Default)]
pub struct Stats {
    /// allocated bytes
    pub bytes: u64,
    pub files: u64,
    /// newest modification time found anywhere inside (unix secs)
    pub newest: i64,
    /// directories we weren't allowed to read
    pub denied: u64,
    /// bytes in files that have other hard links (may not be freed on delete)
    pub linked: u64,
}

impl Stats {
    fn merge(mut self, o: Stats) -> Stats {
        self.bytes += o.bytes;
        self.files += o.files;
        self.newest = self.newest.max(o.newest);
        self.denied += o.denied;
        self.linked += o.linked;
        self
    }

    /// Move the clone-shared part of a just-added file from `bytes` to `linked`.
    fn privatize(&mut self, path: &Path, md: &Metadata) {
        if md.nlink() > 1 {
            return; // already fully counted as linked
        }
        let alloc = allocated(md);
        if let Some(own) = private_size(path) {
            let shared = alloc.saturating_sub(own);
            self.bytes -= shared;
            self.linked += shared;
        }
    }

    fn add(&mut self, md: &Metadata) {
        let b = allocated(md);
        self.bytes += b;
        self.files += 1;
        self.newest = self.newest.max(md.mtime());
        if !md.is_dir() && md.nlink() > 1 {
            self.linked += b;
        }
    }
}

/// Total allocated size of a path (file or whole tree), in parallel.
pub fn measure(path: &Path) -> Stats {
    measure_with(path, false)
}

/// Like `measure`, but only counts bytes each file owns *privately*: blocks
/// shared through APFS clones (bun/pnpm/uv install packages this way) land in
/// `linked` instead, because deleting this copy alone won't free them.
pub fn measure_private(path: &Path) -> Stats {
    measure_with(path, true)
}

fn measure_with(path: &Path, private: bool) -> Stats {
    match fs::symlink_metadata(path) {
        Ok(md) if md.is_dir() => walk(path, &md, md.dev(), private),
        Ok(md) => {
            let mut s = Stats::default();
            s.add(&md);
            if private {
                s.privatize(path, &md);
            }
            s
        }
        Err(e) => Stats {
            denied: is_denied(&e) as u64,
            ..Default::default()
        },
    }
}

pub fn measure_all(paths: &[PathBuf]) -> Stats {
    paths
        .par_iter()
        .map(|p| measure(p))
        .reduce(Stats::default, Stats::merge)
}

fn walk(dir: &Path, md: &Metadata, dev: u64, private: bool) -> Stats {
    let mut st = Stats::default();
    st.add(md);
    let rd = match fs::read_dir(dir) {
        Ok(rd) => rd,
        Err(e) => {
            st.denied += is_denied(&e) as u64;
            return st;
        }
    };
    let mut subdirs = Vec::new();
    for entry in rd.flatten() {
        // DirEntry::metadata does not traverse symlinks
        let Ok(md) = entry.metadata() else { continue };
        if md.is_dir() {
            // stay on one filesystem: never wander into mounted volumes
            if md.dev() == dev {
                subdirs.push((entry.path(), md));
            }
        } else {
            st.add(&md);
            if private && md.is_file() {
                st.privatize(&entry.path(), &md);
            }
        }
    }
    let sub = subdirs
        .par_iter()
        .map(|(p, md)| walk(p, md, dev, private))
        .reduce(Stats::default, Stats::merge);
    st.merge(sub)
}

/// Bytes that belong *only* to this file — i.e. what deleting it frees on APFS.
/// Clones (`cp -c`, Finder "Duplicate") share blocks and report less here.
pub fn private_size(path: &Path) -> Option<u64> {
    use std::ffi::CString;
    use std::os::unix::ffi::OsStrExt;

    #[repr(C, packed(4))]
    struct Buf {
        length: u32,
        returned: [u32; 5],
        private_size: i64,
    }

    let c = CString::new(path.as_os_str().as_bytes()).ok()?;
    let mut list = libc::attrlist {
        bitmapcount: libc::ATTR_BIT_MAP_COUNT,
        reserved: 0,
        commonattr: libc::ATTR_CMN_RETURNED_ATTRS,
        volattr: 0,
        dirattr: 0,
        fileattr: 0,
        forkattr: libc::ATTR_CMNEXT_PRIVATESIZE,
    };
    let mut buf = Buf {
        length: 0,
        returned: [0; 5],
        private_size: 0,
    };
    let rc = unsafe {
        libc::getattrlist(
            c.as_ptr(),
            &mut list as *mut _ as *mut libc::c_void,
            &mut buf as *mut _ as *mut libc::c_void,
            std::mem::size_of::<Buf>(),
            libc::FSOPT_ATTR_CMN_EXTENDED | libc::FSOPT_NOFOLLOW,
        )
    };
    // returned[4] is the forkattr group: did the kernel actually fill it in?
    let returned = buf.returned;
    if rc != 0 || returned[4] & libc::ATTR_CMNEXT_PRIVATESIZE == 0 {
        return None;
    }
    let v = buf.private_size;
    Some(v.max(0) as u64)
}

// ---------------------------------------------------------------------------
// disk
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, serde::Serialize)]
pub struct Disk {
    pub total: u64,
    pub used: u64,
    pub free: u64,
}

pub fn disk(path: &Path) -> Option<Disk> {
    use std::ffi::CString;
    use std::os::unix::ffi::OsStrExt;
    let c = CString::new(path.as_os_str().as_bytes()).ok()?;
    let mut s: libc::statfs = unsafe { std::mem::zeroed() };
    if unsafe { libc::statfs(c.as_ptr(), &mut s) } != 0 {
        return None;
    }
    let bs = s.f_bsize as u64;
    let total = s.f_blocks * bs;
    let free = s.f_bavail * bs;
    let used = total.saturating_sub(s.f_bfree * bs);
    Some(Disk { total, used, free })
}

// ---------------------------------------------------------------------------
// the guard: last line of defence before anything is removed
// ---------------------------------------------------------------------------

/// Paths (relative to $HOME) that must never be removed themselves.
const PROTECTED: &[&str] = &[
    "Library/Caches",
    "Library/Logs",
    "Library/Application Support",
    "Library/Developer",
    "Library/Developer/Xcode",
    "Library/Developer/Xcode/DerivedData",
    "Library/Developer/Xcode/Archives",
    "Library/Developer/CoreSimulator",
    "Library/Developer/CoreSimulator/Devices",
    "Library/Application Support/MobileSync",
    "Library/Application Support/MobileSync/Backup",
    "Library/Preferences",
];

/// Subtrees (relative to $HOME) we refuse to touch at all.
const OFF_LIMITS: &[&str] = &[
    ".ssh",
    ".gnupg",
    "Library/Keychains",
    "Library/Mobile Documents",
    "Library/CloudStorage",
    "Library/Mail",
    "Library/Messages",
    "Library/Containers",
    "Library/Group Containers",
    "Library/Accounts",
    "Library/Photos",
];

#[derive(Clone, Debug)]
pub struct Guard {
    pub home: PathBuf,
    /// extra roots the user explicitly asked us to scan (e.g. `--projects /Volumes/dev`)
    pub roots: Vec<PathBuf>,
}

impl Guard {
    pub fn check(&self, p: &Path) -> Result<(), String> {
        if !p.is_absolute() {
            return Err("not an absolute path".into());
        }
        if p.components()
            .any(|c| matches!(c, Component::ParentDir | Component::CurDir))
        {
            return Err("path contains '.' or '..'".into());
        }
        // apps in /Applications may go to the Trash
        if p.parent() == Some(Path::new("/Applications"))
            && p.extension().is_some_and(|e| e == "app")
        {
            return Ok(());
        }
        if p.starts_with(&self.home) {
            let rel = p.strip_prefix(&self.home).unwrap();
            let depth = rel.components().count();
            if depth == 0 {
                return Err("refusing to touch the home folder itself".into());
            }
            if depth == 1 && is_real_dir(p) {
                return Err("refusing to remove a top-level home folder".into());
            }
            if PROTECTED.iter().any(|x| rel == Path::new(x)) {
                return Err("protected system folder".into());
            }
            if OFF_LIMITS.iter().any(|x| rel.starts_with(x)) {
                return Err("inside an off-limits folder".into());
            }
            return Ok(());
        }
        for root in &self.roots {
            if p.starts_with(root) && p != root {
                return Ok(());
            }
        }
        Err("outside your home folder".into())
    }
}

// ---------------------------------------------------------------------------
// removal
// ---------------------------------------------------------------------------

#[derive(Debug, Default)]
pub struct Removal {
    pub freed: u64,
    pub failed: u64,
    pub first_error: Option<String>,
}

impl Removal {
    fn fail(&mut self, p: &Path, e: &io::Error) {
        self.failed += 1;
        if self.first_error.is_none() {
            self.first_error = Some(format!("{}: {}", p.display(), describe(e)));
        }
    }
    fn merge(mut self, o: Removal) -> Removal {
        self.freed += o.freed;
        self.failed += o.failed;
        if self.first_error.is_none() {
            self.first_error = o.first_error;
        }
        self
    }
}

pub fn describe(e: &io::Error) -> String {
    match e.raw_os_error() {
        Some(libc::EPERM) => {
            "operation not permitted (protected by macOS — try Full Disk Access)".into()
        }
        Some(libc::EACCES) => "permission denied".into(),
        Some(libc::EBUSY) => "in use".into(),
        _ => e.to_string(),
    }
}

/// Permanently delete a file or directory tree. Continues past individual
/// failures, never follows symlinks and never crosses into another volume.
/// `progress` is bumped as bytes are freed.
pub fn remove_tree(path: &Path, progress: &AtomicU64) -> Removal {
    let mut r = Removal::default();
    let md = match fs::symlink_metadata(path) {
        Ok(m) => m,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return r,
        Err(e) => {
            r.fail(path, &e);
            return r;
        }
    };
    if md.is_dir() {
        rm_dir(path, &md, md.dev(), progress)
    } else {
        let b = allocated(&md);
        match remove_file_retry(path) {
            Ok(()) => {
                r.freed += b;
                progress.fetch_add(b, Ordering::Relaxed);
            }
            Err(e) => r.fail(path, &e),
        }
        r
    }
}

fn remove_file_retry(p: &Path) -> io::Result<()> {
    match fs::remove_file(p) {
        Err(e) if is_denied(&e) => {
            // read-only parent (e.g. Go's module cache) — make it writable and retry
            if let Some(parent) = p.parent() {
                make_owner_rwx(parent);
            }
            fs::remove_file(p)
        }
        other => other,
    }
}

fn make_owner_rwx(dir: &Path) {
    if let Ok(md) = fs::symlink_metadata(dir)
        && md.is_dir()
    {
        let mode = md.permissions().mode();
        if mode & 0o700 != 0o700 {
            let _ = fs::set_permissions(dir, fs::Permissions::from_mode(mode | 0o700));
        }
    }
}

fn rm_dir(dir: &Path, md: &Metadata, dev: u64, progress: &AtomicU64) -> Removal {
    let mut r = Removal::default();
    if md.permissions().mode() & 0o700 != 0o700 {
        make_owner_rwx(dir);
    }
    let rd = match fs::read_dir(dir) {
        Ok(rd) => rd,
        Err(e) => {
            r.fail(dir, &e);
            return r;
        }
    };
    let mut subdirs = Vec::new();
    for entry in rd.flatten() {
        let p = entry.path();
        let Ok(emd) = entry.metadata() else { continue };
        if emd.is_dir() {
            if emd.dev() == dev {
                subdirs.push((p, emd));
            } else {
                r.fail(&p, &io::Error::other("is a mount point — skipped"));
            }
        } else {
            let b = allocated(&emd);
            match remove_file_retry(&p) {
                Ok(()) => {
                    r.freed += b;
                    progress.fetch_add(b, Ordering::Relaxed);
                }
                Err(e) => r.fail(&p, &e),
            }
        }
    }
    let sub = subdirs
        .par_iter()
        .map(|(p, m)| rm_dir(p, m, dev, progress))
        .reduce(Removal::default, Removal::merge);
    let mut r = r.merge(sub);
    if r.failed == 0 {
        match fs::remove_dir(dir) {
            Ok(()) => {
                let b = allocated(md);
                r.freed += b;
                progress.fetch_add(b, Ordering::Relaxed);
            }
            Err(e) if e.kind() == io::ErrorKind::NotFound => {}
            Err(e) => r.fail(dir, &e),
        }
    }
    r
}

/// Move to the user's Trash via NSFileManager (no Finder automation prompt).
pub fn trash(path: &Path) -> Result<(), String> {
    use trash::TrashContext;
    use trash::macos::{DeleteMethod, TrashContextExtMacos};
    let mut ctx = TrashContext::default();
    ctx.set_delete_method(DeleteMethod::NsFileManager);
    ctx.delete(path).map_err(|e| {
        let s = e.to_string();
        if s.contains("permission") || s.contains("513") {
            "macOS needs admin rights for this one — drag it to the Trash in Finder".to_string()
        } else {
            s
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn guard() -> Guard {
        Guard {
            home: PathBuf::from("/Users/villain"),
            roots: vec![PathBuf::from("/Volumes/Lair/projects")],
        }
    }

    #[test]
    fn guard_blocks_dangerous_paths() {
        let g = guard();
        for bad in [
            "/",
            "/Users",
            "/Users/villain",
            "/Users/villain/Library/Caches",
            "/Users/villain/Library/Developer/Xcode/DerivedData",
            "/Users/villain/.ssh/id_ed25519",
            "/Users/villain/Library/Keychains/login.keychain-db",
            "/Users/villain/Library/Mobile Documents/com~apple~CloudDocs/x",
            "/Users/villain/Library/Containers/com.apple.mail/Data",
            "/Users/villain/../nemesis/Library/Caches/x",
            "/System/Library/Caches/x",
            "/Applications/Utilities/Terminal.app",
            "/Volumes/Lair/projects",
            "relative/path",
        ] {
            assert!(g.check(Path::new(bad)).is_err(), "should block {bad}");
        }
    }

    #[test]
    fn guard_allows_cleanup_targets() {
        let g = guard();
        for ok in [
            "/Users/villain/Library/Caches/com.spotify.client",
            "/Users/villain/Library/Developer/Xcode/DerivedData/Inator-abc",
            "/Users/villain/.Trash/old-scheme.txt",
            "/Users/villain/.npm/_cacache",
            "/Users/villain/Documents/lair/node_modules",
            "/Volumes/Lair/projects/inator/target",
            "/Applications/Unused.app",
        ] {
            assert!(g.check(Path::new(ok)).is_ok(), "should allow {ok}");
        }
    }

    #[test]
    fn remove_tree_handles_readonly_dirs_and_symlinks() {
        let base = std::env::temp_dir().join(format!("dcl-test-{}", std::process::id()));
        let victim = base.join("victim");
        let keep = base.join("keep");
        fs::create_dir_all(victim.join("ro/deeper")).unwrap();
        fs::create_dir_all(&keep).unwrap();
        fs::write(victim.join("ro/deeper/file.bin"), vec![7u8; 10_000]).unwrap();
        fs::write(keep.join("precious.txt"), "do not delete").unwrap();
        // symlink pointing outside must be removed as a link, not followed
        std::os::unix::fs::symlink(&keep, victim.join("link-to-keep")).unwrap();
        // read-only directories, like Go's module cache
        fs::set_permissions(victim.join("ro/deeper"), fs::Permissions::from_mode(0o555)).unwrap();
        fs::set_permissions(victim.join("ro"), fs::Permissions::from_mode(0o555)).unwrap();

        let progress = AtomicU64::new(0);
        let r = remove_tree(&victim, &progress);
        assert_eq!(r.failed, 0, "{:?}", r.first_error);
        assert!(r.freed >= 10_000);
        assert!(!victim.exists());
        assert!(keep.join("precious.txt").exists());
        fs::remove_dir_all(&base).unwrap();
    }

    #[test]
    fn private_size_sees_apfs_clones() {
        let base = std::env::temp_dir().join(format!("dcl-clone-{}", std::process::id()));
        fs::create_dir_all(&base).unwrap();
        let orig = base.join("orig.bin");
        let copy = base.join("copy.bin");
        let clone = base.join("clone.bin");
        fs::write(&orig, vec![42u8; 4_000_000]).unwrap();
        // write the bytes ourselves: std::fs::copy would make an APFS clone
        fs::write(&copy, fs::read(&orig).unwrap()).unwrap();
        let ok = std::process::Command::new("cp")
            .arg("-c")
            .arg(&orig)
            .arg(&clone)
            .status()
            .unwrap();
        assert!(ok.success());
        let p_copy = private_size(&copy).expect("getattrlist works");
        let p_clone = private_size(&clone).expect("getattrlist works");
        assert!(p_copy >= 4_000_000, "real copy owns its bytes: {p_copy}");
        assert!(p_clone < 100_000, "clone shares its bytes: {p_clone}");
        fs::remove_dir_all(&base).unwrap();
    }

    #[test]
    fn measure_counts_allocated_bytes() {
        let base = std::env::temp_dir().join(format!("dcl-measure-{}", std::process::id()));
        fs::create_dir_all(base.join("a/b")).unwrap();
        fs::write(base.join("a/b/x"), vec![1u8; 100_000]).unwrap();
        fs::write(base.join("y"), vec![1u8; 50_000]).unwrap();
        let s = measure(&base);
        assert!(s.bytes >= 150_000, "{s:?}");
        assert_eq!(s.files, 5); // 3 dirs + 2 files
        fs::remove_dir_all(&base).unwrap();
    }
}
