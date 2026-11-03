//! ASCII art, the big block font, and the ray-gun theatrics.

use crate::ui::{self, DARK, GREY, PURPLE, RED, YELLOW};
use std::io::{Write, stdout};
use std::thread::sleep;
use std::time::Duration;

// ---------------------------------------------------------------------------
// tiny RNG (no need for a crate to pick a quip)
// ---------------------------------------------------------------------------

fn rand() -> u64 {
    use std::cell::Cell;
    thread_local! {
        static S: Cell<u64> = Cell::new(
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos() as u64)
                .unwrap_or(0x9E37_79B9_7F4A_7C15)
                | 1,
        );
    }
    S.with(|s| {
        let mut x = s.get();
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        s.set(x);
        x
    })
}

pub fn pick<'a>(options: &[&'a str]) -> &'a str {
    options[(rand() % options.len() as u64) as usize]
}

// ---------------------------------------------------------------------------
// block font
// ---------------------------------------------------------------------------

fn glyph(c: char) -> Option<[&'static str; 6]> {
    Some(match c {
        'A' => [
            " █████╗ ",
            "██╔══██╗",
            "███████║",
            "██╔══██║",
            "██║  ██║",
            "╚═╝  ╚═╝",
        ],
        'B' => [
            "██████╗ ",
            "██╔══██╗",
            "██████╔╝",
            "██╔══██╗",
            "██████╔╝",
            "╚═════╝ ",
        ],
        'C' => [
            " ██████╗",
            "██╔════╝",
            "██║     ",
            "██║     ",
            "╚██████╗",
            " ╚═════╝",
        ],
        'D' => [
            "██████╗ ",
            "██╔══██╗",
            "██║  ██║",
            "██║  ██║",
            "██████╔╝",
            "╚═════╝ ",
        ],
        'E' => [
            "███████╗",
            "██╔════╝",
            "█████╗  ",
            "██╔══╝  ",
            "███████╗",
            "╚══════╝",
        ],
        'G' => [
            " ██████╗ ",
            "██╔════╝ ",
            "██║  ███╗",
            "██║   ██║",
            "╚██████╔╝",
            " ╚═════╝ ",
        ],
        'I' => ["██╗", "██║", "██║", "██║", "██║", "╚═╝"],
        'K' => [
            "██╗  ██╗",
            "██║ ██╔╝",
            "█████╔╝ ",
            "██╔═██╗ ",
            "██║  ██╗",
            "╚═╝  ╚═╝",
        ],
        'L' => [
            "██╗     ",
            "██║     ",
            "██║     ",
            "██║     ",
            "███████╗",
            "╚══════╝",
        ],
        'M' => [
            "███╗   ███╗",
            "████╗ ████║",
            "██╔████╔██║",
            "██║╚██╔╝██║",
            "██║ ╚═╝ ██║",
            "╚═╝     ╚═╝",
        ],
        'N' => [
            "███╗   ██╗",
            "████╗  ██║",
            "██╔██╗ ██║",
            "██║╚██╗██║",
            "██║ ╚████║",
            "╚═╝  ╚═══╝",
        ],
        'O' => [
            " ██████╗ ",
            "██╔═══██╗",
            "██║   ██║",
            "██║   ██║",
            "╚██████╔╝",
            " ╚═════╝ ",
        ],
        'P' => [
            "██████╗ ",
            "██╔══██╗",
            "██████╔╝",
            "██╔═══╝ ",
            "██║     ",
            "╚═╝     ",
        ],
        'R' => [
            "██████╗ ",
            "██╔══██╗",
            "██████╔╝",
            "██╔══██╗",
            "██║  ██║",
            "╚═╝  ╚═╝",
        ],
        'T' => [
            "████████╗",
            "╚══██╔══╝",
            "   ██║   ",
            "   ██║   ",
            "   ██║   ",
            "   ╚═╝   ",
        ],
        'U' => [
            "██╗   ██╗",
            "██║   ██║",
            "██║   ██║",
            "██║   ██║",
            "╚██████╔╝",
            " ╚═════╝ ",
        ],
        'Z' => [
            "███████╗",
            "╚══███╔╝",
            "  ███╔╝ ",
            " ███╔╝  ",
            "███████╗",
            "╚══════╝",
        ],
        '!' => ["██╗", "██║", "██║", "╚═╝", "██╗", "╚═╝"],
        '-' => ["      ", "      ", "█████╗", "╚════╝", "      ", "      "],
        '.' => ["   ", "   ", "   ", "   ", "██╗", "╚═╝"],
        ' ' => ["   ", "   ", "   ", "   ", "   ", "   "],
        '0' => [
            " ██████╗ ",
            "██╔═████╗",
            "██║██╔██║",
            "████╔╝██║",
            "╚██████╔╝",
            " ╚═════╝ ",
        ],
        '1' => [" ██╗", "███║", "╚██║", " ██║", " ██║", " ╚═╝"],
        '2' => [
            "██████╗ ",
            "╚════██╗",
            " █████╔╝",
            "██╔═══╝ ",
            "███████╗",
            "╚══════╝",
        ],
        '3' => [
            "██████╗ ",
            "╚════██╗",
            " █████╔╝",
            " ╚═══██╗",
            "██████╔╝",
            "╚═════╝ ",
        ],
        '4' => [
            "██╗  ██╗",
            "██║  ██║",
            "███████║",
            "╚════██║",
            "     ██║",
            "     ╚═╝",
        ],
        '5' => [
            "███████╗",
            "██╔════╝",
            "███████╗",
            "╚════██║",
            "███████║",
            "╚══════╝",
        ],
        '6' => [
            " ██████╗ ",
            "██╔════╝ ",
            "███████╗ ",
            "██╔═══██╗",
            "╚██████╔╝",
            " ╚═════╝ ",
        ],
        '7' => [
            "███████╗",
            "╚════██║",
            "    ██╔╝",
            "   ██╔╝ ",
            "   ██║  ",
            "   ╚═╝  ",
        ],
        '8' => [
            " █████╗ ",
            "██╔══██╗",
            "╚█████╔╝",
            "██╔══██╗",
            "╚█████╔╝",
            " ╚════╝ ",
        ],
        '9' => [
            " █████╗ ",
            "██╔══██╗",
            "╚██████║",
            " ╚═══██║",
            " █████╔╝",
            " ╚════╝ ",
        ],
        _ => return None,
    })
}

/// Render `text` in the block font (unknown characters are skipped).
pub fn big(text: &str) -> Vec<String> {
    let mut rows = vec![String::new(); 6];
    for ch in text.to_ascii_uppercase().chars() {
        if let Some(g) = glyph(ch) {
            for (row, part) in rows.iter_mut().zip(g) {
                row.push_str(part);
            }
        }
    }
    rows
}

pub fn big_width(text: &str) -> usize {
    big(text)[0].chars().count()
}

/// Darker partner for each gradient stop, used for the font's "shadow" strokes.
const SHADOW: &[u8] = &[
    125, 126, 127, 128, 91, 97, 97, 98, 61, 67, 32, 31, 31, 37, 37,
];

/// Color block-font rows: solid blocks ride the gradient, outlines get its shadow.
pub fn colorize_big(rows: &[String], span: usize, offset: usize) -> Vec<String> {
    if !ui::fancy() {
        return rows.to_vec();
    }
    rows.iter()
        .map(|row| {
            row.chars()
                .enumerate()
                .map(|(i, ch)| {
                    let stop = ((i + offset) * (ui::GRADIENT.len() - 1) / span.max(2))
                        .min(ui::GRADIENT.len() - 1);
                    match ch {
                        ' ' => " ".to_string(),
                        '█' => ui::paint("█", ui::GRADIENT[stop]),
                        other => ui::paint(other.to_string(), SHADOW[stop]),
                    }
                })
                .collect()
        })
        .collect()
}

// ---------------------------------------------------------------------------
// the machine
// ---------------------------------------------------------------------------

const MACHINE: &[&str] = &[
    r"                      _______________",
    r"               ______/  ___________  \______________________",
    r"      ___     |      | |  ( @ @ )  | |                      |\",
    r"     [___]====|  [o] | |___________| |   DECLUTTER-INATOR   | \_________",
    r"      | |     |  [o] |_______________|______________________|  _________|>",
    r"      | |     |  [o]  [=] [=] [=] [=]      (SELF-DESTRUCT)  | /",
    r"     _|_|_    |_____________________________________________|/",
    r"    |_____|    (O)     (O)     (O)     (O)     (O)     (O)",
];
/// Row of MACHINE that holds the barrel's muzzle.
const MUZZLE_ROW: usize = 4;
const INDENT: &str = "  ";

fn color_machine_line(line: &str) -> String {
    if !ui::fancy() {
        return line.to_string();
    }
    const SPECIAL: &[(&str, u8)] = &[
        ("DECLUTTER-INATOR", 0),
        ("(SELF-DESTRUCT)", RED),
        ("( @ @ )", YELLOW),
        ("[o]", ui::GREEN),
        ("[=]", ui::CYAN),
        ("(O)", GREY),
        ("|>", ui::CYAN),
    ];
    let mut out = String::new();
    let mut rest = line;
    'outer: while !rest.is_empty() {
        for (pat, color) in SPECIAL {
            if let Some(tail) = rest.strip_prefix(pat) {
                out.push_str(&if *color == 0 {
                    ui::rainbow(pat)
                } else {
                    ui::paint(pat, *color)
                });
                rest = tail;
                continue 'outer;
            }
        }
        let ch = rest.chars().next().unwrap();
        out.push_str(&ui::paint(ch.to_string(), PURPLE));
        rest = &rest[ch.len_utf8()..];
    }
    out
}

pub fn machine_width() -> usize {
    MACHINE.iter().map(|l| l.chars().count()).max().unwrap_or(0) + INDENT.len()
}

fn flush() {
    let _ = stdout().flush();
}

fn pause(ms: u64) {
    if ui::anim() {
        flush();
        sleep(Duration::from_millis(ms));
    }
}

pub fn hide_cursor() {
    if ui::anim() {
        print!("\x1b[?25l");
        flush();
    }
}

pub fn show_cursor() {
    if console::Term::stdout().is_term() {
        print!("\x1b[?25h");
        flush();
    }
}

// ---------------------------------------------------------------------------
// banners
// ---------------------------------------------------------------------------

const TAGLINES: &[&str] = &[
    "Reclaiming every wasted gigabyte in the entire TRI-STATE AREA!",
    "Because your SSD deserves better than 40 GB of node_modules.",
    "Fully armed. Fully operational. Self-destruct button: present, as always.",
    "Now with 100% fewer platypus-related setbacks. Probably.",
];

/// The big entrance. `machine` adds the contraption and fires its ray.
pub fn banner(machine: bool) {
    let w = ui::width();
    if !ui::fancy() {
        println!(
            "DECLUTTER-INATOR v{} — macOS disk analyzer & cleaner\n",
            env!("CARGO_PKG_VERSION")
        );
        return;
    }
    hide_cursor();
    let top = big("DECLUTTER");
    let span = big_width("DECLUTTER");
    if w < span + 4 {
        compact_header("");
    } else {
        println!();
        let bottom = big("-INATOR");
        let off = " ".repeat((span - big_width("-INATOR")) / 2);
        for row in colorize_big(&top, span, 0) {
            println!("{INDENT}{row}");
            pause(18);
        }
        // the second word lines up with the gradient of the first
        for row in colorize_big(&bottom, span, off.len()) {
            println!("{INDENT}{off}{row}");
            pause(18);
        }
        let tag = pick(TAGLINES);
        let pad = " ".repeat((span.saturating_sub(tag.chars().count())) / 2);
        println!();
        print!("{INDENT}{pad}");
        typewriter(tag, YELLOW, 6);
        println!();
    }
    if machine && w >= machine_width() + 2 {
        println!();
        for line in MACHINE {
            println!("{INDENT}{}", color_machine_line(line));
            pause(25);
        }
        fire_beam(w);
    }
    show_cursor();
    println!();
}

pub fn compact_header(sub: &str) {
    if !ui::fancy() {
        println!("DECLUTTER-INATOR {sub}\n");
        return;
    }
    let title = ui::rainbow("▓▒░ DECLUTTER-INATOR ░▒▓");
    if sub.is_empty() {
        println!("\n  {title}\n");
    } else {
        println!("\n  {title}  {}\n", ui::dim(sub));
    }
}

fn typewriter(text: &str, color: u8, ms: u64) {
    if !ui::anim() {
        print!("{}", ui::paint(text, color));
        return;
    }
    for ch in text.chars() {
        print!("{}", ui::paint(ch.to_string(), color));
        pause(ms);
    }
}

/// Animate the ray leaving the barrel of the machine just printed above.
fn fire_beam(term_w: usize) {
    let start_col = INDENT.len() + MACHINE[MUZZLE_ROW].chars().count() + 1;
    let room = term_w.saturating_sub(start_col + 1);
    if room < 6 {
        return;
    }
    let up = MACHINE.len() - MUZZLE_ROW;
    let beam_len = room.min(40);
    // move up to the muzzle row, right after the barrel
    print!("\x1b[{up}A\x1b[{start_col}G");
    if ui::anim() {
        for i in 0..beam_len {
            let ch = if i + 1 == beam_len {
                '✸'
            } else if i % 3 == 2 {
                '═'
            } else {
                '━'
            };
            print!(
                "{}",
                ui::paint(ch.to_string(), ui::gradient_at(i, beam_len))
            );
            pause(9);
        }
        // flicker
        for _ in 0..2 {
            print!(
                "\x1b[{start_col}G{}",
                ui::paint("━".repeat(beam_len - 1), 231)
            );
            pause(45);
            print!(
                "\x1b[{start_col}G{}",
                ui::rainbow(&"━".repeat(beam_len - 1))
            );
            pause(45);
        }
    } else {
        print!("{}✸", ui::rainbow(&"━".repeat(beam_len - 1)));
    }
    print!("\x1b[{up}B\r");
    flush();
}

// ---------------------------------------------------------------------------
// cleaning theatrics
// ---------------------------------------------------------------------------

pub fn charge() {
    if !ui::anim() {
        return;
    }
    hide_cursor();
    let w = 30;
    for i in 0..=w {
        let pct = i * 100 / w;
        print!(
            "\r  {} {} {}",
            ui::paint("⚡ CHARGING", YELLOW),
            ui::gradient_bar(i as f64 / w as f64, w),
            ui::dim(format!("{pct:>3}%"))
        );
        pause(14);
    }
    print!("\r\x1b[2K");
    show_cursor();
}

const DUST: &[char] = &['▓', '▒', '░', '·', ' '];

/// Print `prefix text`, disintegrate the text, then leave `indent after` on the line.
pub fn disintegrate(indent: &str, text: &str, after: &str) {
    if !ui::anim() {
        println!("{indent}{after}");
        return;
    }
    let prefix = format!("{indent}{} ", ui::paint("✸", ui::PINK));
    let chars: Vec<char> = text.chars().collect();
    let mut stage = vec![0usize; chars.len()];
    for frame in 0..9 {
        let s: String = chars
            .iter()
            .zip(stage.iter_mut())
            .map(|(&ch, st)| {
                if frame > 0 && rand().is_multiple_of(3) {
                    *st = (*st + 1).min(DUST.len());
                }
                match *st {
                    0 => ui::paint(ch.to_string(), RED),
                    n => ui::paint(DUST[n - 1].to_string(), ui::gradient_at(n, DUST.len())),
                }
            })
            .collect();
        print!("\r\x1b[2K{prefix}{s}");
        pause(32);
    }
    println!("\r\x1b[2K{indent}{after}");
}

pub fn finale(freed: u64, trashed: u64) {
    let size = ui::size(freed);
    println!();
    if ui::fancy() && ui::width() >= big_width(&size) + 6 && freed > 0 {
        let rows = big(&size);
        let span = big_width(&size);
        for row in colorize_big(&rows, span, 0) {
            println!("  {row}");
            pause(30);
        }
        println!();
    }
    if freed > 0 {
        println!(
            "  {} {}",
            ui::paint("MWAHAHAHA!", ui::PINK),
            ui::bold(format!(
                "{} vaporized. The Tri-State Area grows tidier by the minute!",
                ui::size(freed)
            ))
        );
    }
    if trashed > 0 {
        println!(
            "  {} {} moved to the Trash — it only frees space once you empty it.",
            ui::paint("🗑", GREY),
            ui::bold(ui::size(trashed))
        );
    }
    if freed == 0 && trashed == 0 {
        println!("  {}", ui::paint("Curses! Nothing was zapped.", RED));
    }
}

pub const CANCEL_LINES: &[&str] = &[
    "Curses! Foiled again!",
    "Fine. Keep your clutter. See if I care. (I care a little.)",
    "Retreating to the lair. This isn't over!",
];

pub const NOTHING_LINES: &[&str] = &[
    "Nothing to zap?! What kind of evil genius keeps a tidy disk?",
    "Spotless. Suspiciously spotless.",
];

pub fn self_destruct() {
    compact_header("self-destruct sequence");
    for n in ["3", "2", "1"] {
        let rows = big(n);
        for row in colorize_big(&rows, 9, 0) {
            println!("    {row}");
        }
        pause(600);
        if ui::anim() {
            print!("\x1b[6A\x1b[J");
        }
    }
    println!(
        "  {}\n  {}\n",
        ui::paint("…just kidding. Your files are fine.", YELLOW),
        ui::dim("Why do I keep putting one of these buttons on everything?")
    );
}

pub fn divider() {
    println!("  {}", ui::paint("━".repeat(ui::width().min(78) - 4), DARK));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn glyph_rows_are_rectangular() {
        for ch in "ABCDEGIKLMNOPRTUZ!-. 0123456789".chars() {
            let g = glyph(ch).unwrap();
            let w = g[0].chars().count();
            assert!(
                g.iter().all(|r| r.chars().count() == w),
                "glyph {ch:?} is ragged"
            );
        }
    }

    #[test]
    fn banner_fits_80_columns() {
        assert!(big_width("DECLUTTER") + INDENT.len() <= 80);
        assert!(machine_width() <= 80);
    }
}
