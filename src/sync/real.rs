//! The real file system and journal.

use super::protocol::Committed;
use super::storage::{Fs, Journal, JournalContents, Stamp};
use std::fs::{self, File, OpenOptions};
use std::io::{self, BufRead, BufReader, BufWriter, Write};
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

pub struct RealFs;

impl Fs for RealFs {
    fn stamp(&self, path: &Path) -> io::Result<Option<Stamp>> {
        match fs::metadata(path) {
            Ok(m) => {
                let mtime_ns = m.modified().ok().and_then(|t| t.duration_since(UNIX_EPOCH).ok()).map(|d| d.as_nanos()).unwrap_or(0);
                Ok(Some(Stamp { len: m.len(), mtime_ns }))
            }
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(e),
        }
    }

    fn read(&self, path: &Path) -> io::Result<Vec<u8>> {
        fs::read(path)
    }

    /// Write to a temporary file in the same directory, flush it to disk, then rename it over the original, so
    /// a crash leaves either the old complete file or the new complete file. The original's permissions are kept.
    fn write_atomic(&mut self, path: &Path, bytes: &[u8]) -> io::Result<()> {
        crate::history::write_atomic(path, bytes)
    }
}

/// JSON-lines journal: a header line `{"base_hash":…,"base_rev":…}` then one `Committed` per line.
/// Reading tolerates a torn last line (a crash in the middle of a write).
pub struct FileJournal {
    path: PathBuf,
    out: Option<BufWriter<File>>,
}

#[derive(serde::Serialize, serde::Deserialize)]
struct Header {
    base_hash: String,
    base_rev: u64,
}

impl FileJournal {
    pub fn new(path: PathBuf) -> FileJournal {
        FileJournal { path, out: None }
    }

    fn open_append(&mut self) -> io::Result<&mut BufWriter<File>> {
        if self.out.is_none() {
            if let Some(dir) = self.path.parent() {
                fs::create_dir_all(dir)?;
                private_dir(dir);
            }
            let f = OpenOptions::new().append(true).create(true).open(&self.path)?;
            private_file(&f);
            self.out = Some(BufWriter::new(f));
        }
        Ok(self.out.as_mut().unwrap())
    }
}

#[cfg(unix)]
fn private_dir(d: &Path) {
    use std::os::unix::fs::PermissionsExt;
    let _ = fs::set_permissions(d, fs::Permissions::from_mode(0o700));
}

#[cfg(not(unix))]
fn private_dir(_: &Path) {}

#[cfg(unix)]
fn private_file(f: &File) {
    use std::os::unix::fs::PermissionsExt;
    let _ = f.set_permissions(fs::Permissions::from_mode(0o600));
}

#[cfg(not(unix))]
fn private_file(_: &File) {}

impl Journal for FileJournal {
    fn append(&mut self, entry: &Committed) -> io::Result<()> {
        let line = super::protocol::encode(entry);
        let w = self.open_append()?;
        w.write_all(line.as_bytes())?;
        w.write_all(b"\n")
    }

    fn flush(&mut self) -> io::Result<()> {
        if let Some(w) = self.out.as_mut() {
            w.flush()?;
            w.get_ref().sync_data()?;
        }
        Ok(())
    }

    fn reset(&mut self, base_hash: &str, base_rev: u64) -> io::Result<()> {
        self.out = None;
        if let Some(dir) = self.path.parent() {
            fs::create_dir_all(dir)?;
            private_dir(dir);
        }
        let tmp = self.path.with_extension("journal.tmp");
        {
            let mut f = File::create(&tmp)?;
            private_file(&f);
            let header = super::protocol::encode(&Header { base_hash: base_hash.to_string(), base_rev });
            f.write_all(header.as_bytes())?;
            f.write_all(b"\n")?;
            f.sync_all()?;
        }
        fs::rename(&tmp, &self.path)
    }

    fn replace(&mut self, base_hash: &str, base_rev: u64, entries: &[Committed]) -> io::Result<()> {
        self.out = None;
        if let Some(dir) = self.path.parent() {
            fs::create_dir_all(dir)?;
            private_dir(dir);
        }
        let tmp = self.path.with_extension("journal.tmp");
        {
            let mut f = BufWriter::new(File::create(&tmp)?);
            private_file(f.get_ref());
            f.write_all(super::protocol::encode(&Header { base_hash: base_hash.to_string(), base_rev }).as_bytes())?;
            f.write_all(b"\n")?;
            for e in entries {
                f.write_all(super::protocol::encode(e).as_bytes())?;
                f.write_all(b"\n")?;
            }
            f.flush()?;
            f.get_ref().sync_all()?;
        }
        fs::rename(&tmp, &self.path)
    }

    fn load(&self) -> io::Result<Option<JournalContents>> {
        let f = match File::open(&self.path) {
            Ok(f) => f,
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(None),
            Err(e) => return Err(e),
        };
        let mut lines = BufReader::new(f).lines();
        let Some(Ok(first)) = lines.next() else { return Ok(None) };
        let Ok(h) = super::protocol::decode::<Header>(&first) else { return Ok(None) };
        let mut entries = vec![];
        for l in lines {
            let Ok(l) = l else { break };
            match super::protocol::decode::<Committed>(&l) {
                Ok(e) => entries.push(e),
                Err(_) => break, // a torn last line: ignore it and anything after
            }
        }
        Ok(Some(JournalContents { base_hash: h.base_hash, base_rev: h.base_rev, entries }))
    }
}

#[cfg(test)]
mod tests {
    use super::super::ot::ins;
    use super::super::protocol::{Committed, Kind};
    use super::*;

    fn entry(rev: u64, text: &str) -> Committed {
        Committed { rev, origin: 1, id: None, kind: Kind::Edit, ops: vec![ins(0, text)], focus: None }
    }

    #[test]
    fn atomic_write_replaces_the_file_and_leaves_no_temp_files() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("a.txt");
        fs::write(&p, "old").unwrap();
        RealFs.write_atomic(&p, b"new content").unwrap();
        assert_eq!(fs::read_to_string(&p).unwrap(), "new content");
        let names: Vec<_> = fs::read_dir(d.path()).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().into_owned()).collect();
        assert_eq!(names, vec!["a.txt"], "no temporary file is left behind: {names:?}");
    }

    #[test]
    fn atomic_write_creates_missing_files_and_keeps_permissions() {
        use std::os::unix::fs::PermissionsExt;
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("script.sh");
        RealFs.write_atomic(&p, b"#!/bin/sh\n").unwrap();
        assert_eq!(fs::read(&p).unwrap(), b"#!/bin/sh\n");
        fs::set_permissions(&p, fs::Permissions::from_mode(0o755)).unwrap();
        RealFs.write_atomic(&p, b"#!/bin/sh\necho hi\n").unwrap();
        assert_eq!(fs::metadata(&p).unwrap().permissions().mode() & 0o777, 0o755, "the executable bit survives a save");
    }

    #[test]
    fn atomic_write_to_a_missing_directory_fails_cleanly() {
        let d = tempfile::tempdir().unwrap();
        assert!(RealFs.write_atomic(&d.path().join("nope/x.txt"), b"x").is_err());
    }

    #[test]
    fn stamps_change_when_the_file_changes() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("s.txt");
        assert_eq!(RealFs.stamp(&p).unwrap(), None);
        fs::write(&p, "a").unwrap();
        let s1 = RealFs.stamp(&p).unwrap().unwrap();
        fs::write(&p, "abc").unwrap();
        let s2 = RealFs.stamp(&p).unwrap().unwrap();
        assert_ne!(s1, s2);
    }

    #[test]
    fn journal_round_trips_and_resets() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("state/recovery/j.journal");
        let mut j = FileJournal::new(p.clone());
        assert!(j.load().unwrap().is_none());
        j.reset("hash1", 4).unwrap();
        j.append(&entry(5, "a\nb")).unwrap();
        j.append(&entry(6, "é😀")).unwrap();
        j.flush().unwrap();
        let c = j.load().unwrap().unwrap();
        assert_eq!((c.base_hash.as_str(), c.base_rev, c.entries.len()), ("hash1", 4, 2));
        assert_eq!(c.entries[1].ops, vec![ins(0, "é😀")]);
        j.reset("hash2", 6).unwrap();
        let c = j.load().unwrap().unwrap();
        assert_eq!((c.base_hash.as_str(), c.entries.len()), ("hash2", 0), "a save starts the journal over");
        j.append(&entry(7, "x")).unwrap();
        j.flush().unwrap();
        assert_eq!(j.load().unwrap().unwrap().entries.len(), 1);
    }

    #[test]
    fn replace_swaps_the_whole_journal_at_once() {
        let d = tempfile::tempdir().unwrap();
        let mut j = FileJournal::new(d.path().join("j.journal"));
        j.reset("h", 0).unwrap();
        for i in 1..=5 {
            j.append(&entry(i, "x")).unwrap();
        }
        j.flush().unwrap();
        j.replace("h", 4, &[entry(5, "net change")]).unwrap();
        let c = j.load().unwrap().unwrap();
        assert_eq!((c.base_rev, c.entries.len(), c.entries[0].rev), (4, 1, 5));
        j.append(&entry(6, "more")).unwrap();
        j.flush().unwrap();
        assert_eq!(j.load().unwrap().unwrap().entries.len(), 2, "appending continues after a replace");
        assert!(!d.path().join("j.journal.tmp").exists());
    }

    #[test]
    fn unflushed_entries_are_not_durable_but_flushed_ones_are() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("j.journal");
        let mut j = FileJournal::new(p.clone());
        j.reset("h", 0).unwrap();
        j.append(&entry(1, "one")).unwrap();
        j.flush().unwrap();
        j.append(&entry(2, "two")).unwrap();
        // another reader (a restarted process) sees only what was flushed to the file
        let seen = FileJournal::new(p).load().unwrap().unwrap();
        assert_eq!(seen.entries.len(), 1);
    }

    #[test]
    fn a_torn_last_line_is_ignored_not_fatal() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("j.journal");
        let mut j = FileJournal::new(p.clone());
        j.reset("h", 0).unwrap();
        j.append(&entry(1, "one")).unwrap();
        j.flush().unwrap();
        drop(j);
        let mut f = OpenOptions::new().append(true).open(&p).unwrap();
        f.write_all(b"{\"rev\":2,\"origin\":1,\"id\":null,\"ki").unwrap(); // power cut mid-line
        let c = FileJournal::new(p).load().unwrap().unwrap();
        assert_eq!(c.entries.len(), 1);
    }

    #[test]
    fn a_corrupt_or_empty_journal_is_treated_as_absent() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("j.journal");
        fs::write(&p, "").unwrap();
        assert!(FileJournal::new(p.clone()).load().unwrap().is_none());
        fs::write(&p, "not json\n").unwrap();
        assert!(FileJournal::new(p).load().unwrap().is_none());
    }

    #[test]
    fn the_journal_is_private_to_the_user() {
        use std::os::unix::fs::PermissionsExt;
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("recovery/j.journal");
        let mut j = FileJournal::new(p.clone());
        j.reset("h", 0).unwrap();
        j.append(&entry(1, "secret code")).unwrap();
        j.flush().unwrap();
        assert_eq!(fs::metadata(&p).unwrap().permissions().mode() & 0o777, 0o600);
        assert_eq!(fs::metadata(p.parent().unwrap()).unwrap().permissions().mode() & 0o777, 0o700);
    }
}
