//! The git layer of `kuro git`, against throw-away repositories created here (never the user's repositories, never the user's git config).

mod common;
use common::gitrepo::*;
use kuro::gitview::cmd::{Change, Git, GitError, Head, toplevel};
use std::path::PathBuf;

#[test]
fn outside_a_repository_is_reported_not_a_repo() {
    let d = tempfile::tempdir().unwrap();
    let env = isolated_env(d.path());
    // an empty directory, a subdirectory, and a directory that does not exist
    assert_eq!(toplevel(d.path(), &env), Err(GitError::NotRepo));
    std::fs::create_dir(d.path().join("sub")).unwrap();
    assert_eq!(toplevel(&d.path().join("sub"), &env), Err(GitError::NotRepo));
    assert_eq!(toplevel(&d.path().join("missing"), &env), Err(GitError::NotRepo));
    assert_eq!(GitError::NotRepo.to_string(), "this directory is not in a git repository", "the exact message");
    assert!(Git::open(d.path(), env).is_err());
}

#[test]
fn inside_a_subdirectory_the_top_of_the_repository_is_found() {
    let mut r = Repo::new();
    r.write("deep/er/file.txt", "x\n");
    r.commit("c");
    let sub = r.path().join("deep/er");
    let g = Git::open(&sub, isolated_env(r.path())).unwrap();
    assert_eq!(g.top.canonicalize().unwrap(), r.path().canonicalize().unwrap());
}

#[test]
fn a_repository_with_no_commits_is_handled() {
    let r = Repo::new();
    let g = r.open();
    assert_eq!(g.head().unwrap(), Head::Unborn("main".into()));
    assert!(!g.has_commits());
    assert!(g.tree().unwrap().is_empty());
    assert!(g.log(None, false, 0, 10).unwrap().is_empty());
    assert!(g.log(Some("a.txt"), true, 0, 10).unwrap().is_empty());
    assert!(g.file_history("a.txt", 10).unwrap().is_empty());
    assert!(!g.has_history("a.txt"));
}

#[test]
fn the_log_has_author_date_subject_parents_and_pages() {
    let mut r = Repo::new();
    r.write("a.txt", "1\n");
    let h1 = r.commit("first");
    r.write("a.txt", "2\n");
    let h2 = r.commit("second: with a colon");
    r.write("a.txt", "3\n");
    let h3 = r.commit("third");
    let g = r.open();
    let log = g.log(None, false, 0, 10).unwrap();
    assert_eq!(log.iter().map(|c| c.subject.as_str()).collect::<Vec<_>>(), vec!["third", "second: with a colon", "first"], "newest first");
    assert_eq!((log[0].hash.as_str(), log[0].parents.clone()), (h3.as_str(), vec![h2.clone()]));
    assert!(log[2].parents.is_empty(), "the root commit");
    assert_eq!(
        (log[0].author.as_str(), log[0].email.as_str(), log[0].date.as_str()),
        ("Test User", "test@example.invalid", "2026-01-03 10:00")
    );
    assert!(h1.starts_with(&log[2].short) && log[2].short.len() >= 7);
    // pages
    let page = g.log(None, false, 1, 1).unwrap();
    assert_eq!(page.len(), 1);
    assert_eq!(page[0].hash, h2);
    assert!(g.log(None, false, 3, 10).unwrap().is_empty());
    // one file
    r.write("b.txt", "other\n");
    r.commit("fourth: b only");
    let g = r.open();
    assert_eq!(g.log(Some("a.txt"), false, 0, 10).unwrap().len(), 3, "the commit that only touched b.txt is not in a.txt's history");
    assert_eq!(g.log(Some("b.txt"), false, 0, 10).unwrap().len(), 1);
}

#[test]
fn a_detached_head_is_reported_with_the_short_hash() {
    let mut r = Repo::new();
    r.write("a.txt", "1\n");
    let h1 = r.commit("first");
    r.write("a.txt", "2\n");
    r.commit("second");
    r.git(&["checkout", "-q", "--detach", &h1]);
    let g = r.open();
    match g.head().unwrap() {
        Head::Detached(s) => assert!(h1.starts_with(&s), "{s}"),
        other => panic!("{other:?}"),
    }
    let log = g.log(None, false, 0, 10).unwrap();
    assert_eq!(log.len(), 1, "the log is of what HEAD reaches");
    assert_eq!(g.head().unwrap().label(), format!("HEAD detached at {}", &h1[..7]));
    let mut r2 = Repo::new();
    r2.write("a", "x");
    r2.commit("c");
    assert_eq!(r2.open().head().unwrap(), Head::Branch("main".into()));
}

#[test]
fn file_history_follows_a_rename() {
    let mut r = Repo::new();
    let body = lines(40, "original");
    r.write("old_name.txt", &body);
    r.commit("add old_name");
    r.write("old_name.txt", &format!("{body}one more line\n"));
    r.commit("edit old_name");
    r.git(&["mv", "old_name.txt", "new_name.txt"]);
    r.commit("rename it");
    r.write("new_name.txt", &format!("{body}one more line\nand another\n"));
    r.commit("edit new_name");
    let g = r.open();
    let plain = g.log(Some("new_name.txt"), false, 0, 20).unwrap();
    assert_eq!(plain.len(), 2, "without --follow history stops at the rename");
    let followed = g.log(Some("new_name.txt"), true, 0, 20).unwrap();
    assert_eq!(
        followed.iter().map(|c| c.subject.as_str()).collect::<Vec<_>>(),
        vec!["edit new_name", "rename it", "edit old_name", "add old_name"]
    );
    let ch = g.file_history("new_name.txt", 20).unwrap();
    assert_eq!(ch.len(), 4);
    assert_eq!(
        ch.iter().map(|(c, _)| c.subject.as_str()).collect::<Vec<_>>(),
        followed.iter().map(|c| c.subject.as_str()).collect::<Vec<_>>(),
        "the single walk gives the same commits as the separate log"
    );
    // a longer read starts from the newest commit again: git finds nothing when it follows a rename across skipped commits
    assert_eq!(g.file_history("new_name.txt", 2).unwrap().len(), 2);
    assert_eq!(g.file_history("new_name.txt", 3).unwrap().len(), 3);
    assert!(g.log(Some("new_name.txt"), true, 2, 20).unwrap().is_empty(), "git cannot skip with --follow");
    let rename = ch[1].1.as_ref().unwrap();
    assert_eq!((rename.status, rename.old_path.as_deref(), rename.path.as_str()), ('R', Some("old_name.txt"), "new_name.txt"));
    assert_eq!(ch[3].1.as_ref().unwrap().path, "old_name.txt", "before the rename the file had its old name");
    // the file as it was at an old commit, under its old name
    let old = followed.iter().find(|c| c.subject == "add old_name").unwrap();
    assert_eq!(g.file_at(old, "old_name.txt").unwrap(), body.as_bytes());
    assert!(g.file_at(old, "new_name.txt").is_err());
}

#[test]
fn a_commit_lists_added_removed_renamed_and_modified_files() {
    let mut r = Repo::new();
    r.write("keep.txt", &lines(30, "keep"));
    r.write("gone.txt", &lines(30, "gone"));
    r.write("moved.txt", &lines(60, "moved"));
    r.commit("base");
    r.write("keep.txt", &format!("{}extra\n", lines(30, "keep")));
    r.write("added.txt", "brand new\n");
    std::fs::remove_file(r.path().join("gone.txt")).unwrap();
    std::fs::create_dir_all(r.path().join("sub dir")).unwrap();
    r.git(&["mv", "moved.txt", "sub dir/moved here.txt"]);
    r.commit("many changes");
    let g = r.open();
    let c = &g.log(None, false, 0, 1).unwrap()[0];
    let mut ch = g.changes(c).unwrap();
    ch.sort_by(|a, b| a.path.cmp(&b.path));
    let summary: Vec<String> = ch.iter().map(|c| c.label()).collect();
    assert_eq!(summary, vec!["A added.txt", "D gone.txt", "M keep.txt", "R moved.txt → sub dir/moved here.txt"], "{summary:?}");
    // the whole diff
    let d = g.diff(c, &[]).unwrap();
    assert!(d.contains("+brand new") && d.contains("-line 0 of the gone file") && d.contains("+extra"), "{d}");
    assert!(d.contains("rename from moved.txt") && d.contains("rename to sub dir/moved here.txt"), "{d}");
    // one file of it: both names for a rename
    let one = g.diff(c, &["keep.txt"]).unwrap();
    assert!(one.contains("+extra") && !one.contains("brand new"));
    let rn: &Change = ch.iter().find(|c| c.status == 'R').unwrap();
    let rd = g.diff(c, &[rn.old_path.as_deref().unwrap(), rn.path.as_str()]).unwrap();
    assert!(rd.contains("rename from moved.txt"), "{rd}");
}

#[test]
fn the_root_commit_is_all_additions() {
    let mut r = Repo::new();
    r.write("a.txt", "hello\n");
    r.write("b/c.txt", "world\n");
    r.commit("root");
    let g = r.open();
    let c = &g.log(None, false, 0, 1).unwrap()[0];
    let ch = g.changes(c).unwrap();
    assert_eq!(ch.len(), 2);
    assert!(ch.iter().all(|c| c.status == 'A'));
    assert!(g.diff(c, &[]).unwrap().contains("+hello"));
    assert_eq!(g.tree().unwrap(), vec!["a.txt", "b/c.txt"]);
    assert_eq!(g.commit_message(c).unwrap(), "root");
}

#[test]
fn a_merge_is_shown_against_its_first_parent() {
    let mut r = Repo::new();
    r.write("a.txt", &lines(20, "a"));
    r.commit("base");
    r.git(&["checkout", "-q", "-b", "side"]);
    r.write("side.txt", "from the side\n");
    r.commit("side work");
    r.git(&["checkout", "-q", "main"]);
    r.write("main.txt", "from main\n");
    r.commit("main work");
    r.git(&["merge", "-q", "--no-ff", "-m", "merge side", "side"]);
    let g = r.open();
    let log = g.log(None, false, 0, 10).unwrap();
    let merge = &log[0];
    assert_eq!(merge.subject, "merge side");
    assert_eq!(merge.parents.len(), 2);
    let ch = g.changes(merge).unwrap();
    assert_eq!(ch.iter().map(|c| c.label()).collect::<Vec<_>>(), vec!["A side.txt"], "what the merge brought into main");
}

#[test]
fn awkward_names_and_binary_files() {
    let mut r = Repo::new();
    r.write("with space.txt", "x\n");
    r.write("ünïcode/файл.txt", "y\n");
    r.write("tab\there.txt", "z\n");
    std::fs::write(r.path().join("bin.dat"), [0u8, 1, 2, 255, 0, 7]).unwrap();
    r.commit("odd names");
    let g = r.open();
    let tree = g.tree().unwrap();
    for n in ["with space.txt", "ünïcode/файл.txt", "bin.dat"] {
        assert!(tree.contains(&n.to_string()), "{n} in {tree:?}");
    }
    let c = &g.log(None, false, 0, 1).unwrap()[0];
    assert!(g.diff(c, &["bin.dat"]).unwrap().contains("Binary files"));
    assert!(g.diff(c, &["ünïcode/файл.txt"]).unwrap().contains("+y"));
    assert!(g.has_history("with space.txt"));
    assert!(!g.has_history("no such file"));
    assert_eq!(g.file_at(c, "bin.dat").unwrap(), vec![0u8, 1, 2, 255, 0, 7]);
}

#[test]
fn a_repository_cannot_make_kuro_run_its_configured_programs() {
    let mut r = Repo::new();
    let marker: PathBuf = r.path().join("..").join(format!("ran-{}", r.path().file_name().unwrap().to_string_lossy()));
    let script = r.path().join("evil.sh");
    std::fs::write(&script, format!("#!/bin/sh\ntouch '{}'\ncat \"$1\"\n", marker.display())).unwrap();
    std::fs::set_permissions(&script, std::os::unix::fs::PermissionsExt::from_mode(0o755)).unwrap();
    r.git(&["config", "diff.external", script.to_str().unwrap()]);
    r.git(&["config", "diff.evil.textconv", script.to_str().unwrap()]);
    r.write(".gitattributes", "*.txt diff=evil\n");
    r.write("a.txt", "one\n");
    r.commit("c1");
    r.write("a.txt", "two\n");
    r.commit("c2");
    let g = r.open();
    let c = &g.log(None, false, 0, 1).unwrap()[0];
    let d = g.diff(c, &[]).unwrap();
    assert!(d.contains("-one") && d.contains("+two"), "a normal diff, not the program's output: {d}");
    assert!(g.changes(c).is_ok());
    assert!(!marker.exists(), "the configured diff.external / textconv program ran");
}
