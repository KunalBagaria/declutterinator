//! The sensors of the DECLUTTER-INATOR: one finder per kind of clutter.
//!
//! Finders only *read*. Each returns categories (with concrete items) and
//! optional tips for things we can explain but won't automate.

use crate::fsx::{self, Stats, is_real_dir};
use crate::model::{Category, Estimate, Item};
use crate::scan::Ctx;
use crate::shell;
use crate::ui;
use rayon::prelude::*;
use std::path::{Path, PathBuf};
use std::time::Duration;

#[derive(Default)]
pub struct Found {
    pub cats: Vec<Category>,
    pub tips: Vec<String>,
}

impl Found {
    fn one(c: Category) -> Found {
        Found {
            cats: vec![c],
            tips: vec![],
        }
    }
    fn tip(t: String) -> Found {
        Found {
            cats: vec![],
            tips: vec![t],
        }
    }
}

pub struct Finder {
    pub label: &'static str,
    /// categories this finder can produce (empty = tips only)
    pub ids: &'static [&'static str],
    pub run: fn(&Ctx) -> Found,
    /// acts on the whole machine, not just the home folder
    pub system_wide: bool,
}

pub const FINDERS: &[Finder] = &[
    Finder {
        label: "home sweep",
        ids: &["project-artifacts", "large-files"],
        run: crate::sweep::sweep,
        system_wide: false,
    },
    Finder {
        label: "homebrew",
        ids: &["homebrew"],
        run: homebrew,
        system_wide: true,
    },
    Finder {
        label: "docker",
        ids: &["docker"],
        run: docker,
        system_wide: true,
    },
    Finder {
        label: "simulators",
        ids: &["unavailable-simulators"],
        run: unavailable_simulators,
        system_wide: true,
    },
    Finder {
        label: "app caches",
        ids: &["app-caches"],
        run: app_caches,
        system_wide: false,
    },
    Finder {
        label: "web caches",
        ids: &["web-caches"],
        run: web_caches,
        system_wide: false,
    },
    Finder {
        label: "logs",
        ids: &["logs"],
        run: logs,
        system_wide: false,
    },
    Finder {
        label: "trash",
        ids: &["trash"],
        run: trash,
        system_wide: false,
    },
    Finder {
        label: "xcode",
        ids: &[
            "xcode-derived-data",
            "simulator-caches",
            "device-support",
            "xcode-archives",
        ],
        run: xcode,
        system_wide: false,
    },
    Finder {
        label: "package caches",
        ids: &[
            "npm-cache",
            "yarn-cache",
            "pip-cache",
            "cargo-cache",
            "gradle-cache",
            "go-cache",
            "cocoapods-cache",
            "browser-binaries",
            "maven-repo",
        ],
        run: package_caches,
        system_wide: false,
    },
    Finder {
        label: "clone-aware caches",
        ids: &["bun-cache", "uv-cache"],
        run: cloned_caches,
        system_wide: false,
    },
    Finder {
        label: "pnpm",
        ids: &["pnpm-store"],
        run: pnpm,
        system_wide: true,
    },
    Finder {
        label: "hugging face",
        ids: &["huggingface"],
        run: huggingface,
        system_wide: false,
    },
    Finder {
        label: "downloads",
        ids: &["installers", "unzipped-archives", "old-downloads"],
        run: downloads,
        system_wide: false,
    },
    Finder {
        label: "apps",
        ids: &["unused-apps"],
        run: unused_apps,
        system_wide: true,
    },
    Finder {
        label: "device backups",
        ids: &["ios-backups", "ios-firmware"],
        run: devices,
        system_wide: false,
    },
    Finder {
        label: "time machine",
        ids: &[],
        run: time_machine,
        system_wide: true,
    },
    Finder {
        label: "toolchains & models",
        ids: &[],
        run: toolchains,
        system_wide: false,
    },
];

// ---------------------------------------------------------------------------
// helpers
// ---------------------------------------------------------------------------

pub fn item_from(path: &Path, label: String, st: &Stats, now: i64) -> Item {
    Item {
        label,
        detail: None,
        paths: vec![path.to_path_buf()],
        bytes: st.bytes,
        age_days: Some(fsx::days_since(st.newest, now)),
        linked_bytes: st.linked,
        preselect: true,
    }
}

/// One item per child of `dir` whose name passes `keep`.
fn child_items(dir: &Path, keep: impl Fn(&str) -> bool + Sync, now: i64) -> (Vec<Item>, u64) {
    let kids = match fsx::children(dir) {
        Ok(k) => k,
        Err(e) => return (vec![], fsx::is_denied(&e) as u64),
    };
    let measured: Vec<(Item, u64)> = kids
        .par_iter()
        .filter(|p| keep(&fsx::file_name(p)))
        .map(|p| {
            let st = fsx::measure(p);
            (item_from(p, fsx::file_name(p), &st, now), st.denied)
        })
        .collect();
    let denied = measured.iter().map(|m| m.1).sum();
    (measured.into_iter().map(|m| m.0).collect(), denied)
}

/// One item per existing path.
fn path_items(paths: Vec<(String, PathBuf)>, now: i64) -> (Vec<Item>, u64) {
    let measured: Vec<(Item, u64)> = paths
        .into_par_iter()
        .filter(|(_, p)| fsx::exists(p))
        .map(|(label, p)| {
            let st = fsx::measure(&p);
            (item_from(&p, label, &st, now), st.denied)
        })
        .collect();
    let denied = measured.iter().map(|m| m.1).sum();
    (measured.into_iter().map(|m| m.0).collect(), denied)
}

fn paths_cat(id: &str, paths: Vec<(String, PathBuf)>, ctx: &Ctx) -> Category {
    let (items, denied) = path_items(paths, ctx.now);
    Category::from_items(id, items, denied)
}

fn env_dir(var: &str) -> Option<PathBuf> {
    std::env::var_os(var)
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
}

fn not_hidden(n: &str) -> bool {
    !n.starts_with('.')
}

// ---------------------------------------------------------------------------
// caches & logs
// ---------------------------------------------------------------------------

/// Cache folders that have their own dedicated category.
const CLAIMED_CACHES: &[&str] = &[
    "Homebrew",
    "pip",
    "Yarn",
    "CocoaPods",
    "go-build",
    "ms-playwright",
];

/// Caches owned by macOS daemons that don't use a `com.apple.` name — left to macOS.
const MACOS_CACHES: &[&str] = &[
    "CloudKit",
    "GeoServices",
    "SiriEntityCache",
    "PassKit",
    "Animoji",
    "GameKit",
    "AudioUnitCache",
    "askpermissiond",
    "icloudmailagent",
    "AMSDataMigratorTool",
    "mbuseragent",
    "tvapp_bag",
    "SharedImageCache",
    "libCachedImageData",
    "familycircle",
    "FamilyCircle",
    "storeassetd",
    "Cleanup At Startup",
    "JetPackCache",
    "FontRegistry",
    "SpeechSynthesis",
];

fn app_caches(ctx: &Ctx) -> Found {
    let dir = ctx.h("Library/Caches");
    let (items, denied) = child_items(
        &dir,
        |n| {
            not_hidden(n)
                && !n.starts_with("com.apple.")
                && !CLAIMED_CACHES.contains(&n)
                && !MACOS_CACHES.contains(&n)
                // loose files here are often live SQLite databases (+ -wal/-shm): only take folders
                && is_real_dir(&dir.join(n))
        },
        ctx.now,
    );
    Found::one(Category::from_items("app-caches", items, denied))
}

fn logs(ctx: &Ctx) -> Found {
    let (items, denied) = child_items(&ctx.h("Library/Logs"), not_hidden, ctx.now);
    Found::one(Category::from_items("logs", items, denied))
}

fn trash(ctx: &Ctx) -> Found {
    let dir = ctx.h(".Trash");
    let (items, denied) = child_items(&dir, |n| n != ".DS_Store" && n != ".localized", ctx.now);
    let mut c = Category::from_items("trash", items, denied);
    if denied > 0 && c.items.is_empty() {
        c = c.with_note("macOS hides the Trash without Full Disk Access");
    }
    Found::one(c)
}

const WEB_CACHE_DIRS: &[&str] = &[
    "Cache",
    "Code Cache",
    "GPUCache",
    "CachedData",
    "CachedExtensionVSIXs",
    "CachedProfilesData",
    "DawnCache",
    "DawnGraphiteCache",
    "DawnWebGPUCache",
    "GrShaderCache",
    "ShaderCache",
    "Service Worker/CacheStorage",
    "Service Worker/ScriptCache",
    "Crashpad/completed",
];

const CHROMIUM: &[(&str, &str)] = &[
    ("Google/Chrome", "Google Chrome"),
    ("Google/Chrome Beta", "Google Chrome Beta"),
    ("Google/Chrome Canary", "Google Chrome Canary"),
    ("BraveSoftware/Brave-Browser", "Brave"),
    ("BraveSoftware/Brave-Browser-Nightly", "Brave Nightly"),
    ("Microsoft Edge", "Microsoft Edge"),
    ("Vivaldi", "Vivaldi"),
    ("Chromium", "Chromium"),
    ("Arc/User Data", "Arc"),
];

/// Application Support folders that belong to macOS itself (and may be TCC-protected).
const APP_SUPPORT_SKIP: &[&str] = &[
    "AddressBook",
    "CallHistoryDB",
    "CallHistoryTransactions",
    "CloudDocs",
    "FaceTime",
    "FileProvider",
    "Knowledge",
    "MobileSync",
    "Dock",
    "iCloud",
    "SyncServices",
    "Animoji",
    "CrashReporter",
    "DiskImages",
    "icdd",
    "Google",
    "BraveSoftware",
    "Microsoft Edge",
    "Vivaldi",
    "Chromium",
    "Arc",
    "AppleMediaServices",
];

fn web_caches(ctx: &Ctx) -> Found {
    let base = ctx.h("Library/Application Support");
    let mut groups: Vec<(String, Vec<PathBuf>)> = Vec::new();

    for (rel, label) in CHROMIUM {
        let root = base.join(rel);
        if !is_real_dir(&root) {
            continue;
        }
        let mut paths: Vec<PathBuf> = [
            "GrShaderCache",
            "ShaderCache",
            "component_crx_cache",
            "Crashpad/completed",
        ]
        .iter()
        .map(|n| root.join(n))
        .collect();
        for profile in fsx::children(&root).unwrap_or_default() {
            let n = fsx::file_name(&profile);
            if n == "Default"
                || n.starts_with("Profile ")
                || n == "Guest Profile"
                || n == "System Profile"
            {
                paths.extend(WEB_CACHE_DIRS.iter().map(|d| profile.join(d)));
            }
        }
        paths.retain(|p| is_real_dir(p));
        if !paths.is_empty() {
            groups.push((label.to_string(), paths));
        }
    }

    for app in fsx::children(&base).unwrap_or_default() {
        let n = fsx::file_name(&app);
        if n.starts_with('.')
            || n.starts_with("com.apple.")
            || APP_SUPPORT_SKIP.contains(&n.as_str())
            || !is_real_dir(&app)
        {
            continue;
        }
        let paths: Vec<PathBuf> = WEB_CACHE_DIRS
            .iter()
            .map(|d| app.join(d))
            .filter(|p| is_real_dir(p))
            .collect();
        if !paths.is_empty() {
            let label = match n.as_str() {
                "Code" => "Visual Studio Code".to_string(),
                "discord" => "Discord".to_string(),
                _ => n,
            };
            groups.push((label, paths));
        }
    }

    let items: Vec<Item> = groups
        .into_par_iter()
        .map(|(label, paths)| {
            let st = fsx::measure_all(&paths);
            let mut kinds: Vec<String> = paths.iter().map(|p| fsx::file_name(p)).collect();
            kinds.sort();
            kinds.dedup();
            Item {
                label,
                detail: Some(kinds.join(", ")),
                bytes: st.bytes,
                age_days: Some(fsx::days_since(st.newest, ctx.now)),
                linked_bytes: 0,
                preselect: true,
                paths,
            }
        })
        .collect();
    Found::one(Category::from_items("web-caches", items, 0))
}

// ---------------------------------------------------------------------------
// developer tools
// ---------------------------------------------------------------------------

fn xcode(ctx: &Ctx) -> Found {
    let dev = ctx.h("Library/Developer");
    if !is_real_dir(&dev) {
        return Found::default();
    }
    let mut f = Found::default();

    let (mut items, denied) = child_items(&dev.join("Xcode/DerivedData"), not_hidden, ctx.now);
    for it in &mut items {
        // "MyApp-gxmxfwhbyhqbbtbefgcwuqnrzbag" → "MyApp"
        if let Some((name, hash)) = it.label.rsplit_once('-')
            && hash.len() >= 20
            && hash.chars().all(|c| c.is_ascii_lowercase())
        {
            it.label = name.to_string();
        }
    }
    f.cats
        .push(Category::from_items("xcode-derived-data", items, denied));

    let (items, denied) = child_items(&dev.join("CoreSimulator/Caches"), not_hidden, ctx.now);
    f.cats
        .push(Category::from_items("simulator-caches", items, denied));

    let mut support = Vec::new();
    let mut denied_total = 0;
    for (dir, platform) in [
        ("iOS DeviceSupport", "iOS"),
        ("watchOS DeviceSupport", "watchOS"),
        ("tvOS DeviceSupport", "tvOS"),
        ("visionOS DeviceSupport", "visionOS"),
        ("xrOS DeviceSupport", "visionOS"),
    ] {
        let (items, denied) = child_items(&dev.join("Xcode").join(dir), not_hidden, ctx.now);
        denied_total += denied;
        support.extend(items.into_iter().map(|mut it| {
            it.label = format!("{platform} {}", it.label);
            it.preselect = it.age_days.unwrap_or(0) >= 90;
            it
        }));
    }
    f.cats.push(Category::from_items(
        "device-support",
        support,
        denied_total,
    ));

    let mut archives = Vec::new();
    for day in fsx::children(&dev.join("Xcode/Archives")).unwrap_or_default() {
        let date = fsx::file_name(&day);
        for a in fsx::children(&day).unwrap_or_default() {
            if a.extension().is_some_and(|e| e == "xcarchive") {
                let st = fsx::measure(&a);
                let name = a
                    .file_stem()
                    .map(|s| s.to_string_lossy().into_owned())
                    .unwrap_or_default();
                let mut it = item_from(&a, name, &st, ctx.now);
                it.detail = Some(format!("archived {date}"));
                it.preselect = false;
                archives.push(it);
            }
        }
    }
    f.cats
        .push(Category::from_items("xcode-archives", archives, 0));
    f
}

fn unavailable_simulators(ctx: &Ctx) -> Found {
    let devices = ctx.h("Library/Developer/CoreSimulator/Devices");
    let Some(xcrun) = shell::which("xcrun") else {
        return Found::default();
    };
    if !is_real_dir(&devices) {
        return Found::default();
    }
    let Some(out) = shell::capture(
        &xcrun,
        &["simctl", "list", "devices", "unavailable", "-j"],
        Duration::from_secs(30),
    ) else {
        return Found::default();
    };
    if !out.ok {
        return Found::default();
    }
    let Ok(json) = serde_json::from_str::<serde_json::Value>(&out.stdout) else {
        return Found::default();
    };
    let mut paths = Vec::new();
    if let Some(map) = json.get("devices").and_then(|d| d.as_object()) {
        for (runtime, list) in map {
            let rt = runtime
                .rsplit('.')
                .next()
                .unwrap_or(runtime)
                .replace('-', " ");
            for d in list.as_array().into_iter().flatten() {
                let (Some(udid), Some(name)) = (
                    d.get("udid").and_then(|v| v.as_str()),
                    d.get("name").and_then(|v| v.as_str()),
                ) else {
                    continue;
                };
                paths.push((format!("{name} ({rt})"), devices.join(udid)));
            }
        }
    }
    Found::one(paths_cat("unavailable-simulators", paths, ctx))
}

fn homebrew(_ctx: &Ctx) -> Found {
    let Some(brew) = shell::which("brew") else {
        return Found::default();
    };
    let Some(out) = shell::capture(
        &brew,
        &["cleanup", "--prune=all", "--dry-run"],
        Duration::from_secs(120),
    ) else {
        return Found::default();
    };
    let bytes = parse_brew_estimate(&out.stdout).unwrap_or(0);
    let count = out
        .stdout
        .lines()
        .filter(|l| l.starts_with("Would remove"))
        .count();
    let mut c = Category::estimated("homebrew", bytes, Estimate::About);
    if count > 0 {
        c = c.with_note(format!("{} old downloads & versions", count));
    }
    Found::one(c)
}

/// `==> This operation would free approximately 1.2GB of disk space.`
pub fn parse_brew_estimate(out: &str) -> Option<u64> {
    let line = out
        .lines()
        .find(|l| l.contains("would free approximately"))?;
    let after = line.split("approximately").nth(1)?.trim();
    let token = after.split_whitespace().next()?;
    // Homebrew labels binary multiples as KB/MB/GB
    ui::parse_size_base(token, 1024.0)
}

fn docker(_ctx: &Ctx) -> Found {
    let Some(docker) = shell::which("docker") else {
        return Found::default();
    };
    let out = shell::capture(
        &docker,
        &["system", "df", "--format", "{{json .}}"],
        Duration::from_secs(8),
    );
    let Some(out) = out.filter(|o| o.ok) else {
        return Found::tip(
            "Docker is installed but not running — start it and re-scan to find unused images & build cache.".into(),
        );
    };
    let (prunable, volumes) = parse_docker_df(&out.stdout);
    let mut f = Found::default();
    if volumes > 1_000_000_000 {
        f.tips.push(format!(
            "{} sits in unused Docker volumes. Volumes hold data, so the machine leaves them alone — review with `docker volume ls`, prune with `docker volume prune`.",
            ui::size(volumes)
        ));
    }
    f.cats
        .push(Category::estimated("docker", prunable, Estimate::About));
    f
}

/// Sum reclaimable space from `docker system df --format '{{json .}}'`.
/// Returns (images + containers + build cache, volumes).
pub fn parse_docker_df(out: &str) -> (u64, u64) {
    let mut prunable = 0;
    let mut volumes = 0;
    for line in out.lines() {
        let Ok(v) = serde_json::from_str::<serde_json::Value>(line) else {
            continue;
        };
        let kind = v.get("Type").and_then(|t| t.as_str()).unwrap_or("");
        let rec = v
            .get("Reclaimable")
            .and_then(|t| t.as_str())
            .unwrap_or("0B");
        let bytes = rec
            .split_whitespace()
            .next()
            .and_then(ui::parse_size)
            .unwrap_or(0);
        if kind.contains("Volume") {
            volumes += bytes;
        } else {
            prunable += bytes;
        }
    }
    (prunable, volumes)
}

fn package_caches(ctx: &Ctx) -> Found {
    let h = |r: &str| ctx.h(r);
    let lbl = |s: &str| s.to_string();
    let cargo = env_dir("CARGO_HOME").unwrap_or_else(|| h(".cargo"));
    let gradle = env_dir("GRADLE_USER_HOME").unwrap_or_else(|| h(".gradle"));
    let gomod = env_dir("GOMODCACHE")
        .or_else(|| env_dir("GOPATH").map(|p| p.join("pkg/mod")))
        .unwrap_or_else(|| h("go/pkg/mod"));
    let gocache = env_dir("GOCACHE").unwrap_or_else(|| h("Library/Caches/go-build"));

    let specs: Vec<(&str, Vec<(String, PathBuf)>)> = vec![
        (
            "npm-cache",
            vec![
                (lbl("_cacache"), h(".npm/_cacache")),
                (lbl("_npx"), h(".npm/_npx")),
                (lbl("_logs"), h(".npm/_logs")),
            ],
        ),
        (
            "yarn-cache",
            vec![
                (lbl("Yarn classic"), h("Library/Caches/Yarn")),
                (lbl("Yarn berry"), h(".yarn/berry/cache")),
                (lbl(".cache/yarn"), h(".cache/yarn")),
            ],
        ),
        (
            "pip-cache",
            vec![
                (lbl("pip"), h("Library/Caches/pip")),
                (lbl(".cache/pip"), h(".cache/pip")),
            ],
        ),
        (
            "cargo-cache",
            vec![
                (lbl("registry/cache"), cargo.join("registry/cache")),
                (lbl("registry/src"), cargo.join("registry/src")),
                (lbl("git/checkouts"), cargo.join("git/checkouts")),
                (lbl("git/db"), cargo.join("git/db")),
            ],
        ),
        (
            "gradle-cache",
            vec![
                (lbl("caches"), gradle.join("caches")),
                (lbl("wrapper/dists"), gradle.join("wrapper/dists")),
                (lbl("daemon logs"), gradle.join("daemon")),
            ],
        ),
        (
            "go-cache",
            vec![(lbl("build cache"), gocache), (lbl("module cache"), gomod)],
        ),
        (
            "cocoapods-cache",
            vec![(lbl("CocoaPods"), h("Library/Caches/CocoaPods"))],
        ),
        (
            "browser-binaries",
            vec![
                (lbl("Playwright"), h("Library/Caches/ms-playwright")),
                (lbl("Puppeteer"), h(".cache/puppeteer")),
                (lbl("Selenium"), h(".cache/selenium")),
            ],
        ),
        ("maven-repo", vec![(lbl("repository"), h(".m2/repository"))]),
    ];
    Found {
        cats: specs
            .into_par_iter()
            .map(|(id, paths)| paths_cat(id, paths, ctx))
            .collect(),
        tips: vec![],
    }
}

/// Caches whose contents are APFS-cloned into projects: count only private bytes.
fn cloned_caches(ctx: &Ctx) -> Found {
    let specs = [
        ("bun-cache", "install cache", ctx.h(".bun/install/cache")),
        (
            "uv-cache",
            "uv",
            env_dir("UV_CACHE_DIR").unwrap_or_else(|| ctx.h(".cache/uv")),
        ),
    ];
    let cats = specs
        .into_par_iter()
        .map(|(id, label, path)| {
            if !fsx::exists(&path) {
                return Category::from_items(id, vec![], 0);
            }
            let st = fsx::measure_private(&path);
            let item = item_from(&path, label.to_string(), &st, ctx.now);
            let c = Category::from_items(id, vec![item], st.denied);
            if st.linked > 500_000_000 {
                c.with_note(format!(
                    "+{} more is shared with your projects (APFS clones) and isn't counted",
                    ui::size(st.linked)
                ))
            } else {
                c
            }
        })
        .collect();
    Found { cats, tips: vec![] }
}

fn pnpm(ctx: &Ctx) -> Found {
    if shell::which("pnpm").is_none() {
        return Found::default();
    }
    let candidates = [
        ctx.h("Library/pnpm/store"),
        ctx.h(".local/share/pnpm/store"),
        ctx.h(".pnpm-store"),
    ];
    let existing: Vec<PathBuf> = candidates.into_iter().filter(|p| is_real_dir(p)).collect();
    if existing.is_empty() {
        return Found::default();
    }
    let st = existing
        .par_iter()
        .map(|p| fsx::measure_private(p))
        .reduce(Stats::default, |a, b| Stats {
            bytes: a.bytes + b.bytes,
            linked: a.linked + b.linked,
            ..a
        });
    Found::one(
        Category::estimated("pnpm-store", st.bytes, Estimate::UpTo)
            .with_note("only packages no project references are removed"),
    )
}

fn huggingface(ctx: &Ctx) -> Found {
    let hub = env_dir("HF_HUB_CACHE")
        .or_else(|| env_dir("HF_HOME").map(|p| p.join("hub")))
        .unwrap_or_else(|| ctx.h(".cache/huggingface/hub"));
    let (mut items, denied) = child_items(
        &hub,
        |n| n.starts_with("models--") || n.starts_with("datasets--") || n.starts_with("spaces--"),
        ctx.now,
    );
    for it in &mut items {
        let mut parts = it.label.splitn(2, "--");
        let kind = parts.next().unwrap_or("").trim_end_matches('s').to_string();
        let name = parts.next().unwrap_or("").replacen("--", "/", 1);
        it.label = name;
        it.detail = Some(kind);
        it.preselect = false;
    }
    Found::one(Category::from_items("huggingface", items, denied))
}

// ---------------------------------------------------------------------------
// personal stuff (Review / Moderate)
// ---------------------------------------------------------------------------

const INSTALLER_EXT: &[&str] = &["dmg", "pkg", "mpkg", "iso", "xip", "ipsw"];

fn is_installer(p: &Path) -> bool {
    p.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| INSTALLER_EXT.contains(&e.to_ascii_lowercase().as_str()))
}

fn is_in_progress(name: &str) -> bool {
    name.ends_with(".download")
        || name.ends_with(".crdownload")
        || name.ends_with(".part")
        || name.ends_with(".opdownload")
}

/// "Last touched" for each path: newest of modified, added-to-folder and last-opened.
fn touched(paths: &[PathBuf], stats: &[Stats]) -> Vec<i64> {
    let used = shell::mdls_dates(paths, "kMDItemLastUsedDate");
    let added = shell::mdls_dates(paths, "kMDItemDateAdded");
    stats
        .iter()
        .enumerate()
        .map(|(i, st)| {
            st.newest
                .max(used[i].unwrap_or(0))
                .max(added[i].unwrap_or(0))
        })
        .collect()
}

const ARCHIVE_EXT: &[&str] = &[
    ".zip", ".tar.gz", ".tgz", ".tar.xz", ".tar.bz2", ".tar", ".7z", ".rar",
];

/// `foo.zip` next to a folder `foo/` → that folder.
fn extracted_twin(archive: &Path) -> Option<PathBuf> {
    let name = fsx::file_name(archive);
    let lower = name.to_ascii_lowercase();
    let ext = ARCHIVE_EXT.iter().find(|e| lower.ends_with(*e))?;
    let stem = &name[..name.len() - ext.len()];
    if stem.is_empty() || !fsx::lstat(archive)?.is_file() {
        return None;
    }
    let twin = archive.with_file_name(stem);
    is_real_dir(&twin).then_some(twin)
}

fn downloads(ctx: &Ctx) -> Found {
    let skip = |n: &str| n == ".DS_Store" || n == ".localized" || is_in_progress(n);
    let mut denied = 0;
    let mut list = |dir: PathBuf| -> Vec<PathBuf> {
        match fsx::children(&dir) {
            Ok(k) => k
                .into_iter()
                .filter(|p| !skip(&fsx::file_name(p)))
                .collect(),
            Err(e) => {
                denied += fsx::is_denied(&e) as u64;
                vec![]
            }
        }
    };
    let dl = list(ctx.h("Downloads"));
    let desk = list(ctx.h("Desktop"));

    let mut installers: Vec<PathBuf> = dl
        .iter()
        .chain(desk.iter())
        .filter(|p| is_installer(p))
        .cloned()
        .collect();
    installers.sort();
    let unzipped: Vec<(PathBuf, PathBuf)> = dl
        .iter()
        .chain(desk.iter())
        .filter_map(|p| extracted_twin(p).map(|dir| (p.clone(), dir)))
        .collect();
    let others: Vec<PathBuf> = dl.into_iter().filter(|p| !is_installer(p)).collect();

    let build = |paths: &[PathBuf]| -> Vec<Item> {
        let stats: Vec<Stats> = paths.par_iter().map(|p| fsx::measure(p)).collect();
        let times = touched(paths, &stats);
        paths
            .iter()
            .zip(stats.iter().zip(times))
            .map(|(p, (st, t))| {
                let mut it = item_from(p, ui::tilde(p, &ctx.home), st, ctx.now);
                it.age_days = Some(fsx::days_since(t, ctx.now));
                it
            })
            .collect()
    };

    let mut inst = build(&installers);
    for it in &mut inst {
        // fresh installers may not be installed yet
        it.preselect = it.age_days.unwrap_or(0) >= 7;
    }
    let archive_paths: Vec<PathBuf> = unzipped.iter().map(|(a, _)| a.clone()).collect();
    let mut archives = build(&archive_paths);
    for (it, (_, dir)) in archives.iter_mut().zip(&unzipped) {
        it.detail = Some(format!("unzipped → {}/", fsx::file_name(dir)));
    }
    let old: Vec<Item> = build(&others)
        .into_iter()
        .filter(|it| it.age_days.unwrap_or(0) >= ctx.download_age_days)
        .map(|mut it| {
            it.preselect = false;
            it
        })
        .collect();
    let mut f = Found::default();
    f.cats
        .push(Category::from_items("installers", inst, denied));
    f.cats
        .push(Category::from_items("unzipped-archives", archives, 0));
    f.cats.push(
        Category::from_items("old-downloads", old, 0)
            .with_note(format!("untouched for {}+ days", ctx.download_age_days)),
    );
    f
}

fn unused_apps(ctx: &Ctx) -> Found {
    let mut apps: Vec<PathBuf> = Vec::new();
    for dir in [PathBuf::from("/Applications"), ctx.h("Applications")] {
        apps.extend(
            fsx::children(&dir)
                .unwrap_or_default()
                .into_iter()
                .filter(|p| p.extension().is_some_and(|e| e == "app") && is_real_dir(p)),
        );
    }
    if apps.is_empty() {
        return Found::default();
    }
    // Spotlight sometimes reports nonsense like 1980 — ignore anything before 2001
    let plausible = |t: Option<i64>| t.filter(|&t| t > 978_307_200 && t <= ctx.now + 86_400);
    let used: Vec<Option<i64>> = shell::mdls_dates(&apps, "kMDItemLastUsedDate")
        .into_iter()
        .map(plausible)
        .collect();
    let added: Vec<Option<i64>> = shell::mdls_dates(&apps, "kMDItemDateAdded")
        .into_iter()
        .map(plausible)
        .collect();
    let ids = shell::mdls_raw(&apps, "kMDItemCFBundleIdentifier");

    let candidates: Vec<(PathBuf, Option<i64>, i64)> = apps
        .into_par_iter()
        .enumerate()
        .filter_map(|(i, p)| {
            let id = ids[i].as_deref();
            if id.is_some_and(|id| {
                id == "com.apple.Safari" || id.starts_with("com.apple.systempreferences")
            }) {
                return None;
            }
            // traces the app leaves in ~/Library whenever it runs
            let activity = app_activity(ctx, &p, id);
            let last_used = used[i].max(Some(activity).filter(|&a| a > 0));
            // without any data at all we can't judge — leave it alone
            let t = last_used.or(added[i])?;
            (fsx::days_since(t, ctx.now) >= ctx.app_age_days).then_some((p, last_used, t))
        })
        .collect();

    let items: Vec<Item> = candidates
        .into_par_iter()
        .map(|(p, used, t)| {
            let st = fsx::measure(&p);
            let name = p
                .file_stem()
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_default();
            let mut it = item_from(&p, name, &st, ctx.now);
            let days = fsx::days_since(t, ctx.now);
            it.age_days = Some(days);
            it.detail = Some(match used {
                Some(_) => format!("last used {} ago", ui::age(days)),
                None => format!("never opened (installed {} ago)", ui::age(days)),
            });
            it.preselect = false;
            it
        })
        .collect();
    Found::one(
        Category::from_items("unused-apps", items, 0)
            .with_note(format!("not opened in {}+ days", ctx.app_age_days)),
    )
}

/// Newest modification among the files an app touches in ~/Library while it runs.
fn app_activity(ctx: &Ctx, app: &Path, bundle_id: Option<&str>) -> i64 {
    let mut newest = 0;
    let mut look = |p: PathBuf, peek_inside: bool| {
        let Some(md) = fsx::lstat(&p) else { return };
        newest = newest.max(std::os::unix::fs::MetadataExt::mtime(&md));
        if peek_inside && md.is_dir() {
            for c in fsx::children(&p).unwrap_or_default() {
                if let Some(cm) = fsx::lstat(&c) {
                    newest = newest.max(std::os::unix::fs::MetadataExt::mtime(&cm));
                }
            }
        }
    };
    if let Some(id) = bundle_id {
        look(
            ctx.h("Library/Preferences").join(format!("{id}.plist")),
            false,
        );
        look(
            ctx.h("Library/Saved Application State")
                .join(format!("{id}.savedState")),
            true,
        );
        look(ctx.h("Library/Caches").join(id), true);
        look(ctx.h("Library/HTTPStorages").join(id), true);
    }
    if let Some(name) = app.file_stem() {
        look(ctx.h("Library/Application Support").join(name), true);
    }
    newest
}

/// Pull `<key>K</key><string>V</string>` (or `<date>`) out of an XML plist.
pub fn plist_value(xml: &str, key: &str) -> Option<String> {
    let k = format!("<key>{key}</key>");
    let rest = &xml[xml.find(&k)? + k.len()..];
    let rest = rest.trim_start();
    let open = rest.find('>')? + 1;
    let tag = rest.get(1..open - 1)?;
    if tag != "string" && tag != "date" {
        return None;
    }
    let close = rest.find(&format!("</{tag}>"))?;
    Some(rest[open..close].trim().to_string())
}

fn devices(ctx: &Ctx) -> Found {
    let mut f = Found::default();
    let backup_dir = ctx.h("Library/Application Support/MobileSync/Backup");
    let (mut items, denied) = child_items(&backup_dir, not_hidden, ctx.now);
    for it in &mut items {
        let info = std::fs::read_to_string(it.paths[0].join("Info.plist")).unwrap_or_default();
        if let Some(name) = plist_value(&info, "Device Name") {
            it.label = name;
        }
        let when = plist_value(&info, "Last Backup Date").and_then(|d| shell::parse_date(&d));
        if let Some(t) = when {
            it.age_days = Some(fsx::days_since(t, ctx.now));
        }
        it.detail = Some(format!(
            "backed up {} ago",
            it.age_days.map(ui::age).unwrap_or_else(|| "?".into())
        ));
        it.preselect = false;
    }
    let mut c = Category::from_items("ios-backups", items, denied);
    if denied > 0
        && c.items.is_empty()
        && is_real_dir(&ctx.h("Library/Application Support/MobileSync"))
    {
        c = c.with_note("needs Full Disk Access to look inside");
    }
    f.cats.push(c);

    let mut fw = Vec::new();
    for dir in [
        "iPhone Software Updates",
        "iPad Software Updates",
        "iPod Software Updates",
    ] {
        let (items, _) = child_items(
            &ctx.h("Library/iTunes").join(dir),
            |n| n.ends_with(".ipsw"),
            ctx.now,
        );
        fw.extend(items);
    }
    f.cats.push(Category::from_items("ios-firmware", fw, 0));
    f
}

// ---------------------------------------------------------------------------
// tips only
// ---------------------------------------------------------------------------

fn time_machine(_ctx: &Ctx) -> Found {
    let Some(tm) = shell::which("tmutil") else {
        return Found::default();
    };
    let Some(out) = shell::capture(&tm, &["listlocalsnapshots", "/"], Duration::from_secs(10))
    else {
        return Found::default();
    };
    let n = out
        .stdout
        .lines()
        .filter(|l| l.contains("com.apple.TimeMachine."))
        .count();
    if n == 0 {
        return Found::default();
    }
    Found::tip(format!(
        "{} local Time Machine snapshot{} on this disk. macOS purges them automatically when space runs low; to drop them now run `sudo tmutil deletelocalsnapshots /`.",
        n,
        if n == 1 { "" } else { "s" }
    ))
}

fn toolchains(ctx: &Ctx) -> Found {
    struct Spec {
        dir: &'static str,
        what: &'static str,
        hint: &'static str,
        min_versions: usize,
    }
    let specs = [
        Spec {
            dir: ".rustup/toolchains",
            what: "Rust toolchains",
            hint: "`rustup toolchain list`, then `rustup toolchain uninstall <name>`",
            min_versions: 2,
        },
        Spec {
            dir: ".nvm/versions/node",
            what: "Node versions (nvm)",
            hint: "`nvm ls`, then `nvm uninstall <version>`",
            min_versions: 2,
        },
        Spec {
            dir: ".pyenv/versions",
            what: "Python versions (pyenv)",
            hint: "`pyenv versions`, then `pyenv uninstall <version>`",
            min_versions: 2,
        },
        Spec {
            dir: "Library/Android/sdk/system-images",
            what: "Android emulator images",
            hint: "Android Studio → SDK Manager",
            min_versions: 1,
        },
        Spec {
            dir: ".ollama/models",
            what: "Ollama models",
            hint: "`ollama list`, then `ollama rm <model>`",
            min_versions: 1,
        },
        Spec {
            dir: ".lmstudio/models",
            what: "LM Studio models",
            hint: "LM Studio → My Models",
            min_versions: 1,
        },
        Spec {
            dir: ".cache/lm-studio/models",
            what: "LM Studio models",
            hint: "LM Studio → My Models",
            min_versions: 1,
        },
    ];
    let tips: Vec<String> = specs
        .par_iter()
        .filter_map(|s| {
            let dir = ctx.h(s.dir);
            let kids = fsx::children(&dir).ok()?;
            let n = kids
                .iter()
                .filter(|p| !fsx::file_name(p).starts_with('.'))
                .count();
            if n < s.min_versions {
                return None;
            }
            let st = fsx::measure(&dir);
            (st.bytes >= 2_000_000_000).then(|| {
                let count = if s.min_versions > 1 {
                    format!("{n} ")
                } else {
                    String::new()
                };
                format!(
                    "{count}{} use {} ({}). Prune with {}.",
                    s.what,
                    ui::size(st.bytes),
                    ui::tilde(&dir, &ctx.home),
                    s.hint
                )
            })
        })
        .collect();
    Found { cats: vec![], tips }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn brew_estimate() {
        let out = "Would remove: /opt/homebrew/Cellar/node/21.0 (2,000 files, 80MB)\n==> This operation would free approximately 1.5GB of disk space.\n";
        assert_eq!(parse_brew_estimate(out), Some(1_610_612_736));
        assert_eq!(parse_brew_estimate("nothing to do"), None);
    }

    #[test]
    fn docker_df() {
        let out = r#"{"Active":"2","Reclaimable":"1.2GB (45%)","Size":"2.7GB","TotalCount":"5","Type":"Images"}
{"Active":"0","Reclaimable":"0B","Size":"0B","TotalCount":"0","Type":"Containers"}
{"Active":"1","Reclaimable":"3GB (60%)","Size":"5GB","TotalCount":"3","Type":"Local Volumes"}
{"Active":"0","Reclaimable":"512.5MB","Size":"512.5MB","TotalCount":"40","Type":"Build Cache"}"#;
        assert_eq!(parse_docker_df(out), (1_712_500_000, 3_000_000_000));
    }

    #[test]
    fn plist() {
        let xml = "<dict>\n\t<key>Device Name</key>\n\t<string>Villain's iPhone</string>\n\t<key>Last Backup Date</key>\n\t<date>2024-05-01T12:34:56Z</date>\n</dict>";
        assert_eq!(
            plist_value(xml, "Device Name").as_deref(),
            Some("Villain's iPhone")
        );
        assert_eq!(
            plist_value(xml, "Last Backup Date").as_deref(),
            Some("2024-05-01T12:34:56Z")
        );
        assert_eq!(plist_value(xml, "Serial"), None);
    }
}
