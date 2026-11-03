//! One parallel pass over the home folder that finds both project build
//! artifacts (node_modules, target/, .venv …) and unusually large files.

use crate::finders::Found;
use crate::fsx::{self, allocated};
use crate::model::{Category, Item};
use crate::scan::Ctx;
use crate::{shell, ui};
use rayon::prelude::*;
use std::collections::{BTreeMap, HashSet};
use std::fs::{self, Metadata};
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};

const MAX_DEPTH: usize = 16;
const MAX_LARGE: usize = 300;

/// Never descend into these, anywhere.
pub const SKIP_ANYWHERE: &[&str] = &[
    ".git",
    ".hg",
    ".svn",
    "node_modules",
    ".Trash",
    ".Trashes",
    ".Spotlight-V100",
    ".fseventsd",
    ".DocumentRevisions-V100",
    ".TemporaryItems",
];

/// Directly under $HOME: covered by dedicated categories, or not ours to judge.
const SKIP_AT_HOME: &[&str] = &[
    "Library",
    ".npm",
    ".cargo",
    ".rustup",
    ".gradle",
    ".m2",
    ".cache",
    ".bun",
    ".pnpm-store",
    ".ollama",
    ".lmstudio",
    ".nvm",
    ".pyenv",
    "go",
    ".yarn",
];

/// macOS packages that look like folders. We never walk inside them.
const PACKAGES_SKIP: &[&str] = &[
    "app",
    "photoslibrary",
    "photolibrary",
    "migratedphotolibrary",
    "aplibrary",
    "musiclibrary",
    "tvlibrary",
    "framework",
    "bundle",
    "plugin",
    "kext",
    "appex",
    "xcodeproj",
    "xcworkspace",
    "lproj",
    "vst3",
    "component",
];

/// Packages that are documents/libraries — measured as one "file".
const PACKAGES_MEASURE: &[&str] = &[
    "sparsebundle",
    "fcpbundle",
    "logicx",
    "band",
    "imovielibrary",
    "key",
    "pages",
    "numbers",
    "rtfd",
    "xcarchive",
    "dmgpart",
    "vmwarevm",
    "pvm",
    "utm",
];

/// True for macOS package folders we never walk into (apps, photo libraries…).
pub fn is_skipped_package(name: &str) -> bool {
    ext_of(name).is_some_and(|e| PACKAGES_SKIP.contains(&e.as_str()))
}

const JS_CACHES: &[&str] = &[
    ".next",
    ".nuxt",
    ".svelte-kit",
    ".turbo",
    ".parcel-cache",
    ".angular",
    ".expo",
    ".docusaurus",
];

struct Artifact {
    project: PathBuf,
    path: PathBuf,
    kind: String,
}

struct Sweeper<'a> {
    ctx: &'a Ctx,
    artifacts: Mutex<Vec<Artifact>>,
    large: Mutex<Vec<(PathBuf, u64, i64)>>,
    denied: AtomicU64,
}

fn ext_of(name: &str) -> Option<String> {
    let (stem, ext) = name.rsplit_once('.')?;
    (!stem.is_empty()).then(|| ext.to_ascii_lowercase())
}

/// Is `name` (a directory next to `siblings`) a regenerable build artifact?
pub fn artifact_kind(name: &str, siblings: &HashSet<&str>, path: &Path) -> Option<String> {
    let has = |f: &str| siblings.contains(f);
    let has_ext = |e: &str| siblings.iter().any(|s| s.ends_with(e));
    let gradle = || {
        has("build.gradle")
            || has("build.gradle.kts")
            || has("settings.gradle")
            || has("settings.gradle.kts")
    };
    let ok = match name {
        "node_modules" => has("package.json"),
        "target" => has("Cargo.toml") || has("pom.xml") || fsx::exists(&path.join("CACHEDIR.TAG")),
        n if JS_CACHES.contains(&n) => has("package.json"),
        ".venv" | "venv" | "env" | ".env" => fsx::exists(&path.join("pyvenv.cfg")),
        "Pods" => has("Podfile"),
        ".build" => has("Package.swift"),
        ".dart_tool" => has("pubspec.yaml"),
        "build" => has("pubspec.yaml") || gradle() || fsx::exists(&path.join("CMakeCache.txt")),
        ".gradle" => gradle(),
        "_build" | "deps" => has("mix.exs"),
        ".zig-cache" | "zig-cache" => has("build.zig"),
        ".stack-work" => has("stack.yaml"),
        "dist-newstyle" => has("cabal.project") || has_ext(".cabal"),
        ".terraform" => has_ext(".tf"),
        "DerivedData" => has_ext(".xcodeproj") || has_ext(".xcworkspace"),
        _ => false,
    };
    ok.then(|| name.to_string())
}

impl Sweeper<'_> {
    fn dir(&self, dir: &Path, depth: usize, project: Option<&Path>, dev: u64, large: bool) {
        let rd = match fs::read_dir(dir) {
            Ok(rd) => rd,
            Err(e) => {
                if fsx::is_denied(&e) {
                    self.denied.fetch_add(1, Ordering::Relaxed);
                }
                return;
            }
        };
        let entries: Vec<(String, PathBuf, Metadata)> = rd
            .flatten()
            .filter_map(|e| {
                let md = e.metadata().ok()?;
                Some((e.file_name().to_string_lossy().into_owned(), e.path(), md))
            })
            .collect();
        let names: HashSet<&str> = entries.iter().map(|e| e.0.as_str()).collect();
        let project = if names.contains(".git") {
            Some(dir)
        } else {
            project
        };
        let at_home = dir == self.ctx.home;

        let mut subdirs = Vec::new();
        for (name, path, md) in &entries {
            if md.is_dir() {
                if let Some(kind) = artifact_kind(name, &names, path) {
                    self.artifacts.lock().unwrap().push(Artifact {
                        project: project.unwrap_or(dir).to_path_buf(),
                        path: path.clone(),
                        kind,
                    });
                    continue;
                }
                if SKIP_ANYWHERE.contains(&name.as_str())
                    || (at_home && SKIP_AT_HOME.contains(&name.as_str()))
                {
                    continue;
                }
                if let Some(ext) = ext_of(name) {
                    if PACKAGES_SKIP.contains(&ext.as_str()) {
                        continue;
                    }
                    if PACKAGES_MEASURE.contains(&ext.as_str()) {
                        if large {
                            let st = fsx::measure(path);
                            self.push_large(path, st.bytes, st.newest);
                        }
                        continue;
                    }
                }
                if md.dev() == dev && depth < MAX_DEPTH {
                    subdirs.push(path.clone());
                }
            } else if large && md.is_file() {
                self.push_large(path, allocated(md), md.mtime());
            }
        }
        subdirs
            .par_iter()
            .for_each(|p| self.dir(p, depth + 1, project, dev, large));
    }

    fn push_large(&self, path: &Path, bytes: u64, mtime: i64) {
        if bytes >= self.ctx.large_min {
            self.large
                .lock()
                .unwrap()
                .push((path.to_path_buf(), bytes, mtime));
        }
    }
}

/// When did anyone last work on this project? Looks at git activity and the
/// top two levels of the project, ignoring the artifacts themselves.
fn project_activity(project: &Path, artifacts: &HashSet<&Path>) -> i64 {
    let mut newest = 0i64;
    let mut bump = |p: &Path| {
        if let Some(md) = fsx::lstat(p) {
            newest = newest.max(md.mtime());
        }
    };
    bump(&project.join(".git/index"));
    bump(&project.join(".git/logs/HEAD"));
    for child in fsx::children(project).unwrap_or_default() {
        let n = fsx::file_name(&child);
        if n == ".git" || n == ".DS_Store" || artifacts.contains(child.as_path()) {
            continue;
        }
        bump(&child);
        if fsx::is_real_dir(&child) {
            for grand in fsx::children(&child).unwrap_or_default() {
                if !artifacts.contains(grand.as_path()) && fsx::file_name(&grand) != ".DS_Store" {
                    bump(&grand);
                }
            }
        }
    }
    newest
}

pub fn sweep(ctx: &Ctx) -> Found {
    let s = Sweeper {
        ctx,
        artifacts: Mutex::new(Vec::new()),
        large: Mutex::new(Vec::new()),
        denied: AtomicU64::new(0),
    };
    let mut roots: Vec<(PathBuf, bool)> = vec![(ctx.home.clone(), true)];
    for r in &ctx.extra_roots {
        if !r.starts_with(&ctx.home) {
            roots.push((r.clone(), false));
        }
    }
    roots.par_iter().for_each(|(root, large)| {
        if let Some(md) = fsx::lstat(root).filter(|m| m.is_dir()) {
            s.dir(root, 0, None, md.dev(), *large);
        }
    });
    let denied = s.denied.load(Ordering::Relaxed);

    // ---- project artifacts, one item per project
    let mut by_project: BTreeMap<PathBuf, Vec<Artifact>> = BTreeMap::new();
    for a in s.artifacts.into_inner().unwrap() {
        by_project.entry(a.project.clone()).or_default().push(a);
    }
    let projects: Vec<(PathBuf, Vec<Artifact>)> = by_project.into_iter().collect();
    let items: Vec<Item> = projects
        .into_par_iter()
        .map(|(project, arts)| {
            let paths: Vec<PathBuf> = arts.iter().map(|a| a.path.clone()).collect();
            let st = fsx::measure_all(&paths);
            let set: HashSet<&Path> = paths.iter().map(PathBuf::as_path).collect();
            let last = project_activity(&project, &set);
            let age = fsx::days_since(last, ctx.now);
            let mut kinds: Vec<&str> = arts.iter().map(|a| a.kind.as_str()).collect();
            kinds.sort();
            kinds.dedup();
            let detail = if paths.len() > kinds.len() {
                format!("{} ({} folders)", kinds.join(", "), paths.len())
            } else {
                kinds.join(", ")
            };
            Item {
                label: ui::tilde(&project, &ctx.home),
                detail: Some(detail),
                bytes: st.bytes,
                age_days: Some(age),
                linked_bytes: st.linked,
                preselect: age >= ctx.stale_days,
                paths,
            }
        })
        .collect();
    let artifacts = Category::from_items("project-artifacts", items, 0);

    // ---- large files
    let mut large = s.large.into_inner().unwrap();
    large.sort_by_key(|l| std::cmp::Reverse(l.1));
    large.truncate(MAX_LARGE);
    let paths: Vec<PathBuf> = large.iter().map(|l| l.0.clone()).collect();
    let used = shell::mdls_dates(&paths, "kMDItemLastUsedDate");
    let items: Vec<Item> = large
        .into_iter()
        .enumerate()
        .map(|(i, (path, bytes, mtime))| {
            let touched = mtime.max(used[i].unwrap_or(0));
            Item {
                label: ui::tilde(&path, &ctx.home),
                detail: None,
                paths: vec![path],
                bytes,
                age_days: Some(fsx::days_since(touched, ctx.now)),
                linked_bytes: 0,
                preselect: false,
            }
        })
        .collect();
    let large_cat = Category::from_items("large-files", items, denied)
        .with_note(format!("{} or bigger", ui::size(ctx.large_min)));

    Found {
        cats: vec![artifacts, large_cat],
        tips: vec![],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_artifacts_only_next_to_their_manifest() {
        let p = Path::new("/nonexistent/x");
        let sib: HashSet<&str> = ["package.json", "Cargo.toml", "src"].into_iter().collect();
        assert_eq!(
            artifact_kind("node_modules", &sib, p).as_deref(),
            Some("node_modules")
        );
        assert_eq!(artifact_kind("target", &sib, p).as_deref(), Some("target"));
        assert_eq!(artifact_kind(".next", &sib, p).as_deref(), Some(".next"));
        assert_eq!(artifact_kind("build", &sib, p), None); // a plain build/ folder could be anything
        assert_eq!(artifact_kind("src", &sib, p), None);

        let bare: HashSet<&str> = ["README.md"].into_iter().collect();
        assert_eq!(artifact_kind("node_modules", &bare, p), None);
        assert_eq!(artifact_kind("target", &bare, p), None);
        assert_eq!(artifact_kind("Pods", &bare, p), None);
    }
}
