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

// ----------------------------------------------------------------------------------------------- file history: kuro's way against `git log --follow`

/// What git's own `--follow` says for `path`: subjects and, per commit, the path the file had.
fn follow_reference(r: &Repo, path: &str) -> Vec<String> {
    r.git(&["log", "--follow", "--format=%s", "--", path]).lines().map(String::from).collect()
}

fn ours(g: &kuro::gitview::cmd::Git, path: &str) -> Vec<String> {
    g.file_history(path, 1000).unwrap().into_iter().map(|(c, _)| c.subject).collect()
}

#[test]
fn the_file_history_equals_git_follow_for_renames_chains_readds_and_edits() {
    let mut r = Repo::new();
    let body = lines(60, "chain");
    r.write("a.txt", &body);
    r.commit("add a");
    r.write("a.txt", &format!("{body}more\n"));
    r.commit("edit a");
    r.git(&["mv", "a.txt", "b.txt"]);
    r.commit("rename a to b");
    r.write("b.txt", &format!("{body}more\nmore still\n"));
    r.commit("edit b");
    r.git(&["mv", "b.txt", "dir_c.txt"]);
    r.commit("rename b to c");
    // an unrelated file with the same start, and the old name coming back as a new file
    r.write("a.txt", "something else entirely\n");
    r.commit("a.txt is a new file now");
    r.write("dir_c.txt", &format!("{body}more\nmore still\nand more\n"));
    r.commit("edit c");
    // a rename with a change in the same commit
    r.git(&["mv", "dir_c.txt", "final.txt"]);
    r.write("final.txt", &format!("{body}more\nmore still\nand more\nplus\n"));
    r.commit("rename c to final and edit");
    let g = r.open();
    for path in ["final.txt", "dir_c.txt", "b.txt", "a.txt"] {
        assert_eq!(ours(&g, path), follow_reference(&r, path), "history of {path}");
    }
    let all = g.file_history("final.txt", 100).unwrap();
    let renames: Vec<_> = all
        .iter()
        .filter_map(|(_, c)| c.as_ref())
        .filter(|c| c.status == 'R')
        .map(|c| (c.old_path.clone().unwrap(), c.path.clone()))
        .collect();
    // the path in a rename entry is the name the file had in the history being followed at that moment
    assert_eq!(renames.len(), 3, "{renames:?}");
    assert_eq!(renames[0].0, "dir_c.txt");
}

#[test]
fn a_deleted_and_recreated_file_keeps_both_lives_like_git_follow() {
    let mut r = Repo::new();
    r.write("x.txt", &lines(30, "x"));
    r.commit("add x");
    r.git(&["rm", "-q", "x.txt"]);
    r.commit("delete x");
    r.write("x.txt", &lines(30, "reborn"));
    r.commit("add x again");
    let g = r.open();
    assert_eq!(ours(&g, "x.txt"), follow_reference(&r, "x.txt"));
    assert_eq!(ours(&g, "x.txt").len(), 3);
}

#[test]
fn a_file_that_never_existed_has_an_empty_history_and_the_reading_ends() {
    let mut r = Repo::new();
    r.write("x.txt", "1\n");
    r.commit("x");
    assert!(r.open().file_history("nope.txt", 10).unwrap().is_empty());
}

#[test]
fn a_rename_on_a_merged_branch_is_followed() {
    let mut r = Repo::new();
    let body = lines(50, "merge");
    r.write("m.txt", &body);
    r.commit("add m");
    r.git(&["checkout", "-q", "-b", "side"]);
    r.git(&["mv", "m.txt", "m2.txt"]);
    r.commit("rename on the side branch");
    r.git(&["checkout", "-q", "main"]);
    r.write("other.txt", "o\n");
    r.commit("main moves on");
    r.git(&["merge", "-q", "--no-ff", "-m", "merge side", "side"]);
    let g = r.open();
    assert_eq!(ours(&g, "m2.txt"), follow_reference(&r, "m2.txt"));
}

// ----------------------------------------------------------------------------------------------- the commit-graph cache

fn objects_listing(r: &Repo) -> Vec<String> {
    let mut v: Vec<String> = walk(&r.path().join(".git/objects"));
    v.sort();
    v
}

fn walk(p: &std::path::Path) -> Vec<String> {
    let mut out = vec![];
    if let Ok(rd) = std::fs::read_dir(p) {
        for e in rd.flatten() {
            let path = e.path();
            out.push(path.display().to_string());
            if path.is_dir() {
                out.extend(walk(&path));
            }
        }
    }
    out
}

fn wait_for_graph(g: &kuro::gitview::cmd::Git) {
    let dir = g.graph_dir().unwrap();
    let start = std::time::Instant::now();
    while (!dir.join("info/commit-graph").is_file() || dir.join("building").exists()) && start.elapsed().as_secs() < 30 {
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
}

#[test]
fn a_big_enough_repository_gets_a_graph_in_the_cache_and_the_repository_is_not_touched() {
    let mut r = Repo::new();
    r.many_commits(40);
    let cache = tempfile::tempdir().unwrap();
    let before = objects_listing(&r);
    let mut g = r.open();
    g.cache_root = Some(cache.path().to_path_buf());
    g.cache_min_commits = 10;
    assert!(g.graph_env().is_empty(), "no graph yet: git is used as it is");
    let plain = ours(&g, "count.txt");
    g.ensure_graph_in_background();
    wait_for_graph(&g);
    let dir = g.graph_dir().unwrap();
    assert!(dir.join("info/commit-graph").is_file(), "built in the cache folder");
    assert!(!dir.join("building").exists(), "the lock is gone");
    assert_eq!(objects_listing(&r), before, "nothing was added to the repository");
    let env = g.graph_env();
    assert_eq!(env.iter().map(|(k, _)| k.as_str()).collect::<Vec<_>>(), ["GIT_OBJECT_DIRECTORY", "GIT_ALTERNATE_OBJECT_DIRECTORIES"]);
    assert_eq!(ours(&g, "count.txt"), plain, "the same history with the cache");
    assert_eq!(plain.len(), 40);
}

#[test]
fn small_repositories_and_a_switched_off_cache_build_nothing() {
    let mut r = Repo::new();
    r.many_commits(5);
    let cache = tempfile::tempdir().unwrap();
    let mut g = r.open();
    g.cache_root = Some(cache.path().to_path_buf());
    g.cache_min_commits = 100;
    g.ensure_graph_in_background();
    std::thread::sleep(std::time::Duration::from_millis(300));
    assert!(!g.graph_dir().unwrap().exists(), "too small: no folder at all");
    let mut off = r.open();
    off.cache_min_commits = 1;
    off.ensure_graph_in_background(); // no cache root: nothing happens
    assert!(off.graph_dir().is_none() && off.graph_env().is_empty());
}

#[test]
fn a_second_build_is_not_started_while_one_runs_and_a_fresh_cache_is_left_alone() {
    let mut r = Repo::new();
    r.many_commits(30);
    let cache = tempfile::tempdir().unwrap();
    let mut g = r.open();
    g.cache_root = Some(cache.path().to_path_buf());
    g.cache_min_commits = 10;
    let dir = g.graph_dir().unwrap();
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("building"), "").unwrap(); // another kuro is building
    g.ensure_graph_in_background();
    std::thread::sleep(std::time::Duration::from_millis(300));
    assert!(!dir.join("info/commit-graph").exists(), "the lock is respected");
    std::fs::remove_file(dir.join("building")).unwrap();
    g.ensure_graph_in_background();
    wait_for_graph(&g);
    let built = std::fs::metadata(dir.join("info/commit-graph")).unwrap().modified().unwrap();
    g.ensure_graph_in_background(); // fresh (less than a day old): untouched
    std::thread::sleep(std::time::Duration::from_millis(300));
    assert_eq!(std::fs::metadata(dir.join("info/commit-graph")).unwrap().modified().unwrap(), built);
}

// ----------------------------------------------------------------------------------------------- the working tree: status and discard

fn paths(g: &kuro::gitview::cmd::Git) -> Vec<String> {
    g.status().unwrap().into_iter().map(|c| format!("{} {}", c.letter(), c.path)).collect()
}

fn base_repo() -> Repo {
    let mut r = Repo::new();
    r.write("a.txt", "one\ntwo\nthree\n");
    r.write("keep.txt", "keep\n");
    r.write("dir/b.txt", "bee\n");
    r.commit("base");
    r
}

#[test]
fn status_lists_modified_deleted_added_and_untracked_files_staged_or_not() {
    let r = base_repo();
    r.write("a.txt", "one\nTWO\nthree\n"); // not staged
    r.write("dir/b.txt", "bee!\n");
    r.git(&["add", "dir/b.txt"]); // staged
    std::fs::remove_file(r.path().join("keep.txt")).unwrap();
    r.write("new file.txt", "x\n"); // untracked, with a space
    r.write("added.txt", "y\n");
    r.git(&["add", "added.txt"]);
    r.write("ignored.log", "z\n");
    r.write(".gitignore", "*.log\n");
    let g = r.open();
    assert_eq!(
        paths(&g),
        [".gitignore", "a.txt", "added.txt", "dir/b.txt", "keep.txt", "new file.txt"].map(|p| {
            let letter = match p {
                ".gitignore" | "new file.txt" => 'U',
                "added.txt" => 'A',
                "keep.txt" => 'D',
                _ => 'M',
            };
            format!("{letter} {p}")
        })
    );
}

#[test]
fn a_clean_repository_and_one_without_commits_are_handled() {
    let r = base_repo();
    assert!(r.open().status().unwrap().is_empty());
    let empty = Repo::new();
    empty.write("first.txt", "1\n");
    assert_eq!(paths(&empty.open()), ["U first.txt"]);
}

#[test]
fn the_diff_of_a_change_is_against_head_and_a_new_file_is_shown_as_all_added() {
    let r = base_repo();
    r.write("a.txt", "one\nTWO\nthree\n");
    r.git(&["add", "a.txt"]);
    r.write("a.txt", "one\nTWO\nTHREE\n"); // staged and then changed again: the diff is HEAD against the working tree
    r.write("fresh.txt", "line1\nline2");
    r.write("bin.dat", "a\0b");
    let g = r.open();
    let ch = g.status().unwrap();
    let get = |p: &str| ch.iter().find(|c| c.path == p).unwrap().clone();
    let d = g.worktree_diff(&get("a.txt")).unwrap();
    assert!(d.contains("-two") && d.contains("+TWO") && d.contains("-three") && d.contains("+THREE"), "{d}");
    let d = g.worktree_diff(&get("fresh.txt")).unwrap();
    assert!(
        d.contains("new file mode")
            && d.contains("@@ -0,0 +1,2 @@")
            && d.contains("+line1\n+line2\n")
            && d.contains("No newline at end of file"),
        "{d}"
    );
    assert!(g.worktree_diff(&get("bin.dat")).unwrap().contains("Binary files"));
}

#[test]
fn discard_restores_tracked_files_and_removes_added_and_untracked_ones_and_nothing_else() {
    let r = base_repo();
    r.write("a.txt", "CHANGED\n");
    r.write("dir/b.txt", "staged change\n");
    r.git(&["add", "dir/b.txt"]);
    std::fs::remove_file(r.path().join("keep.txt")).unwrap();
    r.write("added.txt", "y\n");
    r.git(&["add", "added.txt"]);
    r.write("untracked.txt", "u\n");
    r.write("other/untracked2.txt", "u2\n");
    let g = r.open();
    let all = g.status().unwrap();
    assert_eq!(all.len(), 6);
    // only some are discarded: the others stay exactly as they are
    let some: Vec<_> = all.iter().filter(|c| ["a.txt", "added.txt", "untracked.txt"].contains(&c.path.as_str())).cloned().collect();
    assert_eq!(g.discard(&some).unwrap(), 3);
    assert_eq!(std::fs::read_to_string(r.path().join("a.txt")).unwrap(), "one\ntwo\nthree\n");
    assert!(!r.path().join("added.txt").exists() && !r.path().join("untracked.txt").exists());
    assert_eq!(r.git(&["ls-files", "added.txt"]), "", "removed from the index too");
    assert_eq!(std::fs::read_to_string(r.path().join("dir/b.txt")).unwrap(), "staged change\n", "not chosen: untouched");
    assert!(r.path().join("other/untracked2.txt").exists());
    assert!(!r.path().join("keep.txt").exists());
    // the rest
    let rest = g.status().unwrap();
    assert_eq!(rest.len(), 3);
    assert_eq!(g.discard(&rest).unwrap(), 3);
    assert!(g.status().unwrap().is_empty(), "everything is gone: {:?}", g.status().unwrap());
    assert_eq!(std::fs::read_to_string(r.path().join("dir/b.txt")).unwrap(), "bee\n");
    assert_eq!(std::fs::read_to_string(r.path().join("keep.txt")).unwrap(), "keep\n", "the deleted file is back");
    assert!(!r.path().join("other/untracked2.txt").exists());
    assert_eq!(r.git(&["log", "--format=%s"]).trim(), "base", "history untouched");
}

#[test]
fn discard_is_safe_with_odd_names_and_never_touches_conflicts_or_ignored_files() {
    let mut r = base_repo();
    r.write("-rf", "dash\n");
    r.write("sp ace/caf\u{e9} \u{540d}.txt", "x\n");
    r.write("ignored.log", "keep me\n");
    r.write(".gitignore", "*.log\n");
    r.commit("ignore");
    r.write("-rf", "changed\n");
    r.write("-new", "n\n");
    r.write("sp ace/caf\u{e9} \u{540d}.txt", "changed\n");
    let g = r.open();
    let all = g.status().unwrap();
    assert_eq!(all.len(), 3);
    g.discard(&all).unwrap();
    assert_eq!(std::fs::read_to_string(r.path().join("-rf")).unwrap(), "dash\n");
    assert!(!r.path().join("-new").exists());
    assert_eq!(std::fs::read_to_string(r.path().join("sp ace/caf\u{e9} \u{540d}.txt")).unwrap(), "x\n");
    assert_eq!(std::fs::read_to_string(r.path().join("ignored.log")).unwrap(), "keep me\n", "ignored files are not part of it");
    // a conflict is reported and left alone
    r.git(&["checkout", "-q", "-b", "other"]);
    r.write("a.txt", "other side\n");
    r.commit("other");
    r.git(&["checkout", "-q", "main"]);
    r.write("a.txt", "main side\n");
    r.commit("main");
    let merge =
        std::process::Command::new("git").current_dir(r.path()).args(["merge", "other"]).envs(isolated_env(r.path())).output().unwrap();
    assert!(!merge.status.success());
    let ch = g.status().unwrap();
    let conflict = ch.iter().find(|c| c.path == "a.txt").unwrap();
    assert_eq!(conflict.kind(), kuro::gitview::cmd::WtKind::Conflict);
    assert_eq!(g.discard(&ch).unwrap(), 0, "nothing was discarded: a conflict is left to the user");
    assert!(std::fs::read_to_string(r.path().join("a.txt")).unwrap().contains("<<<<<<<"));
}
