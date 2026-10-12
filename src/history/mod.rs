//! Hard undo: the history of every file kuro writes, kept on disk so a file can be taken back to **what it was before kuro first edited it**,
//! even after every kuro terminal is closed. See docs/HARD-UNDO.md and docs/design/hard-undo.md.
//!
//! One folder per file under the history root:
//!
//! ```text
//! <root>/files/<key>/log      lines `{json}<TAB><crc32 of the json>`; appended, never rewritten
//! <root>/blobs/<sha256>       the baseline contents, written once (content addressed)
//! <root>/lock                 flock while recording
//! ```
//!
//! `<key>` is the first 16 hex digits of the SHA-256 of the file's real path. The log holds: the **baseline** (the file as it was before kuro's first
//! write, or "did not exist"), one line per write kuro made, one per change noticed that was made *outside* kuro, and one per restore. Nothing is ever
//! deleted by kuro itself: only `kuro history delete` / `clear` remove anything.
//!
//! Crash safety: the baseline blob is written (temp file, fsync, rename) *before* the line that names it, which is appended and fsynced *before* kuro's
//! first write replaces the file. A reader ignores a last line that is torn or whose checksum fails, so a crash loses at most the line being written.

use crate::ext::seal::{hex, sha256};
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};

pub mod cli;
pub mod observed;

/// `[history]` settings in the form the store uses.
#[derive(Debug, Clone)]
pub struct HistoryOptions {
    pub enabled: bool,
    pub max_file_bytes: u64,
    /// Globs matched against the file name: files that match are never recorded.
    pub exclude: Vec<String>,
    pub warn_bytes: u64,
}

impl Default for HistoryOptions {
    fn default() -> Self {
        HistoryOptions {
            enabled: true,
            max_file_bytes: 5 << 20,
            exclude: DEFAULT_EXCLUDE.iter().map(|s| s.to_string()).collect(),
            warn_bytes: 200 << 20,
        }
    }
}

pub const DEFAULT_EXCLUDE: &[&str] = &["*.env", ".env*", "*.pem", "*.key", "id_*"];

/// Why a file is not recorded.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Skip {
    Disabled,
    Excluded,
    TooLarge(u64),
    /// The history itself could not be written.
    Error(String),
}

impl std::fmt::Display for Skip {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Skip::Disabled => write!(f, "history.enabled is off"),
            Skip::Excluded => write!(f, "the file matches history.exclude"),
            Skip::TooLarge(n) => write!(f, "the file is {:.1} MiB (history.max_file_mib)", *n as f64 / 1048576.0),
            Skip::Error(e) => write!(f, "{e}"),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "t", rename_all = "snake_case")]
pub enum Line {
    /// The baseline: the file before kuro's first write.
    File { path: String, existed: bool, pre: Option<String>, size: u64, at: u64 },
    /// A write kuro made (`before` is the hash of what it replaced, `after` of what it wrote).
    Write { before: Option<String>, after: String, at: u64 },
    /// A change made outside kuro was noticed (`hash` is `None` when the file had been deleted).
    External { hash: Option<String>, at: u64 },
    /// A hard undo (`after` is `None` when the file was deleted because it did not exist originally).
    Restored { after: Option<String>, at: u64 },
}

/// What a file looks like on disk, as far as the history is concerned.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Disk {
    Missing,
    Hash(String),
}

impl Disk {
    pub fn of(bytes: Option<&[u8]>) -> Disk {
        match bytes {
            Some(b) => Disk::Hash(hex(&sha256(b))),
            None => Disk::Missing,
        }
    }
}

/// The history of one file, folded from its log.
#[derive(Debug, Clone)]
pub struct Entry {
    pub key: String,
    pub path: String,
    pub existed: bool,
    pub baseline: Option<String>,
    pub baseline_size: u64,
    pub started: u64,
    pub writes: usize,
    pub externals: usize,
    pub restores: usize,
    /// What the file was the last time kuro wrote, restored or noticed it.
    pub last_known: Disk,
    pub last_at: u64,
    pub lines: Vec<Line>,
}

/// One file in a hard-undo plan.
#[derive(Debug, Clone)]
pub struct PlanItem {
    pub entry: Entry,
    pub disk: Disk,
    /// The file on disk is not what kuro last saw or wrote: someone changed it since.
    pub changed_since: bool,
    /// The file was also changed outside kuro at some point (those changes are undone too).
    pub also_outside: bool,
    /// Restoring means deleting the file (it did not exist before kuro created it).
    pub will_delete: bool,
}

impl PlanItem {
    /// Needs the second confirmation.
    pub fn flagged(&self) -> bool {
        self.changed_since || self.also_outside
    }
}

#[derive(Debug)]
pub struct Store {
    pub root: PathBuf,
    pub opts: HistoryOptions,
    exclude: Vec<Regex>,
    /// The last error while recording, so the editor can say hard undo is unavailable instead of failing a save.
    pub last_error: std::sync::Mutex<Option<String>>,
}

fn now() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

pub fn crc32(data: &[u8]) -> u32 {
    let mut crc = 0xFFFF_FFFFu32;
    for &b in data {
        crc ^= b as u32;
        for _ in 0..8 {
            crc = if crc & 1 != 0 { (crc >> 1) ^ 0xEDB8_8320 } else { crc >> 1 };
        }
    }
    !crc
}

fn encode(line: &Line) -> String {
    let json = serde_json::to_string(line).expect("a log line serialises");
    format!("{json}\t{:08x}\n", crc32(json.as_bytes()))
}

/// Parse a log; a torn last line and any line whose checksum fails are ignored.
pub fn parse_log(text: &str) -> Vec<Line> {
    let mut out = Vec::new();
    for raw in text.split_inclusive('\n') {
        if !raw.ends_with('\n') {
            break; // torn: a crash in the middle of an append
        }
        let l = raw.trim_end_matches('\n');
        let Some((json, crc)) = l.rsplit_once('\t') else { continue };
        if u32::from_str_radix(crc, 16).ok() != Some(crc32(json.as_bytes())) {
            continue;
        }
        if let Ok(line) = serde_json::from_str::<Line>(json) {
            out.push(line);
        }
    }
    out
}

pub fn fold(key: &str, lines: Vec<Line>) -> Option<Entry> {
    let mut e: Option<Entry> = None;
    for l in &lines {
        match l {
            Line::File { path, existed, pre, size, at } => {
                if e.is_none() {
                    e = Some(Entry {
                        key: key.to_string(),
                        path: path.clone(),
                        existed: *existed,
                        baseline: pre.clone(),
                        baseline_size: *size,
                        started: *at,
                        writes: 0,
                        externals: 0,
                        restores: 0,
                        last_known: pre.clone().map(Disk::Hash).unwrap_or(Disk::Missing),
                        last_at: *at,
                        lines: vec![],
                    });
                }
            }
            Line::Write { after, at, .. } => {
                if let Some(e) = &mut e {
                    e.writes += 1;
                    e.last_known = Disk::Hash(after.clone());
                    e.last_at = *at;
                }
            }
            Line::External { hash, at } => {
                if let Some(e) = &mut e {
                    e.externals += 1;
                    e.last_known = hash.clone().map(Disk::Hash).unwrap_or(Disk::Missing);
                    e.last_at = *at;
                }
            }
            Line::Restored { after, at } => {
                if let Some(e) = &mut e {
                    e.restores += 1;
                    e.last_known = after.clone().map(Disk::Hash).unwrap_or(Disk::Missing);
                    e.last_at = *at;
                }
            }
        }
    }
    e.map(|mut e| {
        e.lines = lines;
        e
    })
}

pub fn real_path(p: &Path) -> PathBuf {
    if let Ok(c) = p.canonicalize() {
        return c;
    }
    // a file that does not exist yet: the real path of its folder plus its name
    let parent = p
        .parent()
        .filter(|d| !d.as_os_str().is_empty())
        .map(|d| d.canonicalize().unwrap_or_else(|_| d.to_path_buf()))
        .unwrap_or_else(|| std::env::current_dir().unwrap_or_default());
    match p.file_name() {
        Some(n) => parent.join(n),
        None => parent,
    }
}

pub fn key_of(path: &Path) -> String {
    hex(&sha256(real_path(path).to_string_lossy().as_bytes()))[..16].to_string()
}

struct Lock(#[allow(dead_code)] File);

impl Store {
    pub fn new(root: PathBuf, opts: HistoryOptions) -> Store {
        let exclude = opts.exclude.iter().filter_map(|g| Regex::new(&crate::ext::glob_to_regex(g)).ok()).collect();
        Store { root, opts, exclude, last_error: Default::default() }
    }

    /// `$XDG_STATE_HOME/kuro/history` (default `~/.local/state/kuro/history`).
    pub fn default_root() -> PathBuf {
        let home = std::env::var_os("HOME").map(PathBuf::from).unwrap_or_else(|| PathBuf::from("/tmp"));
        std::env::var_os("XDG_STATE_HOME")
            .filter(|v| !v.is_empty())
            .map(PathBuf::from)
            .unwrap_or_else(|| home.join(".local/state"))
            .join("kuro")
            .join("history")
    }

    fn dir(&self, key: &str) -> PathBuf {
        self.root.join("files").join(key)
    }

    fn blob(&self, hash: &str) -> PathBuf {
        self.root.join("blobs").join(hash)
    }

    fn lock(&self) -> io::Result<Lock> {
        fs::create_dir_all(&self.root)?;
        private_dir(&self.root);
        let f = OpenOptions::new().create(true).append(true).open(self.root.join("lock"))?;
        // SAFETY: flock on a descriptor we own.
        if unsafe { libc::flock(std::os::fd::AsRawFd::as_raw_fd(&f), libc::LOCK_EX) } != 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(Lock(f))
    }

    /// Is `path` recorded at all, with this content size?
    pub fn eligible(&self, path: &Path, size: u64) -> Result<(), Skip> {
        if !self.opts.enabled {
            return Err(Skip::Disabled);
        }
        let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        if self.exclude.iter().any(|r| r.is_match(&name)) {
            return Err(Skip::Excluded);
        }
        if size > self.opts.max_file_bytes {
            return Err(Skip::TooLarge(size));
        }
        Ok(())
    }

    fn read_entry(&self, key: &str) -> Option<Entry> {
        let text = fs::read(self.dir(key).join("log")).ok()?;
        fold(key, parse_log(&String::from_utf8_lossy(&text)))
    }

    pub fn entry_of(&self, path: &Path) -> Option<Entry> {
        self.read_entry(&key_of(path))
    }

    fn append(&self, key: &str, line: &Line) -> io::Result<()> {
        let dir = self.dir(key);
        fs::create_dir_all(&dir)?;
        if let Some(parent) = dir.parent() {
            private_dir(parent);
        }
        private_dir(&dir);
        let mut f = OpenOptions::new().append(true).create(true).open(dir.join("log"))?;
        f.write_all(encode(line).as_bytes())?;
        f.sync_all()
    }

    fn put_blob(&self, bytes: &[u8]) -> io::Result<String> {
        let hash = hex(&sha256(bytes));
        let dest = self.blob(&hash);
        if dest.exists() {
            return Ok(hash);
        }
        fs::create_dir_all(dest.parent().unwrap())?;
        private_dir(dest.parent().unwrap());
        static N: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let tmp = dest.with_extension(format!("tmp{}-{}", std::process::id(), N.fetch_add(1, std::sync::atomic::Ordering::Relaxed)));
        {
            let mut f = OpenOptions::new().write(true).create_new(true).open(&tmp)?;
            f.write_all(bytes)?;
            f.sync_all()?;
        }
        fs::rename(&tmp, &dest)?;
        Ok(hash)
    }

    fn fail(&self, e: impl std::fmt::Display) -> Skip {
        let msg = format!("hard undo cannot record this file: {e}");
        *self.last_error.lock().unwrap() = Some(msg.clone());
        Skip::Error(msg)
    }

    /// Call **before** kuro replaces `path`. `old` is what is on disk now (`None`: no such file); `new_len` the size about to be written.
    /// Records the baseline on the first write and any change made outside kuro since the last one.
    pub fn before_write(&self, path: &Path, old: Option<&[u8]>, new_len: u64) -> Result<(), Skip> {
        self.eligible(path, old.map_or(0, |b| b.len() as u64).max(new_len))?;
        let key = key_of(path);
        let _lock = self.lock().map_err(|e| self.fail(e))?;
        match self.read_entry(&key) {
            None => {
                let pre = match old {
                    Some(b) => Some(self.put_blob(b).map_err(|e| self.fail(e))?),
                    None => None,
                };
                let line = Line::File {
                    path: real_path(path).to_string_lossy().into_owned(),
                    existed: old.is_some(),
                    pre,
                    size: old.map_or(0, |b| b.len() as u64),
                    at: now(),
                };
                self.append(&key, &line).map_err(|e| self.fail(e))?;
            }
            Some(e) => {
                let now_disk = Disk::of(old);
                if now_disk != e.last_known {
                    let hash = match &now_disk {
                        Disk::Hash(h) => Some(h.clone()),
                        Disk::Missing => None,
                    };
                    self.append(&key, &Line::External { hash, at: now() }).map_err(|e| self.fail(e))?;
                }
            }
        }
        Ok(())
    }

    /// Call **after** kuro wrote `new` to `path`.
    pub fn after_write(&self, path: &Path, old: Option<&[u8]>, new: &[u8]) {
        if self.eligible(path, new.len() as u64).is_err() {
            return;
        }
        let key = key_of(path);
        let Ok(_lock) = self.lock() else { return };
        if self.read_entry(&key).is_none() {
            return; // before_write refused or failed: nothing to extend
        }
        let line = Line::Write { before: old.map(|b| hex(&sha256(b))), after: hex(&sha256(new)), at: now() };
        if let Err(e) = self.append(&key, &line) {
            self.fail(e);
        }
    }

    /// Call when a file is opened: a difference from what kuro last saw is a change made outside kuro.
    pub fn note_open(&self, path: &Path, bytes: Option<&[u8]>) {
        let key = key_of(path);
        let Ok(_lock) = self.lock() else { return };
        let Some(e) = self.read_entry(&key) else { return };
        let disk = Disk::of(bytes);
        if disk != e.last_known {
            let hash = match disk {
                Disk::Hash(h) => Some(h),
                Disk::Missing => None,
            };
            let _ = self.append(&key, &Line::External { hash, at: now() });
        }
    }

    // ----------------------------------------------------------------------------------------- hard undo

    fn plan_item(&self, e: Entry) -> PlanItem {
        let path = PathBuf::from(&e.path);
        let disk = Disk::of(fs::read(&path).ok().as_deref());
        let changed_since = disk != e.last_known;
        PlanItem { also_outside: e.externals > 0, will_delete: !e.existed, changed_since, disk, entry: e }
    }

    /// The plan item for the same file, recomputed now (the disk may have changed since the plan was made).
    pub fn refresh(&self, item: &PlanItem) -> Option<PlanItem> {
        self.read_entry(&item.entry.key).map(|e| self.plan_item(e))
    }

    /// The files a hard undo would touch: those with a recorded history whose content differs from the baseline. `Some(path)`: only that file.
    pub fn plan(&self, only: Option<&Path>) -> Vec<PlanItem> {
        let entries: Vec<Entry> = match only {
            Some(p) => self.entry_of(p).into_iter().collect(),
            None => self.entries(),
        };
        let mut out: Vec<PlanItem> = entries.into_iter().filter(|e| e.writes > 0 || e.externals > 0).map(|e| self.plan_item(e)).collect();
        out.retain(|i| i.disk != i.baseline_disk());
        out.sort_by(|a, b| a.entry.path.cmp(&b.entry.path));
        out
    }

    /// Put the file back as it was before kuro first wrote it (delete it when it did not exist). Atomic: the old or the new file, never half.
    pub fn restore(&self, item: &PlanItem) -> Result<(), String> {
        let path = PathBuf::from(&item.entry.path);
        let _lock = self.lock().map_err(|e| e.to_string())?;
        let after = match &item.entry.baseline {
            Some(h) => {
                let bytes = fs::read(self.blob(h))
                    .map_err(|e| format!("the saved original of {} is missing or unreadable: {e}", path.display()))?;
                if hex(&sha256(&bytes)) != *h {
                    return Err(format!("the saved original of {} is damaged (its checksum does not match)", path.display()));
                }
                write_atomic(&path, &bytes).map_err(|e| format!("cannot write {}: {e}", path.display()))?;
                Some(h.clone())
            }
            None => {
                match fs::remove_file(&path) {
                    Ok(()) => {}
                    Err(e) if e.kind() == io::ErrorKind::NotFound => {}
                    Err(e) => return Err(format!("cannot delete {}: {e}", path.display())),
                }
                None
            }
        };
        self.append(&item.entry.key, &Line::Restored { after, at: now() }).map_err(|e| e.to_string())
    }

    // ----------------------------------------------------------------------------------------- management

    pub fn entries(&self) -> Vec<Entry> {
        let mut v: Vec<Entry> = fs::read_dir(self.root.join("files"))
            .into_iter()
            .flatten()
            .flatten()
            .filter_map(|d| self.read_entry(&d.file_name().to_string_lossy()))
            .collect();
        v.sort_by(|a, b| a.path.cmp(&b.path));
        v
    }

    /// Bytes used by the history (logs and saved originals).
    pub fn size_bytes(&self) -> u64 {
        fn walk(p: &Path) -> u64 {
            fs::read_dir(p)
                .into_iter()
                .flatten()
                .flatten()
                .map(|e| match e.metadata() {
                    Ok(m) if m.is_dir() => walk(&e.path()),
                    Ok(m) => m.len(),
                    Err(_) => 0,
                })
                .sum()
        }
        walk(&self.root)
    }

    /// Delete the histories `keys` (or unique prefixes of them), then the saved originals nothing refers to any more.
    pub fn delete(&self, keys: &[String]) -> Result<Vec<String>, String> {
        let _lock = self.lock().map_err(|e| e.to_string())?;
        let all: Vec<String> = fs::read_dir(self.root.join("files"))
            .into_iter()
            .flatten()
            .flatten()
            .map(|d| d.file_name().to_string_lossy().into_owned())
            .collect();
        let mut removed = Vec::new();
        for k in keys {
            let matches: Vec<&String> = all.iter().filter(|a| a.starts_with(k.as_str())).collect();
            match matches.as_slice() {
                [one] => {
                    fs::remove_dir_all(self.dir(one)).map_err(|e| format!("cannot delete {one}: {e}"))?;
                    removed.push((*one).clone());
                }
                [] => return Err(format!("no history `{k}` (see kuro history list)")),
                _ => return Err(format!("`{k}` matches several histories; give more characters")),
            }
        }
        self.gc();
        Ok(removed)
    }

    pub fn clear(&self) -> Result<usize, String> {
        let _lock = self.lock().map_err(|e| e.to_string())?;
        let n = fs::read_dir(self.root.join("files")).map(|d| d.flatten().count()).unwrap_or(0);
        let _ = fs::remove_dir_all(self.root.join("files"));
        let _ = fs::remove_dir_all(self.root.join("blobs"));
        Ok(n)
    }

    /// Remove saved originals no log refers to (only called after a delete).
    fn gc(&self) {
        let used: std::collections::HashSet<String> = self.entries().into_iter().filter_map(|e| e.baseline).collect();
        for d in fs::read_dir(self.root.join("blobs")).into_iter().flatten().flatten() {
            let n = d.file_name().to_string_lossy().into_owned();
            if !used.contains(&n) {
                let _ = fs::remove_file(d.path());
            }
        }
    }
}

impl PlanItem {
    pub fn baseline_disk(&self) -> Disk {
        self.entry.baseline.clone().map(Disk::Hash).unwrap_or(Disk::Missing)
    }
}

/// Replace a file atomically, keeping its permissions (temp file, fsync, rename).
pub fn write_atomic(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let dir = path.parent().filter(|d| !d.as_os_str().is_empty()).unwrap_or(Path::new("."));
    let name = path.file_name().ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "no file name"))?;
    let tmp = dir.join(format!(".{}.kuro-tmp-{}", name.to_string_lossy(), std::process::id()));
    let result = (|| {
        let mut f = OpenOptions::new().write(true).create(true).truncate(true).open(&tmp)?;
        if let Ok(meta) = fs::metadata(path) {
            let _ = f.set_permissions(meta.permissions());
        }
        f.write_all(bytes)?;
        f.sync_all()?;
        drop(f);
        fs::rename(&tmp, path)?;
        if let Ok(d) = File::open(dir) {
            let _ = d.sync_all();
        }
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(&tmp);
    }
    result
}

fn private_dir(dir: &Path) {
    use std::os::unix::fs::PermissionsExt;
    let _ = fs::set_permissions(dir, fs::Permissions::from_mode(0o700));
}

/// Read a file fully, `None` when it does not exist.
pub fn read_if_exists(path: &Path) -> Option<Vec<u8>> {
    let mut buf = Vec::new();
    File::open(path).ok()?.read_to_end(&mut buf).ok()?;
    Some(buf)
}
