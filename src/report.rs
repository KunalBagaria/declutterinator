//! Rendering scan results for humans (and `--json` for machines).

use crate::art;
use crate::model::{Category, CategoryJson, Item, Risk};
use crate::scan::{Ctx, Scan};
use crate::ui::{self, DARK, PINK, YELLOW};
use serde::Serialize;

pub fn width() -> usize {
    ui::width().min(104)
}

/// "DISK ████▒▒▒░░░  83%  412 GB of 494 GB used · 82 GB free"
pub fn disk_gauge(scan: &Scan) {
    let Some(d) = scan.disk else { return };
    let w = ui::width().saturating_sub(58).clamp(10, 34);
    let total = d.total.max(1) as f64;
    let used_frac = d.used as f64 / total;
    let loot_frac =
        (scan.total_for(Risk::Safe) + scan.total_for(Risk::Moderate)).min(d.used) as f64 / total;
    let solid = ((used_frac - loot_frac) * w as f64).round().max(0.0) as usize;
    let loot = ((loot_frac * w as f64).round() as usize).min(w - solid.min(w));
    let free = w.saturating_sub(solid + loot);
    let fill = match used_frac {
        f if f >= 0.9 => ui::RED,
        f if f >= 0.75 => ui::ORANGE,
        _ => ui::GREEN,
    };
    let bar = if ui::fancy() {
        format!(
            "{}{}{}",
            ui::paint("█".repeat(solid), fill),
            ui::paint("▒".repeat(loot), PINK),
            ui::paint("░".repeat(free), DARK)
        )
    } else {
        format!(
            "[{}{}{}]",
            "#".repeat(solid),
            "+".repeat(loot),
            ".".repeat(free)
        )
    };
    println!(
        "  {}  {}  {}  {}",
        ui::bold("DISK"),
        bar,
        ui::bold(format!("{:.0}%", used_frac * 100.0)),
        ui::dim(format!(
            "{} of {} used · {} free",
            ui::size(d.used),
            ui::size(d.total),
            ui::size(d.free)
        ))
    );
    if loot > 0 {
        println!(
            "        {} {}",
            ui::paint("▒", PINK),
            ui::dim("= what the machine can reclaim without touching your own files")
        );
    }
    println!();
}

fn top_items_line(c: &Category, room: usize) -> Option<String> {
    // a single fixed folder (e.g. ~/.npm/_cacache) needs no breakdown
    let list_items = c.items.len() >= 2 || c.meta.pick_items || c.meta.id == "trash";
    if c.items.is_empty() || !list_items {
        return c.note.clone();
    }
    let mut parts = Vec::new();
    let mut used = c.note.as_ref().map_or(0, |n| n.chars().count() + 3);
    for it in c.items.iter().take(3) {
        let s = format!(
            "{} {}",
            ui::fit(short_label(&it.label), 28),
            ui::size(it.bytes)
        );
        if !parts.is_empty() && used + s.chars().count() + 3 > room {
            break;
        }
        used += s.chars().count() + 3;
        parts.push(s);
    }
    let rest = c.items.len() - parts.len();
    let mut line = parts.join(" · ");
    if rest > 0 {
        line.push_str(&format!(" · +{rest} more"));
    }
    Some(match &c.note {
        Some(n) => format!("{n} — {line}"),
        None => line,
    })
}

/// Last path component for compact listings.
fn short_label(label: &str) -> &str {
    if label.starts_with('~') || label.starts_with('/') {
        label.rsplit('/').next().unwrap_or(label)
    } else {
        label
    }
}

/// Width for the label column so labels and details share the line fairly.
pub fn label_width(items: &[Item], w: usize) -> usize {
    let longest = items
        .iter()
        .map(|i| console::measure_text_width(&i.label))
        .max()
        .unwrap_or(10);
    let has_detail = items.iter().any(|i| i.detail.is_some());
    let room = w.saturating_sub(18 + if has_detail { 24 } else { 0 });
    longest.clamp(10, room.max(10))
}

pub fn item_line(it: &Item, w: usize, label_w: usize) -> String {
    let age = it
        .age_days
        .map(|d| format!("{:>6}", ui::age(d)))
        .unwrap_or_else(|| " ".repeat(6));
    let detail_w = w.saturating_sub(18 + label_w + 2);
    let detail = it
        .detail
        .as_deref()
        .map(|d| format!("  {}", console::truncate_str(d, detail_w, "…")))
        .unwrap_or_default();
    format!(
        "{}  {}  {}{}",
        ui::rpad(&ui::size(it.bytes), 9),
        ui::dim(age),
        ui::pad(&ui::fit(&it.label, label_w), label_w),
        ui::dim(detail)
    )
}

pub fn print(scan: &Scan, ctx: &Ctx, verbose: bool) {
    let w = width();
    disk_gauge(scan);

    if scan.cats.is_empty() {
        println!("  {}\n", ui::paint(art::pick(art::NOTHING_LINES), YELLOW));
    }

    let biggest = scan.cats.iter().map(|c| c.bytes).max().unwrap_or(1).max(1);
    let name_w = 36;
    let bar_w = w.saturating_sub(name_w + 9 + 10).clamp(8, 26);

    for risk in [Risk::Safe, Risk::Moderate, Risk::Review] {
        let cats: Vec<&Category> = scan.cats.iter().filter(|c| c.meta.risk == risk).collect();
        if cats.is_empty() {
            continue;
        }
        let total = scan.total_for(risk);
        let head = format!("▌{}", risk.title());
        let blurb = risk.blurb();
        // right-align the section total with the end of the bars below it
        let row_w = 4 + (name_w - 2) + 1 + 9 + 2 + bar_w;
        let head_w = 2 + console::measure_text_width(&head) + 2 + blurb.chars().count();
        println!(
            "  {}  {}{}{}",
            ui::bold(ui::paint(&head, risk.color())),
            ui::dim(blurb),
            " ".repeat(row_w.saturating_sub(head_w + 9).max(1)),
            ui::bold(ui::paint(ui::rpad(&ui::size(total), 9), risk.color()))
        );
        for c in cats {
            let frac = c.bytes as f64 / biggest as f64;
            println!(
                "    {} {}  {}",
                ui::leader(c.meta.name, name_w - 2),
                ui::bold(ui::rpad(&c.size_str(), 9)),
                ui::gradient_bar(frac, bar_w)
            );
            if verbose {
                if let Some(n) = &c.note {
                    println!("      {}", ui::dim(n));
                }
                let shown = &c.items[..c.items.len().min(25)];
                let lw = label_width(shown, w - 6);
                for it in shown {
                    println!("      {}", item_line(it, w - 6, lw));
                }
                if c.items.len() > 25 {
                    println!(
                        "      {}",
                        ui::dim(format!("… and {} more", c.items.len() - 25))
                    );
                }
            } else if let Some(line) = top_items_line(c, w.saturating_sub(10)) {
                println!("      {}", ui::dim(ui::fit(&line, w - 8)));
            }
            let shared = c.linked_bytes();
            if c.meta.risk != Risk::Safe && shared > c.bytes / 4 && shared > 100_000_000 {
                println!(
                    "      {}",
                    ui::paint(
                        format!(
                            "⚠ {} of this is shared with other files (hard links / APFS clones) and may not be freed",
                            ui::size(shared)
                        ),
                        YELLOW
                    )
                );
            }
        }
        println!();
    }

    if !scan.tips.is_empty() {
        println!("  {}", ui::bold(ui::paint("▌TIPS", ui::CYAN)));
        for t in &scan.tips {
            for (i, line) in wrap(t, w - 8).iter().enumerate() {
                let bullet = if i == 0 {
                    ui::paint("•", ui::CYAN)
                } else {
                    " ".into()
                };
                println!("    {bullet} {line}");
            }
        }
        println!();
    }

    if let Some(top) = scan
        .cats
        .iter()
        .filter(|c| c.meta.risk != Risk::Review)
        .max_by_key(|c| c.bytes)
    {
        println!(
            "  {} {}",
            ui::paint("❝", PINK),
            ui::dim(format!("{} — on {}", top.meta.quip, top.meta.name))
        );
        println!();
    }

    let safe = scan.total_for(Risk::Safe);
    let moderate = scan.total_for(Risk::Moderate);
    let review = scan.total_for(Risk::Review);
    art::divider();
    println!(
        "   {}  {}   {}",
        ui::bold("POTENTIAL LOOT"),
        ui::rainbow(&ui::size(safe + moderate + review)),
        ui::dim(format!(
            "{} zero-regret · {} probably fine · {} your call",
            ui::size(safe),
            ui::size(moderate),
            ui::size(review)
        ))
    );
    art::divider();
    let home = if ctx.extra_roots.is_empty() {
        "your home folder".to_string()
    } else {
        format!(
            "your home folder + {}",
            ui::plural(ctx.extra_roots.len(), "extra root", "extra roots")
        )
    };
    println!(
        "   {}",
        ui::dim(format!(
            "Scanned {home} in {:.1}s.",
            scan.elapsed.as_secs_f64()
        ))
    );
}

pub fn wrap(text: &str, w: usize) -> Vec<String> {
    let mut lines = Vec::new();
    let mut cur = String::new();
    for word in text.split_whitespace() {
        if !cur.is_empty() && cur.chars().count() + 1 + word.chars().count() > w {
            lines.push(std::mem::take(&mut cur));
        }
        if !cur.is_empty() {
            cur.push(' ');
        }
        cur.push_str(word);
    }
    if !cur.is_empty() {
        lines.push(cur);
    }
    lines
}

#[derive(Serialize)]
struct Json<'a> {
    version: &'a str,
    disk: Option<crate::fsx::Disk>,
    reclaimable_bytes: u64,
    categories: Vec<CategoryJson<'a>>,
    tips: &'a [String],
    elapsed_secs: f64,
}

pub fn json(scan: &Scan) -> String {
    let j = Json {
        version: env!("CARGO_PKG_VERSION"),
        disk: scan.disk,
        reclaimable_bytes: scan.total(),
        categories: scan.cats.iter().map(CategoryJson::from).collect(),
        tips: &scan.tips,
        elapsed_secs: scan.elapsed.as_secs_f64(),
    };
    serde_json::to_string_pretty(&j).unwrap_or_else(|_| "{}".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wraps_words() {
        let w = wrap("the quick brown fox jumps over the lazy dog", 10);
        assert!(w.iter().all(|l| l.chars().count() <= 10));
        assert_eq!(w.join(" "), "the quick brown fox jumps over the lazy dog");
    }
}
