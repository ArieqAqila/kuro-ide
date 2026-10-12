//! The hard-undo store: baselines, outside changes, restores, crashes, management. Throw-away directories only.

use kuro::history::observed::{ObservedFs, WriteObserver, write_observed};
use kuro::history::{Disk, HistoryOptions, Line, Skip, Store, crc32, parse_log};
use kuro::sync::storage::Fs;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;

struct Env {
    dir: tempfile::TempDir,
    store: Store,
}

fn env() -> Env {
    env_with(HistoryOptions::default())
}

fn env_with(opts: HistoryOptions) -> Env {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::new(dir.path().join("state/history"), opts);
    Env { dir, store }
}

impl Env {
    fn file(&self, name: &str) -> PathBuf {
        self.dir.path().join("work").join(name)
    }

    fn put(&self, name: &str, bytes: &[u8]) -> PathBuf {
        let p = self.file(name);
        fs::create_dir_all(p.parent().unwrap()).unwrap();
        fs::write(&p, bytes).unwrap();
        p
    }

    /// kuro saves `new` over `path`: the history is told before and after, exactly like a real save.
    fn save(&self, path: &Path, new: &[u8]) {
        let old = fs::read(path).ok();
        self.store.before_write(path, old.as_deref(), new.len() as u64).unwrap();
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, new).unwrap();
        self.store.after_write(path, old.as_deref(), new);
    }

    fn log_path(&self, path: &Path) -> PathBuf {
        self.store.root.join("files").join(kuro::history::key_of(path)).join("log")
    }
}

#[test]
fn the_first_write_records_the_original_and_later_writes_never_move_it() {
    let e = env();
    let p = e.put("a.txt", b"original\n");
    e.save(&p, b"edit one\n");
    e.save(&p, b"edit two\n");
    e.save(&p, b"edit three\n");
    let entry = e.store.entry_of(&p).unwrap();
    assert!(entry.existed);
    assert_eq!(entry.writes, 3);
    assert_eq!(entry.baseline_size, 9);
    let blob = fs::read(e.store.root.join("blobs").join(entry.baseline.as_ref().unwrap())).unwrap();
    assert_eq!(blob, b"original\n", "the content before kuro's first write");
    assert_eq!(fs::read_dir(e.store.root.join("blobs")).unwrap().count(), 1, "one saved original");
    assert_eq!(entry.last_known, Disk::of(Some(b"edit three\n")));
}

#[test]
fn hard_undo_restores_the_original_exactly_even_for_binary_and_non_utf8() {
    let e = env();
    let original: Vec<u8> = (0..=255u8).chain([0xff, 0xfe, 0, 10, 13]).collect();
    let p = e.put("bin.dat", &original);
    e.save(&p, b"text now");
    let plan = e.store.plan(Some(&p));
    assert_eq!(plan.len(), 1);
    assert!(!plan[0].flagged() && !plan[0].will_delete);
    e.store.restore(&plan[0]).unwrap();
    assert_eq!(fs::read(&p).unwrap(), original);
    assert!(e.store.plan(Some(&p)).is_empty(), "nothing left to undo: the file is the original");
    let entry = e.store.entry_of(&p).unwrap();
    assert_eq!((entry.writes, entry.restores), (1, 1));
}

#[test]
fn after_a_restore_new_edits_are_undone_back_to_the_same_original() {
    let e = env();
    let p = e.put("a.txt", b"v0\n");
    e.save(&p, b"v1\n");
    e.store.restore(&e.store.plan(Some(&p))[0]).unwrap();
    e.save(&p, b"v2\n");
    let plan = e.store.plan(Some(&p));
    assert_eq!(plan.len(), 1);
    assert!(!plan[0].flagged(), "kuro wrote v2 itself: nothing changed behind its back");
    e.store.restore(&plan[0]).unwrap();
    assert_eq!(fs::read(&p).unwrap(), b"v0\n");
}

#[test]
fn a_file_kuro_created_is_deleted_by_hard_undo_and_a_missing_one_is_fine() {
    let e = env();
    let p = e.file("new/created.txt");
    e.save(&p, b"fresh\n");
    let entry = e.store.entry_of(&p).unwrap();
    assert!(!entry.existed && entry.baseline.is_none());
    let plan = e.store.plan(Some(&p));
    assert!(plan[0].will_delete);
    e.store.restore(&plan[0]).unwrap();
    assert!(!p.exists());
    assert!(e.store.plan(Some(&p)).is_empty());
    // restoring when the file is already gone is not an error
    e.save(&p, b"again\n");
    fs::remove_file(&p).unwrap();
    let plan = e.store.plan(Some(&p));
    assert_eq!(plan.len(), 0, "the baseline *is* \"missing\", and so is the disk: nothing to do");
}

#[test]
fn a_file_that_existed_and_is_now_missing_is_recreated_but_flagged() {
    let e = env();
    let p = e.put("a.txt", b"original\n");
    e.save(&p, b"edited\n");
    fs::remove_file(&p).unwrap();
    let plan = e.store.plan(Some(&p));
    assert_eq!(plan.len(), 1);
    assert_eq!(plan[0].disk, Disk::Missing);
    assert!(plan[0].changed_since && plan[0].flagged(), "it vanished after kuro's last write: ask first");
    e.store.restore(&plan[0]).unwrap();
    assert_eq!(fs::read(&p).unwrap(), b"original\n");
}

#[test]
fn a_change_made_outside_kuro_after_its_last_write_is_flagged() {
    let e = env();
    let p = e.put("a.txt", b"original\n");
    e.save(&p, b"edited in kuro\n");
    fs::write(&p, b"edited in another editor\n").unwrap();
    let plan = e.store.plan(Some(&p));
    assert!(plan[0].changed_since, "the disk is not what kuro last wrote");
    assert!(plan[0].flagged());
    // kuro never restores by itself: the file is untouched until restore() is called
    assert_eq!(fs::read(&p).unwrap(), b"edited in another editor\n");
}

#[test]
fn outside_edits_between_kuros_writes_are_part_of_the_same_history_and_the_baseline_stays() {
    let e = env();
    let p = e.put("a.txt", b"original\n");
    e.save(&p, b"kuro edit\n");
    fs::write(&p, b"other editor edit\n").unwrap(); // saved outside kuro
    e.save(&p, b"kuro edit 2\n"); // kuro's next save notices it first
    let entry = e.store.entry_of(&p).unwrap();
    assert_eq!((entry.writes, entry.externals), (2, 1));
    assert!(
        matches!(entry.lines[2], Line::External { .. }),
        "the outside change is logged before the write that follows it: {:?}",
        entry.lines
    );
    let plan = e.store.plan(Some(&p));
    assert!(plan[0].also_outside && plan[0].flagged(), "the popup says the file was also changed outside kuro");
    assert!(!plan[0].changed_since, "but the disk is what kuro last wrote");
    e.store.restore(&plan[0]).unwrap();
    assert_eq!(fs::read(&p).unwrap(), b"original\n", "back to the original, outside edits included");
}

#[test]
fn opening_a_file_that_changed_while_kuro_was_closed_records_it() {
    let e = env();
    let p = e.put("a.txt", b"original\n");
    e.save(&p, b"edited\n");
    e.store.note_open(&p, Some(b"edited\n"));
    assert_eq!(e.store.entry_of(&p).unwrap().externals, 0, "unchanged: nothing to note");
    fs::write(&p, b"git pull changed this\n").unwrap();
    e.store.note_open(&p, fs::read(&p).ok().as_deref());
    let entry = e.store.entry_of(&p).unwrap();
    assert_eq!(entry.externals, 1);
    e.store.note_open(&p, fs::read(&p).ok().as_deref());
    assert_eq!(e.store.entry_of(&p).unwrap().externals, 1, "noted once");
    // a file with no history is not tracked just because it was opened
    let q = e.put("untracked.txt", b"x");
    e.store.note_open(&q, Some(b"x"));
    assert!(e.store.entry_of(&q).is_none());
}

mod crash_safety {
    use super::*;

    #[test]
    fn a_torn_last_line_and_a_bad_checksum_are_ignored_and_earlier_lines_survive() {
        let e = env();
        let p = e.put("a.txt", b"original\n");
        e.save(&p, b"one\n");
        e.save(&p, b"two\n");
        let log = e.log_path(&p);
        let good = fs::read_to_string(&log).unwrap();
        assert_eq!(parse_log(&good).len(), 3, "baseline + two writes");
        // a torn append: half of a line, no newline
        fs::write(&log, format!("{good}{{\"t\":\"write\",\"befo")).unwrap();
        assert_eq!(parse_log(&fs::read_to_string(&log).unwrap()).len(), 3);
        assert_eq!(e.store.entry_of(&p).unwrap().writes, 2);
        // a flipped checksum character in the middle line
        let mut lines: Vec<String> = good.lines().map(String::from).collect();
        let last = lines[1].len() - 1;
        let flipped = if lines[1].ends_with('0') { '1' } else { '0' };
        lines[1].replace_range(last.., &flipped.to_string());
        fs::write(&log, lines.join("\n") + "\n").unwrap();
        let entry = e.store.entry_of(&p).unwrap();
        assert_eq!(entry.writes, 1, "the damaged line is skipped, the others kept");
        assert!(entry.baseline.is_some(), "the baseline is intact: hard undo still works");
        e.store.restore(&e.store.plan(Some(&p))[0]).unwrap();
        assert_eq!(fs::read(&p).unwrap(), b"original\n");
    }

    #[test]
    fn a_crash_after_the_saved_original_but_before_its_log_line_leaves_nothing_half_done() {
        let e = env();
        let p = e.put("a.txt", b"original\n");
        // simulate: the blob was written, the log line was not
        fs::create_dir_all(e.store.root.join("blobs")).unwrap();
        let hash = kuro::ext::seal::hex(&kuro::ext::seal::sha256(b"original\n"));
        fs::write(e.store.root.join("blobs").join(&hash), b"original\n").unwrap();
        assert!(e.store.entry_of(&p).is_none(), "no log line: the file is simply untracked");
        // the next real save records the baseline normally (the blob is reused, not duplicated)
        e.save(&p, b"edit\n");
        assert_eq!(fs::read_dir(e.store.root.join("blobs")).unwrap().count(), 1);
        e.store.restore(&e.store.plan(Some(&p))[0]).unwrap();
        assert_eq!(fs::read(&p).unwrap(), b"original\n");
    }

    #[test]
    fn a_damaged_saved_original_is_detected_and_the_file_is_left_alone() {
        let e = env();
        let p = e.put("a.txt", b"original\n");
        e.save(&p, b"edited\n");
        let entry = e.store.entry_of(&p).unwrap();
        fs::write(e.store.root.join("blobs").join(entry.baseline.unwrap()), b"tampered\n").unwrap();
        let err = e.store.restore(&e.store.plan(Some(&p))[0]).unwrap_err();
        assert!(err.contains("damaged"), "{err}");
        assert_eq!(fs::read(&p).unwrap(), b"edited\n", "never overwritten with something wrong");
        fs::remove_file(e.store.root.join("blobs").join(e.store.entry_of(&p).unwrap().baseline.unwrap())).unwrap();
        assert!(e.store.restore(&e.store.plan(Some(&p))[0]).unwrap_err().contains("missing"));
    }

    #[test]
    fn a_failed_restore_write_leaves_the_file_as_it_was() {
        let e = env();
        let p = e.put("a.txt", b"original\n");
        e.save(&p, b"edited\n");
        let plan = e.store.plan(Some(&p));
        // the folder becomes read-only: the atomic write fails before anything is replaced
        use std::os::unix::fs::PermissionsExt;
        let dir = p.parent().unwrap();
        fs::set_permissions(dir, fs::Permissions::from_mode(0o500)).unwrap();
        let r = e.store.restore(&plan[0]);
        fs::set_permissions(dir, fs::Permissions::from_mode(0o700)).unwrap();
        if r.is_ok() {
            return; // running as root: permissions do not apply
        }
        assert_eq!(fs::read(&p).unwrap(), b"edited\n");
        assert_eq!(e.store.entry_of(&p).unwrap().restores, 0, "and no restore was logged");
    }

    #[test]
    fn a_history_that_cannot_be_written_is_reported_and_never_blocks_the_save() {
        let dir = tempfile::tempdir().unwrap();
        // the history root is a file, so nothing can be created under it
        let blocker = dir.path().join("history");
        fs::write(&blocker, "x").unwrap();
        let store = Store::new(blocker, HistoryOptions::default());
        let p = dir.path().join("a.txt");
        fs::write(&p, "x").unwrap();
        let r = store.before_write(&p, Some(b"x"), 1);
        assert!(matches!(r, Err(Skip::Error(_))), "{r:?}");
        assert!(store.last_error.lock().unwrap().as_deref().unwrap().contains("hard undo cannot record"));
        // through write_observed the write itself still happens
        let obs: Arc<dyn WriteObserver> = Arc::new(Store::new(dir.path().join("history"), HistoryOptions::default()));
        write_observed(Some(&obs), &p, b"saved anyway").unwrap();
        assert_eq!(fs::read(&p).unwrap(), b"saved anyway");
    }
}

mod what_is_recorded {
    use super::*;

    #[test]
    fn excluded_disabled_and_too_large_files_are_not_recorded() {
        let e = env();
        for name in ["prod.env", ".env", ".env.local", "server.pem", "api.key", "id_rsa"] {
            let p = e.put(name, b"secret\n");
            assert_eq!(e.store.before_write(&p, Some(b"secret\n"), 6), Err(Skip::Excluded), "{name}");
            assert!(e.store.entry_of(&p).is_none());
        }
        let p = e.put("big.txt", &vec![b'x'; 6 << 20]);
        assert!(matches!(e.store.before_write(&p, Some(&fs::read(&p).unwrap()), 10), Err(Skip::TooLarge(_))));
        let small = e.put("small.txt", b"x");
        assert!(matches!(e.store.before_write(&small, Some(b"x"), 6 << 20), Err(Skip::TooLarge(_))), "a big new content counts too");
        let off = env_with(HistoryOptions { enabled: false, ..Default::default() });
        let q = off.put("a.txt", b"x");
        assert_eq!(off.store.before_write(&q, Some(b"x"), 1), Err(Skip::Disabled));
        assert!(!off.store.root.join("files").exists(), "nothing is written when it is off");
        // a custom exclude list replaces the default one
        let custom = env_with(HistoryOptions { exclude: vec!["*.secret".into()], ..Default::default() });
        let r = custom.put("x.env", b"1");
        assert!(custom.store.before_write(&r, Some(b"1"), 1).is_ok());
        let s = custom.put("x.secret", b"1");
        assert_eq!(custom.store.before_write(&s, Some(b"1"), 1), Err(Skip::Excluded));
    }

    #[test]
    fn identical_originals_share_one_saved_copy() {
        let e = env();
        let a = e.put("a.txt", b"same\n");
        let b = e.put("b.txt", b"same\n");
        e.save(&a, b"1\n");
        e.save(&b, b"2\n");
        assert_eq!(fs::read_dir(e.store.root.join("blobs")).unwrap().count(), 1);
        assert_eq!(e.store.entries().len(), 2);
    }

    #[test]
    fn the_store_is_private_to_the_user() {
        use std::os::unix::fs::PermissionsExt;
        let e = env();
        let p = e.put("a.txt", b"x");
        e.save(&p, b"y");
        for d in [e.store.root.clone(), e.store.root.join("files"), e.store.root.join("blobs")] {
            assert_eq!(fs::metadata(&d).unwrap().permissions().mode() & 0o077, 0, "{} is not private", d.display());
        }
    }

    #[test]
    fn files_are_told_apart_by_their_real_path() {
        let e = env();
        let a = e.put("dir1/same.txt", b"A\n");
        let b = e.put("dir2/same.txt", b"B\n");
        e.save(&a, b"A2\n");
        e.save(&b, b"B2\n");
        assert_eq!(e.store.entries().len(), 2);
        // a symbolic link to a file is the same file
        let link = e.file("link.txt");
        std::os::unix::fs::symlink(&a, &link).unwrap();
        assert_eq!(kuro::history::key_of(&link), kuro::history::key_of(&a));
    }
}

mod plans_and_management {
    use super::*;

    #[test]
    fn the_plan_lists_only_files_that_differ_from_their_original_and_counts_writes() {
        let e = env();
        let a = e.put("a.txt", b"a0\n");
        let b = e.put("b.txt", b"b0\n");
        let c = e.put("c.txt", b"c0\n");
        e.save(&a, b"a1\n");
        e.save(&a, b"a2\n");
        e.save(&b, b"b1\n");
        e.save(&c, b"c1\n");
        e.save(&c, b"c0\n"); // back to the original by hand
        let plan = e.store.plan(None);
        let names: Vec<(String, usize)> =
            plan.iter().map(|i| (Path::new(&i.entry.path).file_name().unwrap().to_string_lossy().into_owned(), i.entry.writes)).collect();
        assert_eq!(names, vec![("a.txt".into(), 2), ("b.txt".into(), 1)], "c.txt is already the original");
    }

    #[test]
    fn deleting_one_or_several_histories_and_only_unreferenced_originals_go() {
        let e = env();
        let a = e.put("a.txt", b"shared\n");
        let b = e.put("b.txt", b"shared\n");
        let c = e.put("c.txt", b"unique\n");
        for p in [&a, &b, &c] {
            e.save(p, b"edited\n");
        }
        assert_eq!(fs::read_dir(e.store.root.join("blobs")).unwrap().count(), 2);
        let (ka, kc) = (kuro::history::key_of(&a), kuro::history::key_of(&c));
        let removed = e.store.delete(&[ka[..8].to_string()]).unwrap();
        assert_eq!(removed, vec![ka.clone()], "a prefix is enough");
        assert_eq!(e.store.entries().len(), 2);
        assert_eq!(fs::read_dir(e.store.root.join("blobs")).unwrap().count(), 2, "the shared original is still used by b");
        e.store.delete(&[kuro::history::key_of(&b), kc]).unwrap();
        assert!(e.store.entries().is_empty());
        assert_eq!(fs::read_dir(e.store.root.join("blobs")).unwrap().count(), 0, "nothing refers to the originals any more");
        assert!(e.store.delete(&["zzzz".into()]).unwrap_err().contains("no history"));
        // the files themselves were never touched by management
        assert_eq!(fs::read(&a).unwrap(), b"edited\n");
    }

    #[test]
    fn an_ambiguous_prefix_deletes_nothing() {
        let e = env();
        let mut keys = vec![];
        for i in 0..40 {
            let p = e.put(&format!("f{i}.txt"), b"x");
            e.save(&p, b"y");
            keys.push(kuro::history::key_of(&p));
        }
        // 40 keys and only 16 possible first characters: some character is shared (pigeonhole), whatever the random keys are
        let shared = "0123456789abcdef"
            .chars()
            .map(|c| c.to_string())
            .find(|c| keys.iter().filter(|k| k.starts_with(c.as_str())).count() > 1)
            .unwrap();
        assert!(e.store.delete(&[shared]).unwrap_err().contains("several"));
        assert_eq!(e.store.entries().len(), 40);
    }

    #[test]
    fn clear_removes_everything_and_the_size_is_reported() {
        let e = env();
        let a = e.put("a.txt", &vec![b'a'; 4000]);
        e.save(&a, b"x");
        assert!(e.store.size_bytes() >= 4000);
        assert_eq!(e.store.clear().unwrap(), 1);
        assert!(e.store.entries().is_empty());
        assert!(e.store.size_bytes() < 100);
        // and the history starts over for the same file
        e.save(&a, b"y");
        assert_eq!(e.store.entry_of(&a).unwrap().writes, 1);
    }

    #[test]
    fn nothing_is_ever_deleted_automatically() {
        let e = env();
        let mut paths = vec![];
        for i in 0..60 {
            let p = e.put(&format!("f{i}.txt"), format!("orig {i}\n").as_bytes());
            e.save(&p, b"edit\n");
            paths.push(p);
        }
        assert_eq!(e.store.entries().len(), 60, "no retention limit trims the history");
        // even when the store is far past the warning size
        let warn_low = Store::new(e.store.root.clone(), HistoryOptions { warn_bytes: 1, ..Default::default() });
        assert!(warn_low.size_bytes() > warn_low.opts.warn_bytes);
        assert_eq!(warn_low.entries().len(), 60);
        for p in &paths {
            assert!(warn_low.entry_of(p).unwrap().baseline.is_some());
        }
    }
}

mod through_the_real_write_paths {
    use super::*;

    #[test]
    fn write_observed_records_before_it_replaces_and_keeps_permissions() {
        use std::os::unix::fs::PermissionsExt;
        let e = env();
        let p = e.put("run.sh", b"#!/bin/sh\necho one\n");
        fs::set_permissions(&p, fs::Permissions::from_mode(0o755)).unwrap();
        let obs: Arc<dyn WriteObserver> = Arc::new(Store::new(e.store.root.clone(), HistoryOptions::default()));
        write_observed(Some(&obs), &p, b"#!/bin/sh\necho two\n").unwrap();
        assert_eq!(fs::read(&p).unwrap(), b"#!/bin/sh\necho two\n");
        assert_eq!(fs::metadata(&p).unwrap().permissions().mode() & 0o777, 0o755);
        let entry = e.store.entry_of(&p).unwrap();
        assert_eq!(entry.writes, 1);
        e.store.restore(&e.store.plan(Some(&p))[0]).unwrap();
        assert_eq!(fs::read(&p).unwrap(), b"#!/bin/sh\necho one\n");
        assert_eq!(fs::metadata(&p).unwrap().permissions().mode() & 0o777, 0o755, "a restore keeps the permissions too");
        // and without an observer it is just an atomic write
        write_observed(None, &p, b"plain").unwrap();
        assert_eq!(fs::read(&p).unwrap(), b"plain");
    }

    #[test]
    fn the_live_sync_file_system_reports_its_saves() {
        let e = env();
        let p = e.put("shared.txt", b"before sync\n");
        let obs: Arc<dyn WriteObserver> = Arc::new(Store::new(e.store.root.clone(), HistoryOptions::default()));
        let mut fs_ = ObservedFs { observer: obs };
        fs_.write_atomic(&p, b"saved by the sync leader\n").unwrap();
        let entry = e.store.entry_of(&p).unwrap();
        assert_eq!(entry.writes, 1);
        assert_eq!(fs::read(&p).unwrap(), b"saved by the sync leader\n");
        e.store.restore(&e.store.plan(Some(&p))[0]).unwrap();
        assert_eq!(fs::read(&p).unwrap(), b"before sync\n");
    }

    #[test]
    fn recording_from_many_threads_and_processes_at_once_keeps_every_line() {
        let e = env();
        let p = e.put("busy.txt", b"v0\n");
        let obs: Arc<dyn WriteObserver> = Arc::new(Store::new(e.store.root.clone(), HistoryOptions::default()));
        let mut handles = vec![];
        for t in 0..6 {
            let (obs, p) = (obs.clone(), p.clone());
            handles.push(std::thread::spawn(move || {
                for i in 0..10 {
                    // kuro's own atomic write, reported to the shared history (several editors saving one file)
                    write_observed(Some(&obs), &p, format!("t{t} i{i}\n").as_bytes()).unwrap();
                }
            }));
        }
        for h in handles {
            h.join().unwrap();
        }
        let entry = e.store.entry_of(&p).unwrap();
        assert_eq!(entry.writes, 60, "every write is a line");
        assert_eq!(entry.baseline_size, 3, "and the baseline is still the original");
        e.store.restore(&e.store.plan(Some(&p))[0]).unwrap();
        assert_eq!(fs::read(&p).unwrap(), b"v0\n");
        let log = fs::read_to_string(e.log_path(&p)).unwrap();
        for l in log.lines() {
            let (json, crc) = l.rsplit_once('\t').unwrap();
            assert_eq!(u32::from_str_radix(crc, 16).unwrap(), crc32(json.as_bytes()), "no interleaved or torn line");
        }
    }
}

#[test]
fn crc32_matches_the_standard_check_value() {
    assert_eq!(crc32(b"123456789"), 0xCBF4_3926);
    assert_eq!(crc32(b""), 0);
}
