//! What the leader needs from the outside world, as traits so tests can inject crashes and failures:
//! reading and atomically writing the file, and the recovery journal.

use super::protocol::Committed;
use ropey::Rope;
use sha2::{Digest, Sha256};
use std::io;
use std::path::Path;

/// Cheap change detector for a file (size and modification time), checked often; the content hash is only
/// computed when this changes.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Stamp {
    pub len: u64,
    pub mtime_ns: u128,
}

pub trait Fs: Send {
    /// `Ok(None)` when the file does not exist.
    fn stamp(&self, path: &Path) -> io::Result<Option<Stamp>>;
    fn read(&self, path: &Path) -> io::Result<Vec<u8>>;
    /// Replace `path` with `bytes` so that a crash at any point leaves either the old or the new complete file.
    fn write_atomic(&mut self, path: &Path, bytes: &[u8]) -> io::Result<()>;
}

/// The recovery journal: the committed entries since the last save, so unsaved work survives every process dying.
pub trait Journal: Send {
    /// Queue an entry (cheap, not yet durable).
    fn append(&mut self, entry: &Committed) -> io::Result<()>;
    /// Make everything appended so far durable.
    fn flush(&mut self) -> io::Result<()>;
    /// Start over after a save: the file now has content hash `base_hash` and the document is at `base_rev`.
    fn reset(&mut self, base_hash: &str, base_rev: u64) -> io::Result<()>;
    /// Atomically replace the whole journal (used to compact it): either the old contents or the new, never neither.
    fn replace(&mut self, base_hash: &str, base_rev: u64, entries: &[Committed]) -> io::Result<()> {
        self.reset(base_hash, base_rev)?;
        for e in entries {
            self.append(e)?;
        }
        self.flush()
    }
    /// Whatever a previous session left behind.
    fn load(&self) -> io::Result<Option<JournalContents>>;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JournalContents {
    pub base_hash: String,
    pub base_rev: u64,
    pub entries: Vec<Committed>,
}

pub fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

pub fn hash_bytes(b: &[u8]) -> String {
    hex(&Sha256::digest(b))
}

pub fn hash_rope(r: &Rope) -> String {
    let mut h = Sha256::new();
    for chunk in r.chunks() {
        h.update(chunk.as_bytes());
    }
    hex(&h.finalize())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rope_and_bytes_hash_the_same() {
        let text = "hello\nwörld 日本\n".repeat(1000);
        assert_eq!(hash_rope(&Rope::from_str(&text)), hash_bytes(text.as_bytes()));
        assert_ne!(hash_bytes(b"a"), hash_bytes(b"b"));
        assert_eq!(hash_bytes(b"").len(), 64);
    }
}
