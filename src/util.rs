//! Small helpers shared by every module: parallel maps, temporary names,
//! atomic writes, clocks, and display helpers.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::error::{IoContext, Result};

/// Runs `f` over `items` on at most `workers` threads, keeping input order.
pub fn parallel_map<T: Sync, R: Send>(items: &[T], workers: usize, f: impl Fn(&T) -> R + Sync) -> Vec<R> {
    if items.len() <= 1 || workers <= 1 {
        return items.iter().map(&f).collect();
    }
    let next = AtomicUsize::new(0);
    let results: Vec<std::sync::Mutex<Option<R>>> = items.iter().map(|_| std::sync::Mutex::new(None)).collect();
    std::thread::scope(|scope| {
        for _ in 0..workers.min(items.len()) {
            scope.spawn(|| {
                loop {
                    let i = next.fetch_add(1, Ordering::SeqCst);
                    if i >= items.len() {
                        break;
                    }
                    let r = f(&items[i]);
                    *results[i].lock().unwrap() = Some(r);
                }
            });
        }
    });
    results.into_iter().map(|m| m.into_inner().unwrap().unwrap()).collect()
}

/// A name unique to this process and call: `<prefix>-<pid>-<nanos>-<n>`.
pub fn unique_name(prefix: &str) -> String {
    static COUNTER: AtomicUsize = AtomicUsize::new(0);
    let nanos = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.subsec_nanos()).unwrap_or(0);
    format!("{prefix}-{}-{nanos}-{}", std::process::id(), COUNTER.fetch_add(1, Ordering::SeqCst))
}

/// Writes a file so readers see the old content or the new, never a mix.
pub fn write_atomic(path: &Path, contents: &[u8]) -> Result<()> {
    let dir = path.parent().unwrap_or(Path::new("."));
    std::fs::create_dir_all(dir).at("create", dir)?;
    let file_name = path.file_name().and_then(|n| n.to_str()).unwrap_or("file");
    let tmp = dir.join(unique_name(&format!(".tiger-tmp-{file_name}")));
    std::fs::write(&tmp, contents).at("write", &tmp)?;
    if let Err(err) = std::fs::rename(&tmp, path) {
        let _ = std::fs::remove_file(&tmp);
        return Err(crate::error::Error::io("write", path, err));
    }
    Ok(())
}

pub fn now_secs() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

pub fn short_sha(sha: &str) -> &str {
    &sha[..sha.len().min(7)]
}

/// FNV-1a, 64-bit: a stable digest for change detection (not security).
pub fn digest(data: &[u8]) -> String {
    let mut hash: u64 = 0xcbf29ce484222325;
    for byte in data {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    format!("{hash:016x}")
}

pub fn home_dir() -> Option<PathBuf> {
    std::env::var_os("HOME").filter(|h| !h.is_empty()).map(PathBuf::from)
}

/// `~/x` for paths under the home directory, for display only.
pub fn display_path(path: &Path) -> String {
    if let Some(home) = home_dir()
        && let Ok(rest) = path.strip_prefix(&home)
    {
        return if rest.as_os_str().is_empty() { "~".into() } else { format!("~/{}", rest.display()) };
    }
    path.display().to_string()
}

/// Seconds as `850ms`, `3.1s`, or `2m 05s`.
pub fn format_duration_ms(ms: u128) -> String {
    if ms < 1000 {
        format!("{ms}ms")
    } else if ms < 60_000 {
        format!("{:.1}s", ms as f64 / 1000.0)
    } else {
        format!("{}m {:02}s", ms / 60_000, (ms % 60_000) / 1000)
    }
}

/// Parses an interval such as `15m`, `6h`, `1d`, or `90s` into seconds.
pub fn parse_interval(text: &str) -> Option<u64> {
    let text = text.trim();
    let split = text.find(|c: char| !c.is_ascii_digit())?;
    let (number, unit) = text.split_at(split);
    let number: u64 = number.parse().ok()?;
    let factor = match unit {
        "s" => 1,
        "m" => 60,
        "h" => 3600,
        "d" => 86_400,
        _ => return None,
    };
    number.checked_mul(factor).filter(|s| *s > 0)
}

/// Levenshtein distance, for "did you mean" hints.
pub fn edit_distance(a: &str, b: &str) -> usize {
    let b: Vec<char> = b.chars().collect();
    let mut row: Vec<usize> = (0..=b.len()).collect();
    for (i, ca) in a.chars().enumerate() {
        let mut prev = row[0];
        row[0] = i + 1;
        for (j, cb) in b.iter().enumerate() {
            let cur = row[j + 1];
            row[j + 1] = if ca == *cb { prev } else { 1 + prev.min(row[j]).min(row[j + 1]) };
            prev = cur;
        }
    }
    row[b.len()]
}

pub fn closest<'a>(word: &str, candidates: &[&'a str]) -> Option<&'a str> {
    candidates.iter().copied().map(|c| (edit_distance(word, c), c)).filter(|(d, _)| *d <= 2).min().map(|(_, c)| c)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_intervals() {
        assert_eq!(parse_interval("15m"), Some(900));
        assert_eq!(parse_interval("6h"), Some(21_600));
        assert_eq!(parse_interval("1d"), Some(86_400));
        assert_eq!(parse_interval("0h"), None);
        assert_eq!(parse_interval("6"), None);
        assert_eq!(parse_interval("6w"), None);
    }

    #[test]
    fn suggests_close_words() {
        assert_eq!(closest("inlcude", &["include", "exclude", "url"]), Some("include"));
        assert_eq!(closest("zzz", &["include", "exclude"]), None);
    }

    #[test]
    fn parallel_map_keeps_order() {
        let items: Vec<u32> = (0..100).collect();
        assert_eq!(parallel_map(&items, 8, |x| x * 2), items.iter().map(|x| x * 2).collect::<Vec<_>>());
    }
}
