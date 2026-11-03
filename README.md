# DECLUTTER-INATOR

> *"BEHOLD! With this machine, I shall reclaim every wasted gigabyte in the entire TRI-STATE AREA!"*

**[declutter.kunalbagaria.com](https://declutter.kunalbagaria.com)**

A macOS disk analyzer and cleaner written in Rust. It sweeps your home folder for caches, build
artifacts, stale downloads and other clutter, tells you what's safe to remove, and then (only once
you approve) gets the space back. Comes with a ray gun.

```
                      _______________
               ______/  ___________  \______________________
      ___     |      | |  ( @ @ )  | |                      |\
     [___]====|  [o] | |___________| |   DECLUTTER-INATOR   | \_________
      | |     |  [o] |_______________|______________________|  _________|>━━━━━━━━━━━━✸
      | |     |  [o]  [=] [=] [=] [=]      (SELF-DESTRUCT)  | /
     _|_|_    |_____________________________________________|/
    |_____|    (O)     (O)     (O)     (O)     (O)     (O)
```

## Install

```sh
curl -fsSL https://declutter.kunalbagaria.com/install | sh
```

That downloads one universal binary (Apple silicon and Intel, macOS 11+), checks its SHA-256, puts it
in `~/.local/bin` and adds that folder to your `PATH` if it isn't there yet. No sudo. Run the same line
again to upgrade; delete `~/.local/bin/declutterinator` to uninstall. You can
[read the installer](install.sh) first, and it has settings for a different folder
(`DECLUTTER_INSTALL_DIR`), a pinned version (`DECLUTTER_VERSION`) or leaving your dotfiles alone
(`DECLUTTER_NO_MODIFY_PATH=1`).

Or build it from source with Rust 1.87 or newer:

```sh
cargo install --git https://github.com/KunalBagaria/declutterinator
```

## Use

```sh
declutterinator                       # the full show: scan → report → pick targets → fire
declutterinator scan -v               # read-only report, listing every item
declutterinator scan --json           # machine-readable
declutterinator clean --dry-run       # print every path that would be removed, touch nothing
declutterinator clean --safe --yes    # zap every zero-regret category, no questions (cron-friendly)
declutterinator clean -c npm-cache -c xcode-derived-data
declutterinator big ~/Library -d 2    # ranked tree of what's eating the disk
declutterinator dupes ~/Pictures      # duplicate files (APFS clone-aware); --clean to trash copies
declutterinator categories            # everything it knows about, with ids
```

Tuning: `--stale-days 30` (projects idle this long get their artifacts pre-selected),
`--download-age 90`, `--app-age 180`, `--large 500MB`, `--projects /Volumes/Dev` (extra roots).
`--plain` turns off color/art; `--no-anim` keeps the art but skips the theatrics.

## What it finds

| Risk | Meaning | Categories |
|---|---|---|
| **SAFE TO ZAP** (pre-selected) | regenerates on its own | app caches, logs, Trash, Xcode DerivedData, simulator caches, unavailable simulators, Homebrew leftovers, npm / Yarn / pnpm / Bun / pip / uv / Cargo / Gradle / Go / CocoaPods caches, Playwright/Puppeteer browsers, iOS firmware |
| **PROBABLY FINE** | safe, but costs a re-download or rebuild | Electron & Chromium web caches, Xcode device support, project build artifacts (`node_modules`, `target/`, `.venv`, `Pods`, `.next`, …), Docker images & build cache, Maven repo, Hugging Face models, installers (`.dmg`/`.pkg`), archives you already unzipped |
| **YOUR CALL** (never pre-selected) | your own files | old downloads, large files, apps you haven't opened in months, iPhone/iPad backups, Xcode archives |

Plus tips for things it explains but won't automate: Time Machine local snapshots, extra Rust / Node /
Python toolchains, Ollama / LM Studio models, Docker volumes, Full Disk Access.

## Safety

- **Nothing is removed without confirmation.** Non-interactive runs need `--yes` and an explicit
  `--safe` or `--category`. "Your call" categories can only be picked item by item, interactively.
- **Your own files go to the Trash** (recoverable); only regenerable caches and build output are
  deleted permanently. After trashing, it offers to empty the Trash so the space is actually freed.
- **A guard checks every path right before removal**: it must be inside your home folder (or a
  `--projects` root, or `/Applications/*.app`), can't be the home folder or a top-level folder in it,
  and can't be in an off-limits area (`.ssh`, Keychains, iCloud Drive, Mail, Messages, app containers…).
- **Deletion never follows symlinks and never crosses into another volume.** It keeps going past
  individual failures and reports them at the end.
- **Build artifacts are only recognized next to their manifest**: `node_modules` needs a
  `package.json` and `target/` needs a `Cargo.toml`/`pom.xml`, so a random `target/` folder is left
  alone. A project's "last activity" comes from git and its top-level files, not from the artifacts.
- `com.apple.*` caches are left to macOS.

## Honest numbers

- Sizes are **allocated** bytes (`st_blocks`), so sparse files and iCloud placeholders count as ~0,
  and paths covered by two categories are counted once.
- **APFS clones**: Bun, pnpm and uv clone packages from their cache into your projects, so deleting the
  cache frees only the blocks it doesn't share. Those caches are measured by *private* bytes
  (`getattrlist` `ATTR_CMNEXT_PRIVATESIZE`), and the shared part is shown separately. `dupes` uses the
  same trick to flag copies that are really clones and would free almost nothing.
- Units are decimal (1 GB = 10⁹ bytes), matching Finder.

## Notes

- Without **Full Disk Access** for your terminal, macOS hides the Trash, iOS backups and a few other
  folders. The report tells you how many folders it couldn't read.
- Interactive picker: ↑/↓ move, space toggles, `a` toggles all, enter fires, esc retreats.
- `DECLUTTER_HOME=/some/dir` points the machine at a stand-in home (used for testing). In that mode
  it skips machine-wide tools such as Homebrew, Docker and `/Applications`.
- Every machine has a self-destruct button. This one is `declutterinator self-destruct`. Don't press it.

## Development

```sh
cargo test                                   # unit tests (they need macOS: APFS, mdls, getattrlist)
cargo clippy --all-targets -- -D warnings
DECLUTTER_HOME=/tmp/fake-home cargo run      # aim the machine at a stand-in home folder
```

The website lives in [`site/`](site): static files in `site/public`, plus a small Cloudflare Worker
(`site/src/worker.js`) that serves `/install` as plain text and sets security and cache headers.

### Releasing

1. Bump `version` in `Cargo.toml` and commit.
2. `scripts/release.sh` builds both architectures, merges them with `lipo`, signs the result ad hoc,
   and stages `declutterinator-<version>-macos-universal.tar.gz` (+ `.sha256`) under
   `site/public/releases/`, pointing `latest.txt` at it.
3. `scripts/release.sh --publish` does the same, then deploys the site with `wrangler` and attaches the
   tarball to a GitHub release.

## License

[MIT](LICENSE). The website's fonts (IBM Plex Mono, Libre Caslon Text) are under the SIL Open Font License.
