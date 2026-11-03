//! `big`: where did all my disk go? A ranked, tree-shaped size explorer.

use crate::fsx::{self, Stats};
use crate::ui::{self, GREY};
use anyhow::{Context, Result};
use indicatif::{ProgressBar, ProgressStyle};
use rayon::prelude::*;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

/// When exploring `/`, these would double-count (APFS firmlinks) or wander off-disk.
const SKIP_FROM_ROOT: &[&str] = &[
    "/System/Volumes",
    "/Volumes",
    "/dev",
    "/net",
    "/home",
    "/private/var/vm",
];

struct Node {
    name: String,
    bytes: u64,
    is_dir: bool,
    children: Vec<Node>,
    /// children beyond the top-N, folded together
    rest: (usize, u64),
    denied: u64,
}

fn build(path: &Path, depth: usize, top: usize) -> Node {
    let name = fsx::file_name(path);
    let md = match fs::symlink_metadata(path) {
        Ok(m) => m,
        Err(_) => {
            return Node {
                name,
                bytes: 0,
                is_dir: false,
                children: vec![],
                rest: (0, 0),
                denied: 1,
            };
        }
    };
    if !md.is_dir() {
        return Node {
            name,
            bytes: fsx::allocated(&md),
            is_dir: false,
            children: vec![],
            rest: (0, 0),
            denied: 0,
        };
    }
    if depth == 0 {
        let st: Stats = fsx::measure(path);
        return Node {
            name,
            bytes: st.bytes,
            is_dir: true,
            children: vec![],
            rest: (0, 0),
            denied: st.denied,
        };
    }
    let kids: Vec<PathBuf> = match fs::read_dir(path) {
        Ok(rd) => rd
            .flatten()
            .map(|e| e.path())
            .filter(|p| !SKIP_FROM_ROOT.iter().any(|s| p == Path::new(s)))
            .collect(),
        Err(_) => {
            return Node {
                name,
                bytes: fsx::allocated(&md),
                is_dir: true,
                children: vec![],
                rest: (0, 0),
                denied: 1,
            };
        }
    };
    let mut children: Vec<Node> = kids
        .par_iter()
        .map(|k| build(k, depth - 1, (top / 2).max(3)))
        .collect();
    children.sort_by(|a, b| b.bytes.cmp(&a.bytes));
    let bytes = fsx::allocated(&md) + children.iter().map(|c| c.bytes).sum::<u64>();
    let denied = children.iter().map(|c| c.denied).sum();
    let mut rest = (0, 0);
    if children.len() > top {
        for c in children.drain(top..) {
            rest.0 += 1;
            rest.1 += c.bytes;
        }
    }
    Node {
        name,
        bytes,
        is_dir: true,
        children,
        rest,
        denied,
    }
}

pub fn run(path: Option<PathBuf>, top: usize, depth: usize, home: &Path) -> Result<()> {
    let path = path.unwrap_or_else(|| home.to_path_buf());
    let path = fs::canonicalize(&path).with_context(|| format!("can't open {}", path.display()))?;
    let pb = if ui::fancy() {
        let pb = ProgressBar::new_spinner();
        pb.set_style(ProgressStyle::with_template("  {spinner:.magenta} {msg}").unwrap());
        pb.set_message(format!("Weighing {} …", ui::tilde(&path, home)));
        pb.enable_steady_tick(Duration::from_millis(90));
        pb
    } else {
        ProgressBar::hidden()
    };
    let t = Instant::now();
    let root = build(&path, depth.max(1), top);
    pb.finish_and_clear();

    let w = ui::width().min(110);
    println!(
        "  {}  {}",
        ui::bold(ui::tilde(&path, home)),
        ui::bold(ui::rainbow(&ui::size(root.bytes)))
    );
    let name_w = (widest(&root.children, 3) + 1).clamp(16, w.saturating_sub(36).max(16));
    render(&root.children, root.rest, root.bytes.max(1), "", name_w);
    println!();
    if root.denied > 0 {
        println!(
            "  {}",
            ui::dim(format!(
                "{} unreadable folders skipped — Full Disk Access reveals them.",
                root.denied
            ))
        );
    }
    println!(
        "  {}\n",
        ui::dim(format!("Weighed in {:.1}s.", t.elapsed().as_secs_f64()))
    );
    Ok(())
}

/// Widest "tree prefix + name/" among the nodes that will be printed.
fn widest(nodes: &[Node], indent: usize) -> usize {
    nodes
        .iter()
        .map(|n| {
            (indent + n.name.chars().count() + n.is_dir as usize)
                .max(widest(&n.children, indent + 3))
        })
        .max()
        .unwrap_or(0)
}

fn render(nodes: &[Node], rest: (usize, u64), parent: u64, prefix: &str, name_w: usize) {
    let count = nodes.len() + (rest.0 > 0) as usize;
    for (i, n) in nodes.iter().enumerate() {
        let last = i + 1 == count;
        let branch = if last { "└─ " } else { "├─ " };
        let frac = n.bytes as f64 / parent as f64;
        let name = if n.is_dir {
            format!("{}/", n.name)
        } else {
            n.name.clone()
        };
        let indent_w = prefix.chars().count() + 3;
        let label_w = name_w.saturating_sub(indent_w).max(8);
        let bar_w = 20;
        println!(
            "  {}{}{} {} {} {}",
            ui::paint(prefix, GREY),
            ui::paint(branch, GREY),
            ui::pad(&ui::fit(&name, label_w), label_w),
            ui::bold(ui::rpad(&ui::size(n.bytes), 9)),
            ui::gradient_bar(frac, bar_w),
            ui::dim(format!("{:>3.0}%", frac * 100.0))
        );
        if !n.children.is_empty() {
            let next = format!("{prefix}{}", if last { "   " } else { "│  " });
            render(&n.children, n.rest, n.bytes.max(1), &next, name_w);
        }
    }
    if rest.0 > 0 {
        println!(
            "  {}{}{}",
            ui::paint(prefix, GREY),
            ui::paint("└─ ", GREY),
            ui::dim(format!("… {} more ({})", rest.0, ui::size(rest.1)))
        );
    }
}
