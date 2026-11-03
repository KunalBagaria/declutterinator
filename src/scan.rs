//! Runs every finder in parallel and assembles the results.

use crate::finders::{FINDERS, Found};
use crate::fsx::{self, Disk};
use crate::model::Category;
use crate::{catalog, ui};
use indicatif::{ProgressBar, ProgressStyle};
use rayon::prelude::*;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::{Duration, Instant};

pub struct Ctx {
    pub home: PathBuf,
    pub extra_roots: Vec<PathBuf>,
    pub stale_days: u64,
    pub download_age_days: u64,
    pub app_age_days: u64,
    pub large_min: u64,
    pub now: i64,
    /// Aimed at a stand-in home (DECLUTTER_HOME): leave system-wide tools
    /// like Homebrew, Docker and /Applications alone.
    pub sandboxed: bool,
}

impl Ctx {
    pub fn h(&self, rel: &str) -> PathBuf {
        self.home.join(rel)
    }

    pub fn guard(&self) -> fsx::Guard {
        fsx::Guard {
            home: self.home.clone(),
            roots: self.extra_roots.clone(),
        }
    }
}

pub struct Scan {
    pub cats: Vec<Category>,
    pub tips: Vec<String>,
    pub disk: Option<Disk>,
    pub elapsed: Duration,
}

impl Scan {
    pub fn total(&self) -> u64 {
        self.cats.iter().map(|c| c.bytes).sum()
    }
    pub fn total_for(&self, risk: crate::model::Risk) -> u64 {
        self.cats
            .iter()
            .filter(|c| c.meta.risk == risk)
            .map(|c| c.bytes)
            .sum()
    }
}

pub fn run(ctx: &Ctx, only: &[String], progress: bool) -> Scan {
    let start = Instant::now();
    let finders: Vec<_> = FINDERS
        .iter()
        .filter(|f| only.is_empty() || f.ids.iter().any(|id| only.iter().any(|o| o == id)))
        .filter(|f| !(ctx.sandboxed && f.system_wide))
        .collect();

    let pb = if progress {
        let pb = ProgressBar::new(finders.len() as u64);
        pb.set_style(
            ProgressStyle::with_template("  {spinner:.magenta} {prefix:.bold} {wide_msg:.dim}")
                .unwrap()
                .tick_strings(&[
                    "▹▹▹▹▹",
                    "▸▹▹▹▹",
                    "▹▸▹▹▹",
                    "▹▹▸▹▹",
                    "▹▹▹▸▹",
                    "▹▹▹▹▸",
                    "▪▪▪▪▪",
                ]),
        );
        pb.set_prefix("Sweeping the Tri-State Area");
        pb.enable_steady_tick(Duration::from_millis(90));
        pb
    } else {
        ProgressBar::hidden()
    };
    let running = Mutex::new(finders.iter().map(|f| f.label).collect::<Vec<_>>());
    pb.set_message(running.lock().unwrap().join(" · "));

    let found: Vec<Found> = finders
        .par_iter()
        .map(|f| {
            let r = (f.run)(ctx);
            let mut left = running.lock().unwrap();
            left.retain(|l| *l != f.label);
            for c in r.cats.iter().filter(|c| !c.is_empty()) {
                if only.is_empty() || only.iter().any(|o| o == c.meta.id) {
                    pb.println(format!(
                        "  {} {} {}",
                        ui::paint("✓", c.meta.risk.color()),
                        ui::leader(c.meta.name, 34),
                        ui::bold(ui::rpad(&c.size_str(), 9))
                    ));
                }
            }
            pb.set_message(left.join(" · "));
            pb.inc(1);
            r
        })
        .collect();
    pb.finish_and_clear();

    let mut cats: Vec<Category> = Vec::new();
    let mut tips: Vec<String> = Vec::new();
    for f in found {
        cats.extend(f.cats);
        tips.extend(f.tips);
    }
    cats.retain(|c| only.is_empty() || only.iter().any(|o| o == c.meta.id));
    cats.sort_by_key(|c| catalog::rank(c.meta.id));
    dedupe_nested(&mut cats);
    annotate(&mut cats, ctx);

    let denied: u64 = cats.iter().map(|c| c.denied).sum();
    cats.retain(|c| !c.is_empty());
    cats.sort_by(|a, b| a.meta.risk.cmp(&b.meta.risk).then(b.bytes.cmp(&a.bytes)));

    let disk = fsx::disk(&ctx.home);
    if denied > 0 && only.is_empty() {
        tips.push(format!(
            "{} folder{} were off-limits. To let the machine see everything, give your terminal Full Disk Access (System Settings → Privacy & Security → Full Disk Access).",
            denied,
            if denied == 1 { "" } else { "s" }
        ));
    }
    if let Some(d) = disk
        && only.is_empty()
        && d.used as f64 / d.total.max(1) as f64 > 0.9
    {
        tips.push("Disk is over 90% full. System Settings → General → Storage can offload Photos & iCloud Drive files you rarely use (\"Optimize Mac Storage\").".into());
    }
    tips.sort();
    tips.dedup();

    Scan {
        cats,
        tips,
        disk,
        elapsed: start.elapsed(),
    }
}

/// Notes that summarize items — computed after de-duplication so they match.
fn annotate(cats: &mut [Category], ctx: &Ctx) {
    for c in cats.iter_mut() {
        if c.meta.id == "project-artifacts" && !c.items.is_empty() {
            let stale = c.items.iter().filter(|i| i.preselect).count();
            let projects = ui::plural(c.items.len(), "project", "projects");
            c.note = Some(format!(
                "{projects}, {stale} idle for {}+ days",
                ctx.stale_days
            ));
        }
    }
}

/// If a path is also covered by another category (exactly, or by an
/// ancestor), keep it only once so totals are never double-counted.
/// `cats` must be in catalog order: earlier categories win exact ties.
fn dedupe_nested(cats: &mut [Category]) {
    let mut owner: HashMap<PathBuf, usize> = HashMap::new();
    for (ci, c) in cats.iter().enumerate() {
        for it in &c.items {
            for p in &it.paths {
                owner.entry(p.clone()).or_insert(ci);
            }
        }
    }
    for (ci, c) in cats.iter_mut().enumerate() {
        let mut changed = false;
        for it in &mut c.items {
            let before = it.paths.len();
            it.paths.retain(|p| {
                let exact_elsewhere = owner.get(p).is_some_and(|&o| o != ci);
                let ancestor_elsewhere = p
                    .ancestors()
                    .skip(1)
                    .any(|a| owner.get(a).is_some_and(|&o| o != ci));
                !(exact_elsewhere || ancestor_elsewhere)
            });
            if it.paths.len() != before {
                changed = true;
                it.bytes = if it.paths.is_empty() {
                    0
                } else {
                    fsx::measure_all(&it.paths).bytes
                };
            }
        }
        if changed {
            c.recompute();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Item;

    #[test]
    fn nested_paths_are_counted_once() {
        let dl = Item::new("foo", PathBuf::from("/h/Downloads/foo"), 100);
        let inner = Item::new("big", PathBuf::from("/h/Downloads/foo/big.mov"), 90);
        let dmg = Item::new("x.dmg", PathBuf::from("/h/Downloads/x.dmg"), 50);
        let elsewhere = Item::new("y", PathBuf::from("/h/Movies/y.mov"), 70);
        let mut cats = vec![
            Category::from_items("installers", vec![dmg.clone()], 0),
            Category::from_items("old-downloads", vec![dl], 0),
            Category::from_items("large-files", vec![inner, dmg, elsewhere], 0),
        ];
        dedupe_nested(&mut cats);
        assert_eq!(cats[0].items.len(), 1);
        assert_eq!(cats[1].items.len(), 1);
        let large: Vec<_> = cats[2].items.iter().map(|i| i.label.as_str()).collect();
        assert_eq!(large, vec!["y"]);
        assert_eq!(cats[2].bytes, 70);
    }
}
