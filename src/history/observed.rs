//! Hooking the history into every place kuro writes a file.

use super::{Skip, Store, read_if_exists};
use crate::sync::storage::{Fs, Stamp};
use std::io;
use std::path::Path;
use std::sync::Arc;

/// Something told about every write kuro makes to a file.
pub trait WriteObserver: Send + Sync + std::fmt::Debug {
    /// Before `path` is replaced by `new_len` bytes; `old` is what is there now. An `Err` does not stop the write.
    fn before_write(&self, path: &Path, old: Option<&[u8]>, new_len: u64) -> Result<(), Skip>;
    fn after_write(&self, path: &Path, old: Option<&[u8]>, new: &[u8]);
}

impl WriteObserver for Store {
    fn before_write(&self, path: &Path, old: Option<&[u8]>, new_len: u64) -> Result<(), Skip> {
        Store::before_write(self, path, old, new_len)
    }
    fn after_write(&self, path: &Path, old: Option<&[u8]>, new: &[u8]) {
        Store::after_write(self, path, old, new)
    }
}

/// Write `bytes` to `path` the way kuro always does (atomically), telling `observer` first and after.
///
/// Returns why the history did *not* record this write, if it did not (the write itself still happened).
pub fn write_observed(observer: Option<&Arc<dyn WriteObserver>>, path: &Path, bytes: &[u8]) -> io::Result<Option<Skip>> {
    let old = if observer.is_some() { read_if_exists(path) } else { None };
    let skip = observer.and_then(|o| o.before_write(path, old.as_deref(), bytes.len() as u64).err());
    super::write_atomic(path, bytes)?;
    if let Some(o) = observer {
        o.after_write(path, old.as_deref(), bytes);
    }
    Ok(skip)
}

/// The real file system for live sync, with every write reported to the history.
pub struct ObservedFs {
    pub observer: Arc<dyn WriteObserver>,
}

impl Fs for ObservedFs {
    fn stamp(&self, path: &Path) -> io::Result<Option<Stamp>> {
        crate::sync::real::RealFs.stamp(path)
    }
    fn read(&self, path: &Path) -> io::Result<Vec<u8>> {
        std::fs::read(path)
    }
    fn write_atomic(&mut self, path: &Path, bytes: &[u8]) -> io::Result<()> {
        // same atomic replace as RealFs (it keeps permissions and fsyncs the folder)
        let old = read_if_exists(path);
        let _ = self.observer.before_write(path, old.as_deref(), bytes.len() as u64);
        crate::sync::real::RealFs.write_atomic(path, bytes)?;
        self.observer.after_write(path, old.as_deref(), bytes);
        Ok(())
    }
}
