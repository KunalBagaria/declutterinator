//! `dupes`: find identical files. APFS-aware: copies that are clones share
//! storage, so we ask the kernel how many bytes each copy *privately* owns.

use crate::clean::{interactive, theme};
use crate::fsx::{self, Guard, SF_DATALESS};
use crate::sweep::{SKIP_ANYWHERE, is_skipped_package};
use crate::ui::{self, PINK, YELLOW};
use anyhow::{Result, bail};
use dialoguer::{Confirm, MultiSelect};
use indicatif::{ProgressBar, ProgressStyle};
use rayon::prelude::*;
use std::collections::{HashMap, HashSet};
use std::fs::{self, File};
use std::os::macos::fs::MetadataExt as _;
use std::os::unix::fs::{FileExt, MetadataExt};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, Instant};

#[derive(Clone)]
struct Cand {
    path: PathBuf,
    len: u64,
    mtime: i64,
}

struct Copy {
    path: PathBuf,
    /// bytes that deleting this copy would actually free
    frees: u64,
}

struct Group {
    len: u64,
    keep: Copy,
    dupes: Vec<Copy>,
}

impl Group {
    fn wasted(&self) -> u64 {
        self.dupes.iter().map(|d| d.frees).sum()
    }
}

fn collect(
    root: &Path,
    home: &Path,
    min: u64,
    out: &Mutex<Vec<Cand>>,
    seen: &Mutex<HashSet<(u64, u64)>>,
) {
    let Ok(rd) = fs::read_dir(root) else { return };
    let at_home = root == home;
    let mut subdirs = Vec::new();
    for e in rd.flatten() {
        let Ok(md) = e.metadata() else { continue };
        let name = e.file_name().to_string_lossy().into_owned();
        if md.is_dir() {
            if SKIP_ANYWHERE.contains(&name.as_str())
                || (at_home && name == "Library")
                || is_skipped_package(&name)
            {
                continue;
            }
            subdirs.push(e.path());
        } else if md.is_file()
            && md.len() >= min
            && md.blocks() > 0
            && md.st_flags() & SF_DATALESS == 0
        {
            // hard links share storage: they're not duplicates
            if md.nlink() > 1 && !seen.lock().unwrap().insert((md.dev(), md.ino())) {
                continue;
            }
            out.lock().unwrap().push(Cand {
                path: e.path(),
                len: md.len(),
                mtime: md.mtime(),
            });
        }
    }
    subdirs
        .par_iter()
        .for_each(|d| collect(d, home, min, out, seen));
}

fn partial_hash(c: &Cand) -> Option<[u8; 32]> {
    const CHUNK: u64 = 16 * 1024;
    let f = File::open(&c.path).ok()?;
    let mut h = blake3::Hasher::new();
    let mut buf = vec![0u8; CHUNK.min(c.len) as usize];
    f.read_exact_at(&mut buf, 0).ok()?;
    h.update(&buf);
    if c.len > CHUNK * 2 {
        f.read_exact_at(&mut buf, c.len - CHUNK).ok()?;
        h.update(&buf);
    }
    Some(*h.finalize().as_bytes())
}

fn full_hash(c: &Cand) -> Option<[u8; 32]> {
    let f = File::open(&c.path).ok()?;
    let mut h = blake3::Hasher::new();
    h.update_reader(f).ok()?;
    Some(*h.finalize().as_bytes())
}

fn regroup<K: std::hash::Hash + Eq + Send>(
    groups: Vec<Vec<Cand>>,
    key: impl Fn(&Cand) -> Option<K> + Sync,
) -> Vec<Vec<Cand>> {
    groups
        .into_par_iter()
        .flat_map_iter(|g| {
            let mut m: HashMap<K, Vec<Cand>> = HashMap::new();
            for c in g {
                if let Some(k) = key(&c) {
                    m.entry(k).or_default().push(c);
                }
            }
            m.into_values().filter(|v| v.len() > 1)
        })
        .collect()
}

pub fn run(
    path: Option<PathBuf>,
    min: u64,
    clean: bool,
    show: usize,
    home: &Path,
    guard: &Guard,
) -> Result<()> {
    let root = fs::canonicalize(path.unwrap_or_else(|| home.to_path_buf()))?;
    if clean && !interactive() {
        bail!("--clean needs an interactive terminal so you can review every duplicate");
    }
    let pb = if ui::fancy() {
        let pb = ProgressBar::new_spinner();
        pb.set_style(ProgressStyle::with_template("  {spinner:.magenta} {msg}").unwrap());
        pb.enable_steady_tick(Duration::from_millis(90));
        pb
    } else {
        ProgressBar::hidden()
    };
    let t = Instant::now();

    pb.set_message(format!(
        "Collecting files ≥ {} under {} …",
        ui::size(min),
        ui::tilde(&root, home)
    ));
    let cands = Mutex::new(Vec::new());
    collect(&root, home, min, &cands, &Mutex::new(HashSet::new()));
    let cands = cands.into_inner().unwrap();
    let scanned = cands.len();

    let mut by_len: HashMap<u64, Vec<Cand>> = HashMap::new();
    for c in cands {
        by_len.entry(c.len).or_default().push(c);
    }
    let groups: Vec<Vec<Cand>> = by_len.into_values().filter(|g| g.len() > 1).collect();
    pb.set_message(format!(
        "Comparing {} same-size files …",
        groups.iter().map(Vec::len).sum::<usize>()
    ));
    let groups = regroup(groups, partial_hash);
    let to_hash: u64 = groups.iter().flatten().map(|c| c.len).sum();
    pb.set_message(format!("Hashing {} of look-alikes …", ui::size(to_hash)));
    let groups = regroup(groups, full_hash);
    pb.finish_and_clear();

    let mut groups: Vec<Group> = groups
        .into_par_iter()
        .map(|mut g| {
            // keep the oldest copy (probably the original); tie-break on shortest path
            g.sort_by(|a, b| {
                a.mtime
                    .cmp(&b.mtime)
                    .then(a.path.as_os_str().len().cmp(&b.path.as_os_str().len()))
            });
            let mut copies = g.into_iter().map(|c| Copy {
                frees: fsx::private_size(&c.path).unwrap_or(c.len),
                path: c.path,
            });
            let keep = copies.next().unwrap();
            let len = fsx::lstat(&keep.path).map(|m| m.len()).unwrap_or(0);
            Group {
                len,
                keep,
                dupes: copies.collect(),
            }
        })
        .collect();
    groups.sort_by(|a, b| b.wasted().cmp(&a.wasted()).then(b.len.cmp(&a.len)));

    let total: u64 = groups.iter().map(Group::wasted).sum();
    let copies: usize = groups.iter().map(|g| g.dupes.len()).sum();
    if groups.is_empty() {
        println!(
            "  {}\n",
            ui::paint(
                format!("No duplicates among {scanned} files. Spotless. Suspiciously spotless."),
                YELLOW
            )
        );
        return Ok(());
    }

    let w = ui::width().min(110);
    for g in groups.iter().take(show) {
        let name = fsx::file_name(&g.keep.path);
        println!(
            "  {}  {}  {}",
            ui::bold(ui::rpad(&ui::size(g.wasted()), 9)),
            ui::paint(format!("×{}", g.dupes.len() + 1), PINK),
            ui::bold(ui::fit(&name, w.saturating_sub(24)))
        );
        println!(
            "      {}  {}",
            ui::paint("keep", ui::GREEN),
            ui::fit(&ui::tilde(&g.keep.path, home), w - 14)
        );
        for d in &g.dupes {
            let clone = d.frees < g.len / 10;
            let tail = if clone {
                ui::dim("  APFS clone — frees almost nothing")
            } else {
                String::new()
            };
            println!(
                "      {}  {}{}",
                ui::paint("dupe", ui::RED),
                ui::fit(&ui::tilde(&d.path, home), w - 50),
                tail
            );
        }
    }
    if groups.len() > show {
        println!(
            "  {}",
            ui::dim(format!(
                "… and {} more groups (use --show to see more)",
                groups.len() - show
            ))
        );
    }
    println!();
    crate::art::divider();
    println!(
        "   {}  {}   {}",
        ui::bold("DUPLICATE LOOT"),
        ui::rainbow(&ui::size(total)),
        ui::dim(format!(
            "{} in {} · {} files compared in {:.1}s",
            ui::plural(copies, "spare copy", "spare copies"),
            ui::plural(groups.len(), "group", "groups"),
            scanned,
            t.elapsed().as_secs_f64()
        ))
    );
    crate::art::divider();

    if !clean {
        println!(
            "   {}\n",
            ui::dim("Run with --clean to choose which copies go to the Trash.")
        );
        return Ok(());
    }

    // pick copies to trash (clones unchecked: trashing them frees nothing)
    let flat: Vec<(&Group, &Copy)> = groups
        .iter()
        .flat_map(|g| g.dupes.iter().map(move |d| (g, d)))
        .collect();
    let labels: Vec<String> = flat
        .iter()
        .map(|(_, d)| {
            format!(
                "{}  {}",
                ui::rpad(&ui::size(d.frees), 9),
                ui::fit(&ui::tilde(&d.path, home), w.saturating_sub(20))
            )
        })
        .collect();
    let defaults: Vec<bool> = flat.iter().map(|(g, d)| d.frees >= g.len / 10).collect();
    println!();
    let Some(picked) = MultiSelect::with_theme(&theme())
        .with_prompt("Spare copies to send to the Trash (the 'keep' copy always stays)")
        .items(&labels)
        .defaults(&defaults)
        .max_length(20)
        .report(false)
        .interact_opt()?
    else {
        println!("\n  {}\n", ui::paint("Curses! Foiled again!", ui::RED));
        return Ok(());
    };
    if picked.is_empty() {
        return Ok(());
    }
    let bytes: u64 = picked.iter().map(|&i| flat[i].1.frees).sum();
    let go = Confirm::with_theme(&theme())
        .with_prompt(format!(
            "Move {} copies ({}) to the Trash?",
            picked.len(),
            ui::size(bytes)
        ))
        .default(true)
        .interact_opt()?;
    if go != Some(true) {
        return Ok(());
    }
    let mut moved = 0u64;
    for &i in &picked {
        let (g, d) = flat[i];
        // paranoia: the keeper must still exist, unchanged, before we touch a copy
        if fsx::lstat(&g.keep.path).map(|m| m.len()) != Some(g.len) {
            println!(
                "  {} keeper vanished, skipping {}",
                ui::paint("◐", YELLOW),
                ui::tilde(&d.path, home)
            );
            continue;
        }
        match guard
            .check(&d.path)
            .map_err(|e| e.to_string())
            .and_then(|_| fsx::trash(&d.path))
        {
            Ok(()) => moved += d.frees,
            Err(e) => println!(
                "  {} {}: {e}",
                ui::paint("◐", YELLOW),
                ui::tilde(&d.path, home)
            ),
        }
    }
    crate::art::finale(0, moved);
    println!();
    Ok(())
}
