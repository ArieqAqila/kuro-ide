//! `kuro git` as the real binary in a pseudo-terminal, on a throw-away repository.

mod common;
use common::gitrepo::*;
use common::pty::*;

const CFG: &str = "[clipboard]\nbackend = \"osc52\"\n";
const ENV: [(&str, &str); 2] = [("GIT_CONFIG_GLOBAL", "/dev/null"), ("GIT_CONFIG_NOSYSTEM", "1")];

fn repo() -> Repo {
    let mut r = Repo::new();
    r.write("README.md", "# demo\n");
    let mut main = String::from("fn main() {\n");
    for i in 0..30 {
        main.push_str(&format!("    println!(\"line {i}\");\n"));
    }
    main.push_str("}\n");
    r.write("src/main.rs", &main);
    r.commit("Initial import");
    r.write("src/main.rs", &main.replace("line 3\"", "line three\""));
    r.write("notes.txt", "one note\n");
    r.commit("Edit main, add notes");
    r
}

fn screen_has(s: &[String], needle: &str) -> bool {
    s.iter().any(|r| r.contains(needle))
}

fn spawn(dir: &std::path::Path, r: &Repo, args: &[&str]) -> Pty {
    Pty::spawn_args(dir, CFG, args, r.path(), &ENV)
}

#[test]
fn browse_from_the_tree_to_a_diff_and_open_a_file_read_only() {
    let r = repo();
    let dir = tempfile::tempdir().unwrap();
    let mut pty = spawn(dir.path(), &r, &["git"]);
    let s = pty.wait_for("the tree", |s| screen_has(s, "kuro git") && screen_has(s, "README.md"));
    assert!(screen_has(&s, "on main") && screen_has(&s, "▸ src/"), "{}", s.join("\n"));
    pty.send("a");
    pty.wait_for("the history", |s| screen_has(s, "Edit main, add notes") && screen_has(s, "Initial import"));
    pty.send("\r");
    let s = pty.wait_for("the commit", |s| screen_has(s, "Changed files (2)"));
    assert!(screen_has(&s, "A notes.txt") && screen_has(&s, "M src/main.rs"));
    pty.send("j\r"); // the diff of src/main.rs
    let s = pty.wait_for("the diff", |s| screen_has(s, "@@") && screen_has(s, "line three"));
    let plus = s.iter().position(|r| r.contains("+    println!(\"line three\")")).expect("an added line");
    assert_eq!(pty.fg_at(plus as u16, 0), vt100::Color::Idx(2), "added lines are green (slot 2)");
    let minus = s.iter().position(|r| r.contains("-    println!(\"line 3\")")).expect("a removed line");
    assert_eq!(pty.fg_at(minus as u16, 0), vt100::Color::Idx(1), "removed lines are red (slot 1)");
    pty.assert_default_background();
    pty.assert_ansi_palette_only();
    // open the file as it was at this commit, in the editor, read-only
    pty.send("o");
    let s = pty.wait_for("the file view", |s| screen_has(s, "line three") && screen_has(s, "Rust"));
    assert!(screen_has(&s, "read-only"), "{}", s.join("\n"));
    let before = std::fs::read_to_string(r.path().join("src/main.rs")).unwrap();
    pty.send("ix"); // would insert in the editor
    pty.send("\x13"); // Ctrl+S would save
    pty.wait_for("the refusal", |s| screen_has(s, "read-only"));
    assert_eq!(std::fs::read_to_string(r.path().join("src/main.rs")).unwrap(), before, "nothing was written");
    pty.send("q");
    pty.wait_for("back in the diff", |s| screen_has(s, "kuro git") && screen_has(s, "@@"));
    // q quits kuro git
    pty.send("q");
    assert!(pty.wait_exit(), "q leaves kuro git");
}

#[test]
fn fl_opens_one_files_history_directly() {
    let r = repo();
    let dir = tempfile::tempdir().unwrap();
    let mut pty = spawn(dir.path(), &r, &["git", "-fl", "src/main.rs"]);
    // the screen opens at once; the commits arrive while git is still reading
    let s =
        pty.wait_for("the file history with its commits", |s| screen_has(s, "history of src/main.rs") && screen_has(s, "Initial import"));
    assert!(screen_has(&s, "Edit main, add notes") && screen_has(&s, "Initial import"), "{}", s.join("\n"));
    assert!(!screen_has(&s, "kuro git · demo · on"), "no tree first");
    pty.send("\x11"); // Ctrl+Q
    assert!(pty.wait_exit());
}

#[test]
fn from_a_subfolder_with_a_relative_path() {
    let r = repo();
    let dir = tempfile::tempdir().unwrap();
    let sub = r.path().join("src");
    let pty = Pty::spawn_args(dir.path(), CFG, &["git", "-fl", "main.rs"], &sub, &ENV);
    pty.wait_for("history of the file", |s| screen_has(s, "history of src/main.rs"));
}

#[test]
fn the_terminal_is_restored_when_it_quits() {
    let r = repo();
    let dir = tempfile::tempdir().unwrap();
    let mut pty = spawn(dir.path(), &r, &["git"]);
    pty.wait_for("started", |s| screen_has(s, "README.md"));
    pty.send("q");
    assert!(pty.wait_exit());
    let raw = String::from_utf8_lossy(&pty.raw_output()).into_owned();
    assert!(raw.contains("\x1b[?1049l"), "the alternate screen was left");
    assert!(!raw.contains("38;2") && !raw.contains("38;5") && !raw.contains("48;"), "no truecolor, no 256-colour, no background");
}

#[test]
fn changes_are_detected_while_it_runs_and_can_be_discarded_in_the_real_binary() {
    let r = repo();
    let dir = tempfile::tempdir().unwrap();
    let mut pty = spawn(dir.path(), &r, &["git"]);
    pty.wait_for("the tree", |s| screen_has(s, "README.md") && !screen_has(s, "(c)"));
    // an edit made by another program shows up by itself, like in VS Code
    r.write("README.md", "# demo\nan edit made elsewhere\n");
    r.write("scratch.txt", "scratch\n");
    pty.wait_for("the badge", |s| screen_has(s, "2 changes (c)"));
    pty.send("c");
    let s = pty.wait_for("the list", |s| screen_has(s, "M  README.md") && screen_has(s, "U  scratch.txt"));
    assert!(screen_has(&s, "changes of the working tree (2)"), "{}", s.join("\n"));
    pty.send("\r");
    pty.wait_for("the diff", |s| screen_has(s, "+an edit made elsewhere"));
    pty.send("\x1b"); // back to the list
    pty.wait_for("the list again", |s| screen_has(s, "M  README.md") && screen_has(s, "Enter diff"));
    pty.send("x");
    pty.wait_for("the question", |s| screen_has(s, "Discard the changes of README.md?"));
    pty.send("y");
    pty.wait_for("restored", |s| !screen_has(s, "M  README.md") && screen_has(s, "U  scratch.txt"));
    assert_eq!(std::fs::read_to_string(r.path().join("README.md")).unwrap(), "# demo\n");
    assert!(r.path().join("scratch.txt").exists(), "the untracked file was not part of it");
    pty.send("X");
    pty.wait_for("the question about untracked files", |s| {
        screen_has(s, "Delete the untracked file scratch.txt?") || screen_has(s, "untracked file")
    });
    pty.send("y");
    pty.wait_for("clean", |s| screen_has(s, "no changes: the working tree is clean"));
    assert!(!r.path().join("scratch.txt").exists());
    pty.send("q");
    assert!(pty.wait_exit());
}
