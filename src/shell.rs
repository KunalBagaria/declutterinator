//! Running external tools (brew, docker, xcrun, mdls, tmutil) safely.

use std::io::{BufRead, BufReader, Read};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

const EXTRA_BIN_DIRS: &[&str] = &[
    "/opt/homebrew/bin",
    "/usr/local/bin",
    "/usr/bin",
    "/bin",
    "/usr/sbin",
];

/// Locate an executable on $PATH (plus the usual Homebrew locations).
pub fn which(prog: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH").unwrap_or_default();
    std::env::split_paths(&path)
        .chain(EXTRA_BIN_DIRS.iter().map(PathBuf::from))
        .map(|d| d.join(prog))
        .find(|p| is_executable(p))
}

fn is_executable(p: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    std::fs::metadata(p).is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
}

pub struct Output {
    pub ok: bool,
    pub stdout: String,
}

/// Run a command, capturing output; kill it if it exceeds `timeout`.
pub fn capture(program: &Path, args: &[&str], timeout: Duration) -> Option<Output> {
    let mut child = Command::new(program)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .env("HOMEBREW_NO_AUTO_UPDATE", "1")
        .env("HOMEBREW_NO_ENV_HINTS", "1")
        .spawn()
        .ok()?;
    let mut out = child.stdout.take()?;
    let mut err = child.stderr.take()?;
    let t_out = std::thread::spawn(move || {
        let mut s = Vec::new();
        let _ = out.read_to_end(&mut s);
        s
    });
    let t_err = std::thread::spawn(move || {
        let mut s = Vec::new();
        let _ = err.read_to_end(&mut s);
        s
    });
    let start = Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break Some(status),
            Ok(None) if start.elapsed() > timeout => {
                let _ = child.kill();
                let _ = child.wait();
                break None;
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(25)),
            Err(_) => break None,
        }
    };
    let stdout = String::from_utf8_lossy(&t_out.join().unwrap_or_default()).into_owned();
    let _ = t_err.join();
    let status = status?;
    Some(Output {
        ok: status.success(),
        stdout,
    })
}

/// Run a command interactively, echoing each output line through `echo`.
pub fn stream(program: &Path, args: &[&str], mut echo: impl FnMut(&str)) -> std::io::Result<bool> {
    let mut child = Command::new(program)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .env("HOMEBREW_NO_AUTO_UPDATE", "1")
        .env("HOMEBREW_NO_ENV_HINTS", "1")
        .spawn()?;
    let err = child.stderr.take().expect("piped stderr");
    let (tx, rx) = std::sync::mpsc::channel::<String>();
    let tx2 = tx.clone();
    let out = child.stdout.take().expect("piped stdout");
    let a = std::thread::spawn(move || {
        for line in BufReader::new(out).lines().map_while(Result::ok) {
            let _ = tx.send(line);
        }
    });
    let b = std::thread::spawn(move || {
        for line in BufReader::new(err).lines().map_while(Result::ok) {
            let _ = tx2.send(line);
        }
    });
    for line in rx {
        echo(&line);
    }
    let _ = a.join();
    let _ = b.join();
    Ok(child.wait()?.success())
}

// ---------------------------------------------------------------------------
// Spotlight metadata
// ---------------------------------------------------------------------------

/// Query one Spotlight date attribute (e.g. `kMDItemLastUsedDate`) for many
/// paths at once. Returns unix timestamps, `None` where unknown.
pub fn mdls_dates(paths: &[PathBuf], attr: &str) -> Vec<Option<i64>> {
    mdls_raw(paths, attr)
        .into_iter()
        .map(|v| v.and_then(|s| parse_date(&s)))
        .collect()
}

pub fn mdls_raw(paths: &[PathBuf], attr: &str) -> Vec<Option<String>> {
    let mut out = vec![None; paths.len()];
    let Some(mdls) = which("mdls") else {
        return out;
    };
    for (chunk_i, chunk) in paths.chunks(200).enumerate() {
        let mut args: Vec<&str> = vec!["-raw", "-name", attr];
        let strs: Vec<String> = chunk
            .iter()
            .map(|p| p.to_string_lossy().into_owned())
            .collect();
        args.extend(strs.iter().map(String::as_str));
        let Some(o) = capture(&mdls, &args, Duration::from_secs(20)) else {
            continue;
        };
        // values are NUL-separated; a vanished file silently drops out, so
        // only trust the batch when the count lines up exactly.
        let vals: Vec<&str> = o.stdout.split('\0').collect();
        if vals.len() != chunk.len() {
            continue;
        }
        for (i, v) in vals.into_iter().enumerate() {
            let v = v.trim();
            if !v.is_empty() && v != "(null)" {
                out[chunk_i * 200 + i] = Some(v.to_string());
            }
        }
    }
    out
}

/// Parse `2024-05-01 12:34:56 +0000` (mdls) or `2024-05-01T12:34:56Z` (plist).
pub fn parse_date(s: &str) -> Option<i64> {
    let s = s.trim();
    let b = s.as_bytes();
    if b.len() < 19 {
        return None;
    }
    let num = |r: std::ops::Range<usize>| s.get(r)?.parse::<i64>().ok();
    let (y, mo, d) = (num(0..4)?, num(5..7)?, num(8..10)?);
    let (h, mi, sec) = (num(11..13)?, num(14..16)?, num(17..19)?);
    let mut offset = 0;
    let rest = s[19..].trim();
    if rest.len() >= 5 && (rest.starts_with('+') || rest.starts_with('-')) {
        let sign = if rest.starts_with('-') { -1 } else { 1 };
        let oh: i64 = rest.get(1..3)?.parse().ok()?;
        let om: i64 = rest.get(3..5)?.parse().ok()?;
        offset = sign * (oh * 3600 + om * 60);
    }
    Some(days_from_civil(y, mo, d) * 86_400 + h * 3600 + mi * 60 + sec - offset)
}

/// Howard Hinnant's days-from-civil algorithm.
fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let mp = (m + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dates_parse() {
        assert_eq!(parse_date("1970-01-01 00:00:00 +0000"), Some(0));
        assert_eq!(parse_date("2024-05-01T12:34:56Z"), Some(1_714_566_896));
        assert_eq!(parse_date("2024-05-01 14:34:56 +0200"), Some(1_714_566_896));
        assert_eq!(parse_date("(null)"), None);
    }

    #[test]
    fn capture_times_out() {
        let sleep = which("sleep").unwrap();
        let t = Instant::now();
        assert!(capture(&sleep, &["5"], Duration::from_millis(200)).is_none());
        assert!(t.elapsed() < Duration::from_secs(2));
    }
}
