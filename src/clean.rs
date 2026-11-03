//! Choosing targets and firing the ray.

use crate::model::{Action, Category, Estimate, Item, Risk};
use crate::scan::{Ctx, Scan};
use crate::ui::{self, GREY, PINK, RED, YELLOW};
use crate::{art, catalog, fsx, report, shell};
use anyhow::{Result, bail};
use console::style;
use dialoguer::theme::ColorfulTheme;
use dialoguer::{Confirm, MultiSelect};
use indicatif::{ProgressBar, ProgressStyle};
use std::io::IsTerminal;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::Duration;

pub struct Opts {
    pub dry_run: bool,
    pub yes: bool,
    pub safe: bool,
    pub ids: Vec<String>,
}

pub fn interactive() -> bool {
    std::io::stdin().is_terminal() && std::io::stderr().is_terminal()
}

pub fn theme() -> ColorfulTheme {
    ColorfulTheme {
        checked_item_prefix: style("✸".to_string()).for_stderr().color256(PINK),
        unchecked_item_prefix: style("○".to_string()).for_stderr().color256(240),
        active_item_prefix: style("▶".to_string()).for_stderr().color256(ui::CYAN),
        inactive_item_prefix: style(" ".to_string()).for_stderr(),
        prompt_prefix: style("⚡".to_string()).for_stderr().color256(YELLOW),
        success_prefix: style("✓".to_string()).for_stderr().color256(ui::GREEN),
        ..ColorfulTheme::default()
    }
}

fn page_len() -> usize {
    console::Term::stderr()
        .size_checked()
        .map(|(h, _)| (h as usize).saturating_sub(8).clamp(5, 30))
        .unwrap_or(15)
}

struct Plan<'a> {
    cat: &'a Category,
    items: Vec<&'a Item>,
}

impl Plan<'_> {
    fn bytes(&self) -> u64 {
        if self.items.is_empty() {
            self.cat.bytes
        } else {
            self.items.iter().map(|i| i.bytes).sum()
        }
    }
    fn is_command(&self) -> bool {
        matches!(self.cat.meta.action, Action::Command { .. })
    }
}

pub fn run(ctx: &Ctx, scan: &Scan, opts: &Opts) -> Result<()> {
    let tty = interactive();
    for id in &opts.ids {
        if !catalog::exists(id) {
            bail!("unknown category '{id}' — see `declutterinator categories`");
        }
    }
    if scan.cats.is_empty() {
        println!("  {}", ui::paint(art::pick(art::NOTHING_LINES), YELLOW));
        return Ok(());
    }

    // 1. which categories?
    let chosen: Vec<&Category> = if !opts.ids.is_empty() {
        for id in &opts.ids {
            if !scan.cats.iter().any(|c| c.meta.id == id) {
                println!("  {} nothing to clean for {}", ui::dim("·"), ui::bold(id));
            }
        }
        scan.cats
            .iter()
            .filter(|c| opts.ids.iter().any(|i| i == c.meta.id))
            .collect()
    } else if opts.safe {
        scan.cats
            .iter()
            .filter(|c| c.meta.risk == Risk::Safe)
            .collect()
    } else if tty {
        match pick_categories(scan)? {
            Some(c) => c,
            None => return cancelled(),
        }
    } else {
        bail!(
            "not running in a terminal — choose targets with --safe or --category <id> (and --yes)"
        );
    };

    // 2. which items?
    let mut plan: Vec<Plan> = Vec::new();
    for cat in chosen {
        let items: Vec<&Item> = if matches!(cat.meta.action, Action::Command { .. }) {
            vec![]
        } else if cat.meta.pick_items && tty && !opts.yes {
            match pick_items(cat, ctx)? {
                Some(items) => items,
                None => return cancelled(),
            }
        } else if cat.meta.pick_items {
            cat.items.iter().filter(|i| i.preselect).collect()
        } else {
            cat.items.iter().collect()
        };
        if items.is_empty() && !matches!(cat.meta.action, Action::Command { .. }) {
            if cat.meta.pick_items && !tty {
                println!(
                    "  {} {} needs you to pick items interactively — skipped",
                    ui::dim("·"),
                    cat.meta.name
                );
            }
            continue;
        }
        plan.push(Plan { cat, items });
    }
    if plan.is_empty() {
        return cancelled();
    }

    // 3. show the plan
    print_plan(&plan, ctx, opts.dry_run);
    if opts.dry_run {
        println!(
            "  {}\n",
            ui::paint("Dry run — not a single byte was harmed.", ui::GREEN)
        );
        return Ok(());
    }

    // 4. last chance
    if !opts.yes {
        if !tty {
            bail!("refusing to clean without confirmation — pass --yes");
        }
        let go = Confirm::with_theme(&theme())
            .with_prompt("Fire the DECLUTTER-INATOR?")
            .default(true)
            .interact_opt()?;
        if go != Some(true) {
            return cancelled();
        }
    }

    // 5. fire!
    execute(ctx, &plan, tty && !opts.yes)
}

fn cancelled() -> Result<()> {
    println!("\n  {}\n", ui::paint(art::pick(art::CANCEL_LINES), RED));
    Ok(())
}

fn pick_categories(scan: &Scan) -> Result<Option<Vec<&Category>>> {
    let w = ui::width().saturating_sub(8).min(96);
    let labels: Vec<String> = scan
        .cats
        .iter()
        .map(|c| {
            let tag = ui::paint(format!("{:<5}", c.meta.risk.tag()), c.meta.risk.color());
            let hint = match (c.meta.action, c.meta.pick_items) {
                (Action::Command { program, .. }, _) => format!("via {program}"),
                (Action::Trash, true) => "you pick → Trash".into(),
                (Action::Trash, false) => "→ Trash".into(),
                (Action::Delete, true) => "you pick".into(),
                (Action::Delete, false) => String::new(),
            };
            let hint = match &c.note {
                Some(n) if hint.is_empty() => n.clone(),
                Some(n) => format!("{hint} · {n}"),
                None => hint,
            };
            let core = format!(
                "{tag} {} {}",
                ui::pad(&ui::fit(c.meta.name, 30), 30),
                ui::rpad(&c.size_str(), 9)
            );
            let room = w.saturating_sub(48);
            format!("{core}  {}", ui::dim(ui::fit(&hint, room)))
        })
        .collect();
    let defaults: Vec<bool> = scan
        .cats
        .iter()
        .map(|c| c.meta.risk == Risk::Safe)
        .collect();
    println!(
        "  {}",
        ui::dim("↑/↓ move · space toggle · a toggle all · enter FIRE · esc retreat")
    );
    let picked = MultiSelect::with_theme(&theme())
        .with_prompt("Choose your targets")
        .items(&labels)
        .defaults(&defaults)
        .max_length(page_len())
        .report(false)
        .interact_opt()?;
    Ok(picked.map(|idx| idx.into_iter().map(|i| &scan.cats[i]).collect()))
}

fn pick_items<'a>(cat: &'a Category, _ctx: &Ctx) -> Result<Option<Vec<&'a Item>>> {
    if cat.items.len() == 1 && cat.meta.risk == Risk::Moderate && cat.items[0].preselect {
        return Ok(Some(vec![&cat.items[0]]));
    }
    let w = ui::width().saturating_sub(8).min(110);
    let lw = report::label_width(&cat.items, w);
    let labels: Vec<String> = cat
        .items
        .iter()
        .map(|it| report::item_line(it, w, lw))
        .collect();
    let defaults: Vec<bool> = cat.items.iter().map(|i| i.preselect).collect();
    println!();
    println!(
        "  {} {}",
        ui::bold(cat.meta.name),
        ui::dim(format!("— {}", cat.meta.desc))
    );
    let picked = MultiSelect::with_theme(&theme())
        .with_prompt(format!(
            "Pick the victims ({} → {})",
            cat.items.len(),
            cat.meta.action.verb()
        ))
        .items(&labels)
        .defaults(&defaults)
        .max_length(page_len())
        .report(false)
        .interact_opt()?;
    Ok(picked.map(|idx| idx.into_iter().map(|i| &cat.items[i]).collect()))
}

fn print_plan(plan: &[Plan], ctx: &Ctx, list_paths: bool) {
    println!();
    println!("  {}", ui::bold(ui::paint("▌THE PLAN", PINK)));
    let mut total = 0;
    let mut approx = false;
    for p in plan {
        let bytes = p.bytes();
        total += bytes;
        approx |= p.cat.estimate != Estimate::Exact;
        let what = if p.is_command() {
            p.cat.meta.action.verb()
        } else {
            format!(
                "{} ({})",
                p.cat.meta.action.verb(),
                ui::plural(p.items.len(), "item", "items")
            )
        };
        println!(
            "    {} {} {}  {}",
            ui::paint("✸", p.cat.meta.risk.color()),
            ui::leader(p.cat.meta.name, 34),
            ui::bold(ui::rpad(
                &format!("{}{}", p.cat.estimate.prefix(), ui::size(bytes)),
                9
            )),
            ui::dim(what)
        );
        if list_paths {
            for it in &p.items {
                let size = ui::dim(ui::rpad(&ui::size(it.bytes), 9));
                if let [only] = it.paths.as_slice() {
                    println!("        {size} {}", ui::tilde(only, &ctx.home));
                } else {
                    println!("        {size} {}", it.label);
                    for path in &it.paths {
                        println!(
                            "        {}  {}",
                            " ".repeat(9),
                            ui::dim(format!("└ {}", ui::tilde(path, &ctx.home)))
                        );
                    }
                }
            }
        }
    }
    println!("    {}", ui::paint("─".repeat(46), ui::DARK));
    println!(
        "    {} {}",
        ui::pad(&ui::bold("Total"), 36),
        ui::bold(ui::rpad(
            &ui::rainbow(&format!(
                "{}{}",
                if approx { "~" } else { "" },
                ui::size(total)
            )),
            9
        ))
    );
    println!();
}

#[derive(Default)]
struct Outcome {
    freed: u64,
    trashed: u64,
    problems: Vec<(String, String)>,
}

fn execute(ctx: &Ctx, plan: &[Plan], offer_empty_trash: bool) -> Result<()> {
    let guard = ctx.guard();
    let before = fsx::disk(&ctx.home);
    art::charge();
    println!(
        "  {}",
        ui::bold(ui::rainbow("━━━ FIRING THE DECLUTTER-INATOR ━━━"))
    );
    println!();
    let mut out = Outcome::default();

    for p in plan {
        let name = p.cat.meta.name;
        println!("  {}", ui::dim(format!("❝ {} ❞", p.cat.meta.quip)));
        let (freed, trashed, problem) = match p.cat.meta.action {
            Action::Delete => delete(p, &guard),
            Action::Trash => trash(p, &guard),
            Action::Command { program, args } => command(p.cat, program, args, ctx),
        };
        out.freed += freed;
        out.trashed += trashed;
        let result = if trashed > 0 {
            format!("{} → Trash", ui::size(trashed))
        } else if freed > 0 {
            format!(
                "{}{} vaporized",
                if p.is_command() { "~" } else { "" },
                ui::size(freed)
            )
        } else {
            "nothing freed".into()
        };
        let mark = if problem.is_some() {
            ui::paint("◐", YELLOW)
        } else {
            ui::paint("✓", ui::GREEN)
        };
        art::disintegrate(
            "  ",
            name,
            &format!("{mark} {}  {}", ui::pad(name, 32), ui::bold(result)),
        );
        if let Some(prob) = problem {
            out.problems.push((name.to_string(), prob));
        }
    }

    if !out.problems.is_empty() {
        println!();
        println!("  {}", ui::paint("Some clutter resisted:", YELLOW));
        for (name, prob) in &out.problems {
            for (i, line) in
                report::wrap(&format!("{name}: {prob}"), ui::width().saturating_sub(10))
                    .iter()
                    .enumerate()
            {
                println!(
                    "    {} {}",
                    if i == 0 {
                        ui::paint("•", YELLOW)
                    } else {
                        " ".into()
                    },
                    ui::dim(line)
                );
            }
        }
    }

    if out.trashed > 0 && offer_empty_trash {
        println!();
        let empty = Confirm::with_theme(&theme())
            .with_prompt(format!(
                "Empty the Trash now to actually free {}? (permanently deletes EVERYTHING in the Trash)",
                ui::size(out.trashed)
            ))
            .default(false)
            .interact_opt()?;
        if empty == Some(true) {
            let (freed, problem) = empty_trash(ctx, &guard);
            out.freed += freed;
            out.trashed = out.trashed.saturating_sub(freed);
            match problem {
                Some(p) => println!("  {} {}", ui::paint("◐", YELLOW), ui::dim(p)),
                None => println!(
                    "  {} Trash emptied ({})",
                    ui::paint("✓", ui::GREEN),
                    ui::size(freed)
                ),
            }
        }
    }

    art::finale(out.freed, out.trashed);
    if let (Some(b), Some(a)) = (before, fsx::disk(&ctx.home)) {
        println!(
            "  {}",
            ui::dim(format!(
                "Free space: {} → {}  (APFS can take a minute to catch up)",
                ui::size(b.free),
                ui::size(a.free)
            ))
        );
    }
    println!();
    Ok(())
}

fn progress_bar(total: u64, name: &str) -> ProgressBar {
    if !ui::fancy() {
        return ProgressBar::hidden();
    }
    let pb = ProgressBar::new(total.max(1));
    pb.set_style(
        ProgressStyle::with_template(
            "  {spinner:.magenta} {prefix:32!} {bar:28.magenta/238} {decimal_bytes:>9} / {decimal_total_bytes}",
        )
        .unwrap()
        .progress_chars("━╸ ")
        .tick_strings(&["◐", "◓", "◑", "◒", "●"]),
    );
    pb.set_prefix(name.to_string());
    pb.enable_steady_tick(Duration::from_millis(80));
    pb
}

fn problem_text(failed: u64, first: Option<String>) -> Option<String> {
    (failed > 0).then(|| {
        format!(
            "{} couldn't be removed (in use or protected). First one: {}",
            ui::plural(failed as usize, "entry", "entries"),
            first.unwrap_or_default()
        )
    })
}

fn delete(p: &Plan, guard: &fsx::Guard) -> (u64, u64, Option<String>) {
    let progress = AtomicU64::new(0);
    let done = AtomicBool::new(false);
    let pb = progress_bar(p.bytes(), p.cat.meta.name);
    let mut failed = 0u64;
    let mut first: Option<String> = None;
    let mut freed = 0u64;
    std::thread::scope(|s| {
        s.spawn(|| {
            while !done.load(Ordering::Relaxed) {
                pb.set_position(progress.load(Ordering::Relaxed));
                std::thread::sleep(Duration::from_millis(50));
            }
        });
        for it in &p.items {
            for path in &it.paths {
                if let Err(why) = guard.check(path) {
                    failed += 1;
                    first.get_or_insert_with(|| {
                        format!("{}: blocked by safety guard ({why})", path.display())
                    });
                    continue;
                }
                let r = fsx::remove_tree(path, &progress);
                freed += r.freed;
                failed += r.failed;
                if first.is_none() {
                    first = r.first_error;
                }
            }
        }
        done.store(true, Ordering::Relaxed);
    });
    pb.finish_and_clear();
    (freed, 0, problem_text(failed, first))
}

fn trash(p: &Plan, guard: &fsx::Guard) -> (u64, u64, Option<String>) {
    let pb = progress_bar(p.bytes(), p.cat.meta.name);
    let mut moved = 0u64;
    let mut failed = 0u64;
    let mut first: Option<String> = None;
    for it in &p.items {
        let mut ok = true;
        for path in &it.paths {
            if let Err(why) = guard.check(path) {
                ok = false;
                failed += 1;
                first.get_or_insert_with(|| {
                    format!("{}: blocked by safety guard ({why})", path.display())
                });
                continue;
            }
            if !fsx::exists(path) {
                continue;
            }
            if let Err(e) = fsx::trash(path) {
                ok = false;
                failed += 1;
                first.get_or_insert_with(|| format!("{}: {e}", path.display()));
            }
        }
        if ok {
            moved += it.bytes;
        }
        pb.inc(it.bytes);
    }
    pb.finish_and_clear();
    (0, moved, problem_text(failed, first))
}

fn command(cat: &Category, program: &str, args: &[&str], ctx: &Ctx) -> (u64, u64, Option<String>) {
    let Some(bin) = shell::which(program) else {
        return (0, 0, Some(format!("`{program}` isn't installed")));
    };
    println!(
        "    {} {}",
        ui::paint("▸", ui::CYAN),
        ui::dim(format!("{program} {}", args.join(" ")))
    );
    let before = fsx::disk(&ctx.home).map(|d| d.free).unwrap_or(0);
    let mut shown = 0usize;
    let mut hidden = 0usize;
    let w = ui::width().saturating_sub(8);
    let res = shell::stream(&bin, args, |line| {
        let line = line.trim_end();
        if line.is_empty() {
            return;
        }
        if shown < 6 {
            println!("      {}", ui::paint(ui::fit(line, w), GREY));
            shown += 1;
        } else {
            hidden += 1;
            if ui::fancy() {
                print!(
                    "\r\x1b[2K      {}",
                    ui::dim(format!("… {} more lines", hidden))
                );
                let _ = std::io::Write::flush(&mut std::io::stdout());
            }
        }
    });
    if hidden > 0 {
        if ui::fancy() {
            println!();
        } else {
            println!("      … {hidden} more lines");
        }
    }
    let after = fsx::disk(&ctx.home).map(|d| d.free).unwrap_or(0);
    let measured = after.saturating_sub(before);
    match res {
        Ok(true) => {
            let freed = match cat.estimate {
                Estimate::UpTo => measured,
                _ => cat.bytes.max(measured),
            };
            (freed, 0, None)
        }
        Ok(false) => (
            measured,
            0,
            Some(format!("`{program}` reported an error (see output above)")),
        ),
        Err(e) => (0, 0, Some(format!("couldn't run `{program}`: {e}"))),
    }
}

fn empty_trash(ctx: &Ctx, guard: &fsx::Guard) -> (u64, Option<String>) {
    let dir = ctx.h(".Trash");
    let kids = match fsx::children(&dir) {
        Ok(k) => k,
        Err(_) => {
            return (0, Some("macOS wouldn't let the machine into the Trash — empty it from the Finder instead (or grant Full Disk Access).".into()));
        }
    };
    let progress = AtomicU64::new(0);
    let mut freed = 0;
    let mut failed = 0;
    for k in kids {
        if guard.check(&k).is_err() {
            failed += 1;
            continue;
        }
        let r = fsx::remove_tree(&k, &progress);
        freed += r.freed;
        failed += r.failed;
    }
    (
        freed,
        (failed > 0).then(|| format!("{failed} entries in the Trash couldn't be removed")),
    )
}
