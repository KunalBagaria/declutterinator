//! Terminal styling, formatting helpers and output-mode switches.

use console::{Style, Term, measure_text_width};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};

static FANCY: AtomicBool = AtomicBool::new(true);
static ANIM: AtomicBool = AtomicBool::new(true);

/// Decide once, at startup, how flashy the output is allowed to be.
pub fn init(plain: bool, no_anim: bool) {
    let tty = Term::stdout().is_term();
    let set = |k: &str| std::env::var_os(k).is_some_and(|v| !v.is_empty() && v != "0");
    // https://bixense.com/clicolors/ and https://no-color.org/
    let fancy = (tty || set("CLICOLOR_FORCE"))
        && !plain
        && !set("NO_COLOR")
        && std::env::var("TERM").map_or(true, |t| t != "dumb");
    console::set_colors_enabled(fancy);
    console::set_colors_enabled_stderr(fancy);
    FANCY.store(fancy, Ordering::Relaxed);
    ANIM.store(fancy && tty && !no_anim && !set("CI"), Ordering::Relaxed);
}

/// Colors + art allowed.
pub fn fancy() -> bool {
    FANCY.load(Ordering::Relaxed)
}

/// Animations allowed (implies fancy).
pub fn anim() -> bool {
    ANIM.load(Ordering::Relaxed)
}

pub fn width() -> usize {
    Term::stdout()
        .size_checked()
        .map(|(_, w)| w as usize)
        .or_else(|| std::env::var("COLUMNS").ok()?.parse().ok())
        .unwrap_or(100)
        .clamp(40, 140)
}

// ---------- palette (xterm-256) ----------

pub const PINK: u8 = 199;
pub const PURPLE: u8 = 135;
pub const CYAN: u8 = 51;
pub const GREEN: u8 = 48;
pub const ORANGE: u8 = 214;
pub const RED: u8 = 203;
pub const YELLOW: u8 = 227;
pub const GREY: u8 = 244;
pub const DARK: u8 = 238;

/// Pink → purple → blue → cyan, used for banners and bars.
pub const GRADIENT: &[u8] = &[
    198, 199, 200, 201, 165, 171, 135, 141, 105, 111, 75, 81, 45, 51, 87,
];

pub fn c(color: u8) -> Style {
    Style::new().color256(color)
}

pub fn paint(s: impl AsRef<str>, color: u8) -> String {
    c(color).apply_to(s.as_ref()).to_string()
}

pub fn bold(s: impl AsRef<str>) -> String {
    Style::new().bold().apply_to(s.as_ref()).to_string()
}

pub fn dim(s: impl AsRef<str>) -> String {
    c(GREY).apply_to(s.as_ref()).to_string()
}

/// Color for position `i` of `n` along the gradient (clamped, never panics).
pub fn gradient_at(i: usize, n: usize) -> u8 {
    if n <= 1 {
        return GRADIENT[0];
    }
    GRADIENT[(i.min(n - 1) * (GRADIENT.len() - 1)) / (n - 1)]
}

/// Paint every visible character of `s` along the gradient.
pub fn rainbow(s: &str) -> String {
    let n = s.chars().count();
    s.chars()
        .enumerate()
        .map(|(i, ch)| paint(ch.to_string(), gradient_at(i, n)))
        .collect()
}

// ---------- sizes ----------

/// Human-readable size using decimal units, matching Finder ("12.3 GB").
pub fn size(n: u64) -> String {
    const UNITS: [&str; 6] = ["B", "KB", "MB", "GB", "TB", "PB"];
    if n < 1000 {
        return format!("{n} B");
    }
    let mut v = n as f64;
    let mut i = 0;
    while v >= 999.95 && i < UNITS.len() - 1 {
        v /= 1000.0;
        i += 1;
    }
    if v >= 100.0 {
        format!("{v:.0} {}", UNITS[i])
    } else {
        format!("{v:.1} {}", UNITS[i])
    }
}

/// Parse sizes such as `500MB`, `1.5G`, `2GiB`, `800 kB`, `4096`.
/// Plain units (`KB`, `M`, …) use `base` (1000 for most tools, 1024 for Homebrew).
pub fn parse_size_base(s: &str, base: f64) -> Option<u64> {
    let s = s.trim();
    let split = s
        .find(|ch: char| !(ch.is_ascii_digit() || ch == '.'))
        .unwrap_or(s.len());
    let (num, unit) = s.split_at(split);
    let num: f64 = num.parse().ok()?;
    let unit = unit.trim().to_ascii_lowercase();
    let (pow, binary) = match unit.as_str() {
        "" | "b" => (0, false),
        "k" | "kb" => (1, false),
        "kib" => (1, true),
        "m" | "mb" => (2, false),
        "mib" => (2, true),
        "g" | "gb" => (3, false),
        "gib" => (3, true),
        "t" | "tb" => (4, false),
        "tib" => (4, true),
        _ => return None,
    };
    let b = if binary { 1024.0 } else { base };
    Some((num * b.powi(pow)) as u64)
}

pub fn parse_size(s: &str) -> Option<u64> {
    parse_size_base(s, 1000.0)
}

/// clap value parser for size arguments.
pub fn size_arg(s: &str) -> Result<u64, String> {
    parse_size(s).ok_or_else(|| format!("can't understand size '{s}' (try 500MB or 2GB)"))
}

// ---------- bars, ages, paths ----------

/// A smooth bar made of eighth-blocks.
pub fn bar(frac: f64, width: usize) -> String {
    const PARTS: [char; 8] = [' ', '▏', '▎', '▍', '▌', '▋', '▊', '▉'];
    let frac = frac.clamp(0.0, 1.0);
    let eighths = (frac * width as f64 * 8.0).round() as usize;
    let mut s = "█".repeat(eighths / 8);
    if !eighths.is_multiple_of(8) {
        s.push(PARTS[eighths % 8]);
    }
    s
}

/// Gradient-colored bar padded to `width` cells.
pub fn gradient_bar(frac: f64, width: usize) -> String {
    let raw = bar(frac, width);
    let n = raw.chars().count();
    let mut out: String = raw
        .chars()
        .enumerate()
        .map(|(i, ch)| paint(ch.to_string(), gradient_at(i, width.max(n))))
        .collect();
    out.push_str(&" ".repeat(width.saturating_sub(n)));
    out
}

pub fn age(days: u64) -> String {
    match days {
        0 => "today".into(),
        1..=13 => format!("{days}d"),
        14..=59 => format!("{}wk", days / 7),
        60..=729 => format!("{}mo", days / 30),
        _ => format!("{}yr", days / 365),
    }
}

/// `~/…` display for paths under the home directory.
pub fn tilde(p: &Path, home: &Path) -> String {
    match p.strip_prefix(home) {
        Ok(rest) if rest.as_os_str().is_empty() => "~".into(),
        Ok(rest) => format!("~/{}", rest.display()),
        Err(_) => p.display().to_string(),
    }
}

/// Fit a string into `w` columns, eliding the *middle* (keeps file names visible).
pub fn fit(s: &str, w: usize) -> String {
    if measure_text_width(s) <= w {
        return s.to_string();
    }
    if w <= 1 {
        return "…".into();
    }
    let chars: Vec<char> = s.chars().collect();
    let keep = w - 1;
    let tail = (keep * 2) / 3;
    let head = keep - tail;
    let mut out: String = chars[..head.min(chars.len())].iter().collect();
    out.push('…');
    out.extend(&chars[chars.len().saturating_sub(tail)..]);
    // wide glyphs may still overflow; fall back to plain truncation
    if measure_text_width(&out) > w {
        return console::truncate_str(s, w, "…").into_owned();
    }
    out
}

/// Left-pad / right-pad by display width (ANSI-aware).
pub fn pad(s: &str, w: usize) -> String {
    let n = measure_text_width(s);
    format!("{s}{}", " ".repeat(w.saturating_sub(n)))
}

pub fn rpad(s: &str, w: usize) -> String {
    let n = measure_text_width(s);
    format!("{}{s}", " ".repeat(w.saturating_sub(n)))
}

/// `name ········ ` dotted leader up to width `w`.
pub fn leader(name: &str, w: usize) -> String {
    let n = measure_text_width(name);
    if n + 2 > w {
        return pad(&fit(name, w), w);
    }
    format!("{name} {}", dim("·".repeat(w - n - 1)))
}

pub fn plural(n: usize, one: &str, many: &str) -> String {
    if n == 1 {
        format!("{n} {one}")
    } else {
        format!("{n} {many}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sizes_format_like_finder() {
        assert_eq!(size(0), "0 B");
        assert_eq!(size(999), "999 B");
        assert_eq!(size(1_000), "1.0 KB");
        assert_eq!(size(12_345_678), "12.3 MB");
        assert_eq!(size(512_000_000), "512 MB");
        assert_eq!(size(999_990_000), "1.0 GB");
        assert_eq!(size(4_200_000_000_000), "4.2 TB");
    }

    #[test]
    fn sizes_parse() {
        assert_eq!(parse_size("500MB"), Some(500_000_000));
        assert_eq!(parse_size("1.5G"), Some(1_500_000_000));
        assert_eq!(parse_size("2GiB"), Some(2 * 1024 * 1024 * 1024));
        assert_eq!(parse_size("800 kB"), Some(800_000));
        assert_eq!(parse_size("4096"), Some(4096));
        assert_eq!(parse_size_base("1.5GB", 1024.0), Some(1_610_612_736));
        assert_eq!(parse_size("lots"), None);
        assert_eq!(parse_size("12 parsecs"), None);
    }

    #[test]
    fn fit_keeps_the_tail() {
        let s = "~/Documents/programming/some-very-long-project-name/node_modules";
        let f = fit(s, 30);
        assert_eq!(measure_text_width(&f), 30);
        assert!(f.ends_with("node_modules"));
        assert_eq!(fit("short", 30), "short");
    }

    #[test]
    fn gradient_never_overflows() {
        for n in 0..30 {
            for i in 0..40 {
                let _ = gradient_at(i, n);
            }
        }
    }

    #[test]
    fn ages() {
        assert_eq!(age(0), "today");
        assert_eq!(age(5), "5d");
        assert_eq!(age(21), "3wk");
        assert_eq!(age(95), "3mo");
        assert_eq!(age(800), "2yr");
    }
}
