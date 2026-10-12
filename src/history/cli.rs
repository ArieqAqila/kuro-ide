//! `kuro history list | show ID | delete ID… | clear`: managing the hard-undo history by hand. kuro never trims it by itself.

use super::{Disk, Line, Store};

pub const USAGE: &str = "usage:
    kuro history list                list the files kuro has a history for (and how much room it takes)
    kuro history show ID             one file's history: baseline, writes, outside changes, restores
    kuro history delete ID [ID…]     delete the history of those files (an ID may be shortened to a few characters)
    kuro history clear [--yes]       delete everything
Hard undo itself is the editor command :hardundo. See docs/HARD-UNDO.md.";

pub struct Outcome {
    pub code: i32,
    pub stdout: String,
    pub stderr: String,
}

fn ok(s: String) -> Outcome {
    Outcome { code: 0, stdout: s, stderr: String::new() }
}

fn fail(s: String) -> Outcome {
    Outcome { code: 1, stdout: String::new(), stderr: s }
}

/// `1700000000` → `2023-11-14 22:13` (UTC), without a date library.
pub fn format_time(secs: u64) -> String {
    let days = (secs / 86400) as i64;
    let rem = secs % 86400;
    // civil-from-days (Howard Hinnant's algorithm)
    let z = days + 719468;
    let era = z.div_euclid(146097);
    let doe = z.rem_euclid(146097);
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    format!("{y:04}-{m:02}-{d:02} {:02}:{:02}", rem / 3600, rem % 3600 / 60)
}

fn size_text(n: u64) -> String {
    if n >= 1 << 20 {
        format!("{:.1} MiB", n as f64 / 1048576.0)
    } else if n >= 1 << 10 {
        format!("{:.1} KiB", n as f64 / 1024.0)
    } else {
        format!("{n} B")
    }
}

/// `args` are the words after `kuro history`. `confirm_clear`: asked when `clear` has no `--yes` (None: refuse).
pub fn run(args: &[String], store: &Store, confirm_clear: Option<&dyn Fn(usize, u64) -> bool>) -> Outcome {
    match args.first().map(String::as_str) {
        Some("list") if args.len() == 1 => {
            let entries = store.entries();
            if entries.is_empty() {
                return ok("no history: kuro has not written any file yet\n".into());
            }
            let mut out = String::new();
            for e in &entries {
                let state = match (store.plan(Some(std::path::Path::new(&e.path))).is_empty(), e.writes + e.externals > 0) {
                    (true, _) => "original",
                    (false, _) => "changed",
                };
                out.push_str(&format!(
                    "{}  {:<8}  {} write{}, {} outside, {} restored  {}\n",
                    e.key,
                    state,
                    e.writes,
                    if e.writes == 1 { "" } else { "s" },
                    e.externals,
                    e.restores,
                    e.path
                ));
            }
            out.push_str(&format!(
                "{} file{}, {} on disk in {}\n",
                entries.len(),
                if entries.len() == 1 { "" } else { "s" },
                size_text(store.size_bytes()),
                store.root.display()
            ));
            ok(out)
        }
        Some("show") if args.len() == 2 => {
            let matches: Vec<_> = store.entries().into_iter().filter(|e| e.key.starts_with(&args[1])).collect();
            match matches.as_slice() {
                [] => fail(format!("no history `{}` (see kuro history list)\n", args[1])),
                [e] => {
                    let mut out = format!(
                        "{}\n  id {}\n  {}\n",
                        e.path,
                        e.key,
                        if e.existed {
                            format!("original: {} bytes, saved {}", e.baseline_size, format_time(e.started))
                        } else {
                            format!("did not exist before kuro created it ({})", format_time(e.started))
                        }
                    );
                    for l in &e.lines[1..] {
                        out.push_str(&match l {
                            Line::File { .. } => String::new(),
                            Line::Write { after, at, .. } => {
                                format!("  {}  kuro wrote it ({})\n", format_time(*at), &after[..12.min(after.len())])
                            }
                            Line::External { hash, at } => format!(
                                "  {}  changed OUTSIDE kuro ({})\n",
                                format_time(*at),
                                hash.as_deref().map_or("deleted".into(), |h| h[..12.min(h.len())].to_string())
                            ),
                            Line::Restored { after, at } => format!(
                                "  {}  hard undo restored {}\n",
                                format_time(*at),
                                if after.is_some() { "the original" } else { "(file deleted: it did not exist originally)" }
                            ),
                        });
                    }
                    out.push_str(&format!(
                        "  now: {}\n",
                        match &e.last_known {
                            Disk::Missing => "missing".to_string(),
                            Disk::Hash(h) => h[..12].to_string(),
                        }
                    ));
                    ok(out)
                }
                _ => fail(format!("`{}` matches several histories; give more characters\n", args[1])),
            }
        }
        Some("delete") if args.len() >= 2 => {
            let paths: std::collections::HashMap<String, String> = store.entries().into_iter().map(|e| (e.key, e.path)).collect();
            match store.delete(&args[1..]) {
                Ok(keys) => ok(keys.iter().map(|k| format!("deleted {k}  {}\n", paths.get(k).map(String::as_str).unwrap_or(""))).collect()),
                Err(e) => fail(format!("{e}\n")),
            }
        }
        Some("clear") if args.len() <= 2 && args.get(1).is_none_or(|a| a == "--yes") => {
            let n = store.entries().len();
            let yes = args.get(1).is_some();
            if n == 0 {
                return ok("no history to delete\n".into());
            }
            let confirmed = yes || confirm_clear.is_some_and(|c| c(n, store.size_bytes()));
            if !confirmed {
                return fail(format!(
                    "not deleted: this would delete the history of {n} file{}; run `kuro history clear --yes` to do it\n",
                    if n == 1 { "" } else { "s" }
                ));
            }
            match store.clear() {
                Ok(n) => ok(format!("deleted the history of {n} file{}\n", if n == 1 { "" } else { "s" })),
                Err(e) => fail(format!("{e}\n")),
            }
        }
        Some("-h") | Some("--help") | Some("help") => ok(format!("{USAGE}\n")),
        Some(other) => fail(format!("unknown history command or arguments `{other}`\n{USAGE}\n")),
        None => fail(format!("{USAGE}\n")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn times_are_utc_dates() {
        assert_eq!(format_time(0), "1970-01-01 00:00");
        assert_eq!(format_time(1_700_000_000), "2023-11-14 22:13");
        assert_eq!(format_time(951_782_400), "2000-02-29 00:00", "a leap day");
        assert_eq!(format_time(4_102_444_800), "2100-01-01 00:00");
    }
}
