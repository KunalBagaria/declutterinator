//! BEHOLD — the DECLUTTER-INATOR!
//!
//! A macOS disk analyzer and cleaner. It sweeps your home folder for caches,
//! build artifacts, stale downloads and other clutter, explains what's safe,
//! and — once you approve — reclaims the space.

mod art;
mod big;
mod catalog;
mod clean;
mod dupes;
mod finders;
mod fsx;
mod model;
mod report;
mod scan;
mod shell;
mod sweep;
mod ui;

use anyhow::{Context, Result, bail};
use clap::{Args, Parser, Subcommand};
use model::Risk;
use std::path::PathBuf;

#[derive(Parser)]
#[command(
    name = "declutterinator",
    version,
    about = "BEHOLD! The DECLUTTER-INATOR — finds wasted disk space on macOS and zaps it.",
    long_about = "BEHOLD! The DECLUTTER-INATOR — finds wasted disk space on macOS and zaps it.\n\n\
        Run it with no arguments for the full experience: scan, report, pick targets, fire.\n\
        Nothing is ever removed without your confirmation, personal files go to the Trash,\n\
        and a safety guard refuses to touch anything outside your home folder.",
    after_help = "Examples:\n  \
        declutterinator                      scan, report, then choose what to zap\n  \
        declutterinator scan -v              detailed read-only report\n  \
        declutterinator clean --dry-run      show exactly what would be removed\n  \
        declutterinator clean --safe --yes   zap every zero-regret category, no questions\n  \
        declutterinator clean -c npm-cache -c xcode-derived-data\n  \
        declutterinator big ~/Library -d 2   where did my disk go?\n  \
        declutterinator dupes ~/Pictures     find duplicate files"
)]
struct Cli {
    #[command(subcommand)]
    cmd: Option<Cmd>,
    /// No colors, art or animations
    #[arg(long, global = true)]
    plain: bool,
    /// Keep the art, skip the animations
    #[arg(long, global = true)]
    no_anim: bool,
}

#[derive(Args, Clone)]
struct ScanArgs {
    /// Extra folders to search for build artifacts (your home folder is always searched)
    #[arg(long = "projects", value_name = "DIR")]
    projects: Vec<PathBuf>,
    /// Pre-select build artifacts of projects idle for this many days
    #[arg(long, default_value_t = 30, value_name = "DAYS")]
    stale_days: u64,
    /// Downloads untouched for this many days count as old
    #[arg(long, default_value_t = 90, value_name = "DAYS")]
    download_age: u64,
    /// Apps not opened for this many days count as unused
    #[arg(long, default_value_t = 180, value_name = "DAYS")]
    app_age: u64,
    /// Files at least this big count as large
    #[arg(long, default_value = "500MB", value_parser = ui::size_arg, value_name = "SIZE")]
    large: u64,
}

impl Default for ScanArgs {
    fn default() -> Self {
        ScanArgs {
            projects: vec![],
            stale_days: 30,
            download_age: 90,
            app_age: 180,
            large: 500_000_000,
        }
    }
}

#[derive(Subcommand)]
enum Cmd {
    /// Analyze the disk and report what can be reclaimed (read-only)
    Scan {
        #[command(flatten)]
        args: ScanArgs,
        /// Only scan these categories (see `categories`)
        #[arg(long = "only", value_name = "ID")]
        only: Vec<String>,
        /// List the items inside every category
        #[arg(short, long)]
        verbose: bool,
        /// Machine-readable output
        #[arg(long)]
        json: bool,
    },
    /// Scan, choose targets, and fire the ray
    Clean {
        #[command(flatten)]
        args: ScanArgs,
        /// Clean only these categories (repeatable; see `categories`)
        #[arg(short = 'c', long = "category", value_name = "ID")]
        categories: Vec<String>,
        /// Select every SAFE category (caches, logs, Trash …)
        #[arg(long)]
        safe: bool,
        /// Don't ask for confirmation (pick-your-own categories use their defaults)
        #[arg(short, long)]
        yes: bool,
        /// Show exactly what would be removed, change nothing
        #[arg(long)]
        dry_run: bool,
    },
    /// Show what's taking up space under a folder, as a ranked tree
    Big {
        /// Folder to weigh (default: your home folder)
        path: Option<PathBuf>,
        /// How many entries to show per level
        #[arg(short = 'n', long, default_value_t = 15)]
        top: usize,
        /// How many levels deep to expand
        #[arg(short, long, default_value_t = 1)]
        depth: usize,
    },
    /// Find duplicate files (APFS clone-aware)
    Dupes {
        /// Folder to search (default: your home folder, minus ~/Library)
        path: Option<PathBuf>,
        /// Ignore files smaller than this
        #[arg(long, default_value = "1MB", value_parser = ui::size_arg, value_name = "SIZE")]
        min: u64,
        /// Pick spare copies to move to the Trash
        #[arg(long)]
        clean: bool,
        /// How many duplicate groups to print
        #[arg(long, default_value_t = 20, value_name = "N")]
        show: usize,
    },
    /// List every kind of clutter the machine knows about
    Categories,
    /// Every machine has one. Don't press it.
    #[command(hide = true)]
    SelfDestruct,
}

fn home_dir() -> Result<PathBuf> {
    // DECLUTTER_HOME lets tests aim the machine at a sandbox
    let home = std::env::var_os("DECLUTTER_HOME")
        .or_else(|| std::env::var_os("HOME"))
        .map(PathBuf::from)
        .context("can't figure out your home folder ($HOME is unset)")?;
    if !home.is_absolute() || !home.is_dir() {
        bail!("home folder {} doesn't look right", home.display());
    }
    // a trailing slash or symlinked home would confuse the safety guard
    Ok(std::fs::canonicalize(&home).unwrap_or(home))
}

fn ctx(args: &ScanArgs) -> Result<scan::Ctx> {
    let home = home_dir()?;
    let mut extra_roots = Vec::new();
    for p in &args.projects {
        let c = std::fs::canonicalize(p)
            .with_context(|| format!("can't open --projects {}", p.display()))?;
        if c == PathBuf::from("/") {
            bail!("--projects / would be a bit much, even for an evil genius");
        }
        extra_roots.push(c);
    }
    Ok(scan::Ctx {
        home,
        extra_roots,
        stale_days: args.stale_days,
        download_age_days: args.download_age,
        app_age_days: args.app_age,
        large_min: args.large,
        now: fsx::now(),
        sandboxed: std::env::var_os("DECLUTTER_HOME").is_some(),
    })
}

fn main() {
    let cli = Cli::parse();
    ui::init(cli.plain, cli.no_anim);
    let _ = ctrlc::set_handler(|| {
        art::show_cursor();
        eprintln!("\n\n  {}\n", ui::paint("Curses! Foiled again!", ui::RED));
        std::process::exit(130);
    });

    if let Err(e) = run(cli) {
        art::show_cursor();
        if let Some(dialoguer::Error::IO(io)) = e.downcast_ref::<dialoguer::Error>()
            && io.kind() == std::io::ErrorKind::Interrupted
        {
            eprintln!("\n  {}\n", ui::paint("Curses! Foiled again!", ui::RED));
            std::process::exit(130);
        }
        eprintln!("\n  {} {e:#}\n", ui::paint("✗", ui::RED));
        std::process::exit(1);
    }
}

fn run(cli: Cli) -> Result<()> {
    match cli.cmd {
        None => {
            let ctx = ctx(&ScanArgs::default())?;
            art::banner(true);
            let scan = scan::run(&ctx, &[], ui::fancy());
            report::print(&scan, &ctx, false);
            if scan.cats.is_empty() {
                return Ok(());
            }
            if clean::interactive() {
                println!();
                let go = dialoguer::Confirm::with_theme(&clean::theme())
                    .with_prompt("Choose targets and fire the DECLUTTER-INATOR?")
                    .default(true)
                    .interact_opt()?;
                if go == Some(true) {
                    let opts = clean::Opts {
                        dry_run: false,
                        yes: false,
                        safe: false,
                        ids: vec![],
                    };
                    clean::run(&ctx, &scan, &opts)?;
                } else {
                    println!(
                        "\n  {}\n",
                        ui::dim(
                            "Retreating to the lair. Run `declutterinator clean` when you're ready."
                        )
                    );
                }
            } else {
                println!(
                    "   {}\n",
                    ui::dim("Run `declutterinator clean` to fire the ray.")
                );
            }
        }
        Some(Cmd::Scan {
            args,
            only,
            verbose,
            json,
        }) => {
            for id in &only {
                if !catalog::exists(id) {
                    bail!("unknown category '{id}' — see `declutterinator categories`");
                }
            }
            let ctx = ctx(&args)?;
            if json {
                let scan = scan::run(&ctx, &only, false);
                println!("{}", report::json(&scan));
            } else {
                art::banner(false);
                let scan = scan::run(&ctx, &only, ui::fancy());
                report::print(&scan, &ctx, verbose);
                println!(
                    "   {}\n",
                    ui::dim(
                        "Run `declutterinator clean` to fire the ray (or `clean --dry-run` to preview)."
                    )
                );
            }
        }
        Some(Cmd::Clean {
            args,
            categories,
            safe,
            yes,
            dry_run,
        }) => {
            for id in &categories {
                if !catalog::exists(id) {
                    bail!("unknown category '{id}' — see `declutterinator categories`");
                }
            }
            let ctx = ctx(&args)?;
            if clean::interactive() && !yes {
                art::banner(true);
            } else {
                art::compact_header(if dry_run { "dry run" } else { "clean" });
            }
            let only: Vec<String> = if !categories.is_empty() {
                categories.clone()
            } else if safe {
                catalog::CATALOG
                    .iter()
                    .filter(|m| m.risk == Risk::Safe)
                    .map(|m| m.id.to_string())
                    .collect()
            } else {
                vec![]
            };
            let scan = scan::run(&ctx, &only, ui::fancy());
            report::disk_gauge(&scan);
            let opts = clean::Opts {
                dry_run,
                yes,
                safe,
                ids: categories,
            };
            clean::run(&ctx, &scan, &opts)?;
        }
        Some(Cmd::Big { path, top, depth }) => {
            let home = home_dir()?;
            art::compact_header("where did my disk go?");
            big::run(path, top.max(1), depth, &home)?;
        }
        Some(Cmd::Dupes {
            path,
            min,
            clean,
            show,
        }) => {
            let home = home_dir()?;
            art::compact_header("duplicate detector");
            let guard = fsx::Guard {
                home: home.clone(),
                roots: vec![],
            };
            dupes::run(path, min.max(1), clean, show, &home, &guard)?;
        }
        Some(Cmd::Categories) => categories(),
        Some(Cmd::SelfDestruct) => art::self_destruct(),
    }
    Ok(())
}

fn categories() {
    art::compact_header("everything the machine can zap");
    for risk in [Risk::Safe, Risk::Moderate, Risk::Review] {
        println!(
            "  {}  {}",
            ui::bold(ui::paint(format!("▌{}", risk.title()), risk.color())),
            ui::dim(risk.blurb())
        );
        for m in catalog::CATALOG.iter().filter(|m| m.risk == risk) {
            println!(
                "    {} {}  {}",
                ui::paint(ui::pad(m.id, 24), risk.color()),
                ui::pad(m.name, 32),
                ui::dim(m.action.verb())
            );
            for line in report::wrap(m.desc, ui::width().saturating_sub(34).max(30)) {
                println!("    {}{}", " ".repeat(25), ui::dim(line));
            }
        }
        println!();
    }
    println!(
        "  {}\n",
        ui::dim("Use the ids with `clean -c <id>` or `scan --only <id>`.")
    );
}
