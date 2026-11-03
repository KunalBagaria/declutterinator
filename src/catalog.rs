//! Every kind of clutter the DECLUTTER-INATOR knows how to find.

use crate::model::{Action, Meta, Risk};

use Action::{Delete, Trash};
use Risk::{Moderate, Review, Safe};

pub const CATALOG: &[Meta] = &[
    // ---------------------------------------------------------------- SAFE
    Meta {
        id: "app-caches",
        name: "Application caches",
        risk: Safe,
        action: Delete,
        pick_items: false,
        desc: "~/Library/Caches — apps rebuild these on demand. Quit big apps first for best results.",
        quip: "Apps hoard caches 'just in case'. I, too, once hoarded. Then I built a machine.",
    },
    Meta {
        id: "logs",
        name: "Logs & crash reports",
        risk: Safe,
        action: Delete,
        pick_items: false,
        desc: "~/Library/Logs — diagnostic logs and crash reports.",
        quip: "Thousands of diary entries nobody will ever read. Like my blog.",
    },
    Meta {
        id: "trash",
        name: "Trash",
        risk: Safe,
        action: Delete,
        pick_items: false,
        desc: "~/.Trash — you already threw these away. Emptying it is permanent.",
        quip: "You threw it out but never took it out. Classic.",
    },
    Meta {
        id: "xcode-derived-data",
        name: "Xcode DerivedData",
        risk: Safe,
        action: Delete,
        pick_items: false,
        desc: "Build intermediates and indexes. Xcode regenerates them (first build will be slower).",
        quip: "Xcode remembers every build you've ever made. Xcode needs to learn to let go.",
    },
    Meta {
        id: "simulator-caches",
        name: "Simulator caches",
        risk: Safe,
        action: Delete,
        pick_items: false,
        desc: "~/Library/Developer/CoreSimulator/Caches — dyld and runtime caches.",
        quip: "Caches for pretend iPhones. Even my evil schemes are more real than this.",
    },
    Meta {
        id: "unavailable-simulators",
        name: "Unavailable simulators",
        risk: Safe,
        action: Action::Command {
            program: "xcrun",
            args: &["simctl", "delete", "unavailable"],
        },
        pick_items: false,
        desc: "Simulators for runtimes that are no longer installed — they can't even boot.",
        quip: "Zombie iPhones for iOS versions that no longer exist. Put them out of their misery.",
    },
    Meta {
        id: "homebrew",
        name: "Homebrew leftovers",
        risk: Safe,
        action: Action::Command {
            program: "brew",
            args: &["cleanup", "--prune=all"],
        },
        pick_items: false,
        desc: "Old formula versions and cached downloads, removed by Homebrew itself.",
        quip: "Old bottles from the Homebrew cellar. Vintage — but not the good kind.",
    },
    Meta {
        id: "npm-cache",
        name: "npm cache",
        risk: Safe,
        action: Delete,
        pick_items: false,
        desc: "~/.npm/_cacache, _npx and _logs — re-downloaded on the next install.",
        quip: "Every package you ever installed, lovingly preserved. Even left-pad.",
    },
    Meta {
        id: "yarn-cache",
        name: "Yarn cache",
        risk: Safe,
        action: Delete,
        pick_items: false,
        desc: "Yarn's global package cache.",
        quip: "A ball of yarn this big is a fire hazard.",
    },
    Meta {
        id: "pnpm-store",
        name: "pnpm store (unreferenced)",
        risk: Safe,
        action: Action::Command {
            program: "pnpm",
            args: &["store", "prune"],
        },
        pick_items: false,
        desc: "pnpm removes packages no project references any more.",
        quip: "Packages nobody depends on. I know the feeling.",
    },
    Meta {
        id: "bun-cache",
        name: "Bun cache",
        risk: Safe,
        action: Delete,
        pick_items: false,
        desc: "~/.bun/install/cache — re-downloaded on demand.",
        quip: "Stale buns. Into the bin.",
    },
    Meta {
        id: "pip-cache",
        name: "pip cache",
        risk: Safe,
        action: Delete,
        pick_items: false,
        desc: "Downloaded wheels and HTTP cache for pip.",
        quip: "Wheels within wheels within wheels.",
    },
    Meta {
        id: "uv-cache",
        name: "uv cache",
        risk: Safe,
        action: Delete,
        pick_items: false,
        desc: "~/.cache/uv — uv re-fetches what it needs.",
        quip: "Fast to download, fast to forget.",
    },
    Meta {
        id: "cargo-cache",
        name: "Cargo registry & git cache",
        risk: Safe,
        action: Delete,
        pick_items: false,
        desc: "Downloaded crates and git checkouts in ~/.cargo — fetched again when needed.",
        quip: "Crates of crates. A warehouse of crates. Somebody call a forklift.",
    },
    Meta {
        id: "gradle-cache",
        name: "Gradle caches",
        risk: Safe,
        action: Delete,
        pick_items: false,
        desc: "~/.gradle/caches and wrapper distributions.",
        quip: "Gradle downloads the internet once per project. Let's give some back.",
    },
    Meta {
        id: "go-cache",
        name: "Go build & module cache",
        risk: Safe,
        action: Delete,
        pick_items: false,
        desc: "Go's build cache and downloaded modules.",
        quip: "Go... away.",
    },
    Meta {
        id: "cocoapods-cache",
        name: "CocoaPods cache",
        risk: Safe,
        action: Delete,
        pick_items: false,
        desc: "~/Library/Caches/CocoaPods — pod downloads.",
        quip: "Pods. Pods everywhere. Like a sci-fi movie, but less exciting.",
    },
    Meta {
        id: "browser-binaries",
        name: "Headless browser downloads",
        risk: Safe,
        action: Delete,
        pick_items: false,
        desc: "Browsers downloaded by Playwright / Puppeteer for testing — fetched again on next run.",
        quip: "Whole web browsers nobody ever looks at. Very ghostly. Very wasteful.",
    },
    Meta {
        id: "ios-firmware",
        name: "iOS firmware downloads",
        risk: Safe,
        action: Delete,
        pick_items: false,
        desc: "*.ipsw restore images left behind by Finder device updates.",
        quip: "Firmware for phones that already updated. Mission accomplished, soldier.",
    },
    // ------------------------------------------------------------ MODERATE
    Meta {
        id: "web-caches",
        name: "App & browser web caches",
        risk: Moderate,
        action: Delete,
        pick_items: true,
        desc: "Code/GPU/Service-Worker caches of Electron apps and Chromium browsers. Quit the apps first.",
        quip: "Every Electron app ships a whole browser, and every browser keeps a whole cache.",
    },
    Meta {
        id: "device-support",
        name: "Xcode device support files",
        risk: Moderate,
        action: Delete,
        pick_items: true,
        desc: "Debug symbols copied from each device/OS version — re-copied when that device connects.",
        quip: "Symbols for an iPhone running iOS from a bygone era. Let it go.",
    },
    Meta {
        id: "project-artifacts",
        name: "Project build artifacts",
        risk: Moderate,
        action: Delete,
        pick_items: true,
        desc: "node_modules, Rust target/, .venv, Pods, .next… — rebuilt by your package manager.",
        quip: "node_modules: the heaviest objects in the known universe.",
    },
    Meta {
        id: "docker",
        name: "Docker images & build cache",
        risk: Moderate,
        action: Action::Command {
            program: "docker",
            args: &["system", "prune", "--all", "--force"],
        },
        pick_items: false,
        desc: "Stopped containers, unused networks, unused images and build cache. Volumes are kept.",
        quip: "Images of images of images. It's images all the way down.",
    },
    Meta {
        id: "maven-repo",
        name: "Maven repository",
        risk: Moderate,
        action: Delete,
        pick_items: false,
        desc: "~/.m2/repository — re-downloaded on the next build (locally installed artifacts too!).",
        quip: "A Maven is an expert. An expert at hoarding JARs.",
    },
    Meta {
        id: "huggingface",
        name: "Hugging Face models & datasets",
        risk: Moderate,
        action: Delete,
        pick_items: true,
        desc: "~/.cache/huggingface/hub — re-downloaded the next time a script asks for them.",
        quip: "Gigabytes of weights. My machine prefers its weights free.",
    },
    Meta {
        id: "installers",
        name: "Installers & disk images",
        risk: Moderate,
        action: Trash,
        pick_items: true,
        desc: ".dmg / .pkg / .iso / .xip files in Downloads and Desktop — the app is usually installed already.",
        quip: "Installers for apps you installed ages ago. They've served their purpose.",
    },
    Meta {
        id: "unzipped-archives",
        name: "Archives you already unzipped",
        risk: Moderate,
        action: Trash,
        pick_items: true,
        desc: ".zip / .tar / .7z files in Downloads or Desktop sitting right next to the folder they were extracted into.",
        quip: "You opened the present. You can throw away the wrapping paper.",
    },
    // -------------------------------------------------------------- REVIEW
    Meta {
        id: "old-downloads",
        name: "Old downloads",
        risk: Review,
        action: Trash,
        pick_items: true,
        desc: "Things in ~/Downloads nobody has opened or touched in a long time. Never pre-selected (moved to Trash).",
        quip: "If you haven't opened it in months, did you ever really download it?",
    },
    Meta {
        id: "large-files",
        name: "Large files",
        risk: Review,
        action: Trash,
        pick_items: true,
        desc: "Big files in your home folder. Never pre-selected — you decide (moved to Trash).",
        quip: "Big ones. I won't touch these without your say-so. Even villains have standards.",
    },
    Meta {
        id: "unused-apps",
        name: "Apps you don't use",
        risk: Review,
        action: Trash,
        pick_items: true,
        desc: "Applications not opened in 6+ months, according to Spotlight (moved to Trash).",
        quip: "You installed it, opened it once, and forgot about it. Rude.",
    },
    Meta {
        id: "ios-backups",
        name: "iPhone & iPad backups",
        risk: Review,
        action: Trash,
        pick_items: true,
        desc: "Local device backups. Keep at least one for any device you still own!",
        quip: "Backups of phones you may not even own anymore.",
    },
    Meta {
        id: "xcode-archives",
        name: "Xcode archives",
        risk: Review,
        action: Trash,
        pick_items: true,
        desc: "Archived builds. They contain dSYMs you need to symbolicate crash reports.",
        quip: "Archives of apps past. Keep the ones whose crashes you still care about.",
    },
];

pub fn meta(id: &str) -> &'static Meta {
    CATALOG
        .iter()
        .find(|m| m.id == id)
        .unwrap_or_else(|| panic!("unknown category id {id}"))
}

pub fn exists(id: &str) -> bool {
    CATALOG.iter().any(|m| m.id == id)
}

/// Position in the catalog, used for stable ordering.
pub fn rank(id: &str) -> usize {
    CATALOG
        .iter()
        .position(|m| m.id == id)
        .unwrap_or(usize::MAX)
}
