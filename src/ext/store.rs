//! Installing, listing, removing and loading extensions.
//!
//! Layout (default `~/.local/share/kuro/extensions/`):
//! ```text
//! registry.toml          one [[extension]] per installed extension: name, version, source, time, seal
//! <name>/                the installed files (kuro-ext.toml and whatever else the extension ships)
//! .lock                  flock while the registry or the folders change
//! ```
//! A crash leaves either the old or the new state: files are staged in a hidden folder, renamed into place, and the registry (which
//! decides what loads) is replaced atomically **last** when installing and **first** when removing. A folder without a registry entry
//! is never loaded; `kuro ext list` shows it as incomplete and `kuro ext remove` cleans it.

use super::manifest::{self, MANIFEST_FILE, Manifest};
use super::seal;
use serde::{Deserialize, Serialize};
use std::fs::{self, File};
use std::io::{self, Write};
use std::path::{Path, PathBuf};

pub const MAX_FILES: usize = 64;
pub const MAX_TOTAL_BYTES: u64 = 4 * 1024 * 1024;
pub const MAX_DEPTH: usize = 4;

#[derive(Debug, Clone)]
pub struct ExtPaths {
    /// The extensions folder (holds `registry.toml`).
    pub root: PathBuf,
    /// The secret the seals are made with.
    pub key: PathBuf,
}

impl ExtPaths {
    pub fn from_env() -> ExtPaths {
        let home = std::env::var_os("HOME").map(PathBuf::from).unwrap_or_else(|| PathBuf::from("/tmp"));
        let dir = |var: &str, fallback: &str| {
            std::env::var_os(var)
                .filter(|v| !v.is_empty())
                .map(|d| PathBuf::from(d).join("kuro"))
                .unwrap_or_else(|| home.join(fallback).join("kuro"))
        };
        ExtPaths {
            root: dir("XDG_DATA_HOME", ".local/share").join("extensions"),
            key: dir("XDG_STATE_HOME", ".local/state").join("ext.key"),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Entry {
    pub name: String,
    pub version: String,
    pub source: String,
    pub installed_at: u64,
    pub kuro_at_install: String,
    pub seal: String,
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct Registry {
    #[serde(default)]
    extension: Vec<Entry>,
}

#[derive(Debug)]
pub struct Installed {
    pub name: String,
    pub version: String,
    pub languages: Vec<String>,
    pub replaced: bool,
}

#[derive(Debug, PartialEq)]
pub enum Status {
    Ok,
    /// The files do not match the seal (edited, copied in by hand, or sealed by another key).
    BadSeal,
    MissingFiles,
    /// A folder with no registry entry (an install or removal was interrupted).
    Incomplete,
}

#[derive(Debug)]
pub struct Listed {
    pub name: String,
    pub version: String,
    pub languages: Vec<String>,
    pub kuro: String,
    pub source: String,
    pub status: Status,
}

pub struct LoadedExt {
    pub manifest: Manifest,
    pub dir: PathBuf,
}

struct Lock(#[allow(dead_code)] File);

fn lock(root: &Path) -> io::Result<Lock> {
    fs::create_dir_all(root)?;
    let f = File::options().create(true).append(true).open(root.join(".lock"))?;
    // SAFETY: flock on an fd we own.
    if unsafe { libc::flock(std::os::fd::AsRawFd::as_raw_fd(&f), libc::LOCK_EX) } != 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(Lock(f))
}

fn read_registry(root: &Path) -> Result<Registry, String> {
    match fs::read_to_string(root.join("registry.toml")) {
        Ok(t) => toml::from_str(&t).map_err(|e| format!("registry.toml is damaged ({}); fix or delete it", e.message().trim())),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(Registry::default()),
        Err(e) => Err(format!("cannot read registry.toml: {e}")),
    }
}

/// Replace a file atomically: write a temporary file next to it, flush it to disk, rename it over the target.
pub fn write_atomic(path: &Path, bytes: &[u8]) -> io::Result<()> {
    static N: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let tmp = path.with_extension(format!("tmp{}-{}", std::process::id(), N.fetch_add(1, std::sync::atomic::Ordering::Relaxed)));
    {
        let mut f = File::create(&tmp)?;
        f.write_all(bytes)?;
        f.sync_all()?;
    }
    fs::rename(&tmp, path)
}

fn write_registry(root: &Path, r: &Registry) -> Result<(), String> {
    let text = toml::to_string_pretty(r).map_err(|e| e.to_string())?;
    write_atomic(&root.join("registry.toml"), text.as_bytes()).map_err(|e| format!("cannot write registry.toml: {e}"))
}

fn now() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

/// Read a source directory into memory, refusing anything that is not a plain, small tree of regular files.
fn read_source(dir: &Path) -> Result<Vec<(String, Vec<u8>)>, String> {
    let meta = fs::metadata(dir).map_err(|e| format!("cannot read {}: {e}", dir.display()))?;
    if !meta.is_dir() {
        return Err(format!("{} is not a directory (a source is a folder containing {MANIFEST_FILE})", dir.display()));
    }
    let mut files = Vec::new();
    let mut total = 0u64;
    fn walk(base: &Path, dir: &Path, depth: usize, files: &mut Vec<(String, Vec<u8>)>, total: &mut u64) -> Result<(), String> {
        if depth > MAX_DEPTH {
            return Err(format!("folders nested more than {MAX_DEPTH} deep are not accepted"));
        }
        let mut entries: Vec<_> = fs::read_dir(dir)
            .map_err(|e| format!("cannot read {}: {e}", dir.display()))?
            .collect::<Result<_, _>>()
            .map_err(|e| e.to_string())?;
        entries.sort_by_key(|e| e.file_name());
        for e in entries {
            let path = e.path();
            let rel = path.strip_prefix(base).unwrap().to_string_lossy().into_owned();
            let ft = e.file_type().map_err(|e| e.to_string())?;
            if ft.is_symlink() {
                return Err(format!("{rel} is a symbolic link; links are not accepted"));
            }
            if rel.split('/').any(|p| p.starts_with('.')) {
                return Err(format!("{rel}: hidden files and folders are not accepted"));
            }
            if ft.is_dir() {
                walk(base, &path, depth + 1, files, total)?;
            } else if ft.is_file() {
                let len = e.metadata().map_err(|e| e.to_string())?.len();
                *total += len;
                if files.len() >= MAX_FILES || *total > MAX_TOTAL_BYTES {
                    return Err(format!("too large: at most {MAX_FILES} files and {} MiB are accepted", MAX_TOTAL_BYTES / 1024 / 1024));
                }
                let bytes = fs::read(&path).map_err(|e| format!("cannot read {rel}: {e}"))?;
                files.push((rel, bytes));
            } else {
                return Err(format!("{rel} is not a regular file"));
            }
        }
        Ok(())
    }
    walk(dir, dir, 0, &mut files, &mut total)?;
    Ok(files)
}

fn rand_suffix() -> String {
    static N: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let mut b = [0u8; 4];
    if let Ok(mut f) = File::open("/dev/urandom") {
        let _ = io::Read::read_exact(&mut f, &mut b);
    }
    format!("{}-{}-{}", std::process::id(), N.fetch_add(1, std::sync::atomic::Ordering::Relaxed), seal::hex(&b))
}

fn read_installed_files(dir: &Path) -> Result<Vec<(String, Vec<u8>)>, String> {
    read_source(dir)
}

/// Remove leftovers of interrupted installs (hidden staging folders).
fn sweep(root: &Path) {
    if let Ok(rd) = fs::read_dir(root) {
        for e in rd.flatten() {
            let n = e.file_name().to_string_lossy().into_owned();
            if n.starts_with(".staging-") || n.starts_with(".old-") {
                let _ = fs::remove_dir_all(e.path());
            }
        }
    }
}

pub fn install(paths: &ExtPaths, source: &Path, force: bool, kuro_version: &str) -> Result<Installed, String> {
    let files = read_source(source)?;
    let manifest_bytes = files
        .iter()
        .find(|(p, _)| p == MANIFEST_FILE)
        .map(|(_, b)| b)
        .ok_or_else(|| format!("{} has no {MANIFEST_FILE}", source.display()))?;
    let text = std::str::from_utf8(manifest_bytes).map_err(|_| format!("{MANIFEST_FILE} is not valid UTF-8"))?;
    let m = manifest::parse(text, kuro_version).map_err(|errs| errs.join("\n"))?;

    let key = seal::key_or_create(&paths.key).map_err(|e| format!("cannot create the extension key {}: {e}", paths.key.display()))?;
    let _lock = lock(&paths.root).map_err(|e| format!("cannot lock {}: {e}", paths.root.display()))?;
    sweep(&paths.root);
    let mut reg = read_registry(&paths.root)?;
    let existing = reg.extension.iter().position(|e| e.name == m.name);
    if existing.is_some() && !force {
        return Err(format!("`{}` is already installed (use --force to replace it)", m.name));
    }

    // stage, then rename into place
    let staging = paths.root.join(format!(".staging-{}", rand_suffix()));
    let stage = (|| -> io::Result<()> {
        for (rel, bytes) in &files {
            let target = staging.join(rel);
            fs::create_dir_all(target.parent().unwrap())?;
            let mut f = File::create(&target)?;
            f.write_all(bytes)?;
            f.sync_all()?;
        }
        Ok(())
    })();
    if let Err(e) = stage {
        let _ = fs::remove_dir_all(&staging);
        return Err(format!("cannot write the extension: {e}"));
    }
    let dest = paths.root.join(&m.name);
    let old = paths.root.join(format!(".old-{}", rand_suffix()));
    let had_dir = dest.exists();
    if had_dir {
        fs::rename(&dest, &old).map_err(|e| format!("cannot replace {}: {e}", dest.display()))?;
    }
    if let Err(e) = fs::rename(&staging, &dest) {
        if had_dir {
            let _ = fs::rename(&old, &dest);
        }
        let _ = fs::remove_dir_all(&staging);
        return Err(format!("cannot install into {}: {e}", dest.display()));
    }
    // the registry decides what loads, so it is written last
    let entry = Entry {
        name: m.name.clone(),
        version: m.version.clone(),
        source: source.canonicalize().unwrap_or_else(|_| source.to_path_buf()).display().to_string(),
        installed_at: now(),
        kuro_at_install: kuro_version.to_string(),
        seal: seal::seal(&key, &files),
    };
    match existing {
        Some(i) => reg.extension[i] = entry,
        None => reg.extension.push(entry),
    }
    if let Err(e) = write_registry(&paths.root, &reg) {
        // put things back as they were
        let _ = fs::remove_dir_all(&dest);
        if had_dir {
            let _ = fs::rename(&old, &dest);
        }
        return Err(e);
    }
    let _ = fs::remove_dir_all(&old);
    Ok(Installed {
        name: m.name,
        version: m.version,
        languages: m.language.iter().map(|l| l.name.clone()).collect(),
        replaced: existing.is_some(),
    })
}

pub fn remove(paths: &ExtPaths, name: &str) -> Result<String, String> {
    if !manifest::validate_name(name) {
        return Err(format!("`{name}` is not a valid extension name"));
    }
    let _lock = lock(&paths.root).map_err(|e| format!("cannot lock {}: {e}", paths.root.display()))?;
    sweep(&paths.root);
    let mut reg = read_registry(&paths.root)?;
    let dir = paths.root.join(name);
    let pos = reg.extension.iter().position(|e| e.name == name);
    if pos.is_none() && !dir.exists() {
        return Err(format!("`{name}` is not installed"));
    }
    // the registry first: a crash afterwards leaves an unregistered folder (never loaded), not an entry without files
    let msg = match pos {
        Some(i) => {
            reg.extension.remove(i);
            write_registry(&paths.root, &reg)?;
            format!("removed `{name}`")
        }
        None => format!("removed the incomplete folder of `{name}`"),
    };
    if dir.exists() {
        fs::remove_dir_all(&dir)
            .map_err(|e| format!("`{name}` is unregistered but its folder {} could not be deleted: {e}", dir.display()))?;
    }
    Ok(msg)
}

fn check_entry(paths: &ExtPaths, key: Option<&[u8]>, e: &Entry) -> (Status, Option<Manifest>) {
    let dir = paths.root.join(&e.name);
    let Ok(files) = read_installed_files(&dir) else { return (Status::MissingFiles, None) };
    let Some(key) = key else { return (Status::BadSeal, None) };
    if !seal::verify(key, &files, &e.seal) {
        return (Status::BadSeal, None);
    }
    let Some(text) = files.iter().find(|(p, _)| p == MANIFEST_FILE).and_then(|(_, b)| std::str::from_utf8(b).ok()) else {
        return (Status::MissingFiles, None);
    };
    match manifest::parse(text, env!("CARGO_PKG_VERSION")) {
        Ok(m) => (Status::Ok, Some(m)),
        // sealed but not valid for this kuro (e.g. after an upgrade): the loader reports why; `list` still shows what it is
        Err(_) => (Status::Ok, toml::from_str::<Manifest>(text).ok()),
    }
}

pub fn list(paths: &ExtPaths) -> Result<Vec<Listed>, String> {
    let reg = read_registry(&paths.root)?;
    let key = seal::read_key(&paths.key);
    let mut out = Vec::new();
    for e in &reg.extension {
        let (status, m) = check_entry(paths, key.as_deref(), e);
        let (languages, kuro) = match &m {
            Some(m) => (m.language.iter().map(|l| l.name.clone()).collect(), m.kuro.clone()),
            None => (vec![], "?".into()),
        };
        out.push(Listed { name: e.name.clone(), version: e.version.clone(), languages, kuro, source: e.source.clone(), status });
    }
    if let Ok(rd) = fs::read_dir(&paths.root) {
        let mut orphans: Vec<String> = rd
            .flatten()
            .filter(|d| d.path().is_dir())
            .map(|d| d.file_name().to_string_lossy().into_owned())
            .filter(|n| !n.starts_with('.') && !reg.extension.iter().any(|e| &e.name == n))
            .collect();
        orphans.sort();
        for n in orphans {
            out.push(Listed {
                name: n,
                version: "?".into(),
                languages: vec![],
                kuro: "?".into(),
                source: "?".into(),
                status: Status::Incomplete,
            });
        }
    }
    Ok(out)
}

/// Load every installed extension that passes its checks; the rest become warnings (kuro still starts).
pub fn load_all(paths: &ExtPaths, kuro_version: &str) -> (Vec<LoadedExt>, Vec<String>) {
    let mut loaded = Vec::new();
    let mut warnings = Vec::new();
    let reg = match read_registry(&paths.root) {
        Ok(r) => r,
        Err(e) => return (loaded, vec![format!("extensions: {e}")]),
    };
    if reg.extension.is_empty() {
        return (loaded, warnings);
    }
    let key = seal::read_key(&paths.key);
    for e in &reg.extension {
        let dir = paths.root.join(&e.name);
        let files = match read_installed_files(&dir) {
            Ok(f) => f,
            Err(_) => {
                warnings.push(format!("extension `{}` skipped: its files are missing (kuro ext remove {})", e.name, e.name));
                continue;
            }
        };
        let Some(k) = key.as_deref() else {
            warnings.push(format!(
                "extension `{}` skipped: the extension key {} is missing, so nothing can be verified (reinstall it)",
                e.name,
                paths.key.display()
            ));
            continue;
        };
        if !seal::verify(k, &files, &e.seal) {
            warnings.push(format!("extension `{}` skipped: its files do not match the seal made when it was installed (edited, copied in, or installed by another user); reinstall it with kuro ext install --force", e.name));
            continue;
        }
        let text = files.iter().find(|(p, _)| p == MANIFEST_FILE).and_then(|(_, b)| std::str::from_utf8(b).ok()).unwrap_or("");
        match manifest::parse(text, kuro_version) {
            Ok(m) if m.name == e.name => loaded.push(LoadedExt { manifest: m, dir }),
            Ok(m) => warnings.push(format!("extension `{}` skipped: its manifest is named `{}`", e.name, m.name)),
            Err(errs) => warnings.push(format!("extension `{}` skipped: {}", e.name, errs.join("; "))),
        }
    }
    (loaded, warnings)
}
