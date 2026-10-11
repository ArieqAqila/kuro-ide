//! In-memory file system and journal for tests, with crash and failure injection. Cloning a handle shares the
//! state, so a test can "kill" a leader (drop it) and start another on the same storage, exactly like processes
//! sharing a disk.

use super::protocol::Committed;
use super::storage::{Fs, Journal, JournalContents, Stamp};
use std::collections::BTreeMap;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

#[derive(Default)]
pub struct SimFsState {
    pub files: BTreeMap<PathBuf, Vec<u8>>,
    mtimes: BTreeMap<PathBuf, u128>,
    clock: u128,
    pub write_calls: usize,
    /// The n-th `write_atomic` call (1-based) crashes after writing its temporary file: the real file is untouched.
    pub crash_on_write: Option<usize>,
    pub fail_writes: bool,
    pub temp_files_left: usize,
}

#[derive(Clone, Default)]
pub struct SimFs(pub Arc<Mutex<SimFsState>>);

impl SimFs {
    pub fn with_file(path: &str, content: &str) -> SimFs {
        let fs = SimFs::default();
        fs.external_write(Path::new(path), content.as_bytes());
        fs
    }

    /// Another program changes the file.
    pub fn external_write(&self, path: &Path, bytes: &[u8]) {
        let mut s = self.0.lock().unwrap();
        s.clock += 1;
        let t = s.clock;
        s.files.insert(path.to_path_buf(), bytes.to_vec());
        s.mtimes.insert(path.to_path_buf(), t);
    }

    pub fn read_string(&self, path: &str) -> Option<String> {
        self.0.lock().unwrap().files.get(Path::new(path)).map(|b| String::from_utf8_lossy(b).into_owned())
    }

    pub fn write_calls(&self) -> usize {
        self.0.lock().unwrap().write_calls
    }
}

impl Fs for SimFs {
    fn stamp(&self, path: &Path) -> io::Result<Option<Stamp>> {
        let s = self.0.lock().unwrap();
        Ok(s.files.get(path).map(|b| Stamp { len: b.len() as u64, mtime_ns: s.mtimes.get(path).copied().unwrap_or(0) }))
    }

    fn read(&self, path: &Path) -> io::Result<Vec<u8>> {
        self.0.lock().unwrap().files.get(path).cloned().ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "no such file"))
    }

    fn write_atomic(&mut self, path: &Path, bytes: &[u8]) -> io::Result<()> {
        let mut s = self.0.lock().unwrap();
        s.write_calls += 1;
        if s.crash_on_write == Some(s.write_calls) {
            s.temp_files_left += 1; // the temporary file was written, the rename never happened
            return Err(io::Error::other("simulated crash during save"));
        }
        if s.fail_writes {
            return Err(io::Error::other("simulated disk full"));
        }
        s.clock += 1;
        let t = s.clock;
        s.files.insert(path.to_path_buf(), bytes.to_vec());
        s.mtimes.insert(path.to_path_buf(), t);
        Ok(())
    }
}

#[derive(Default)]
pub struct SimJournalState {
    pub base_hash: String,
    pub base_rev: u64,
    pub exists: bool,
    pub durable: Vec<Committed>,
    /// Appended but not yet flushed: lost if the process dies now.
    pub pending: Vec<Committed>,
    pub flushes: usize,
}

#[derive(Clone, Default)]
pub struct SimJournal(pub Arc<Mutex<SimJournalState>>);

impl SimJournal {
    /// Power cut / SIGKILL: whatever was not flushed is gone.
    pub fn crash(&self) {
        self.0.lock().unwrap().pending.clear();
    }
    pub fn durable_len(&self) -> usize {
        self.0.lock().unwrap().durable.len()
    }
}

impl Journal for SimJournal {
    fn append(&mut self, entry: &Committed) -> io::Result<()> {
        self.0.lock().unwrap().pending.push(entry.clone());
        Ok(())
    }
    fn flush(&mut self) -> io::Result<()> {
        let mut s = self.0.lock().unwrap();
        let p = std::mem::take(&mut s.pending);
        s.durable.extend(p);
        s.flushes += 1;
        Ok(())
    }
    fn reset(&mut self, base_hash: &str, base_rev: u64) -> io::Result<()> {
        let mut s = self.0.lock().unwrap();
        s.base_hash = base_hash.to_string();
        s.base_rev = base_rev;
        s.exists = true;
        s.durable.clear();
        s.pending.clear();
        Ok(())
    }
    fn replace(&mut self, base_hash: &str, base_rev: u64, entries: &[Committed]) -> io::Result<()> {
        let mut s = self.0.lock().unwrap();
        s.base_hash = base_hash.to_string();
        s.base_rev = base_rev;
        s.exists = true;
        s.durable = entries.to_vec();
        s.pending.clear();
        Ok(())
    }
    fn load(&self) -> io::Result<Option<JournalContents>> {
        let s = self.0.lock().unwrap();
        Ok(s.exists.then(|| JournalContents { base_hash: s.base_hash.clone(), base_rev: s.base_rev, entries: s.durable.clone() }))
    }
}
