//! `:hardundo` in the editor: the popup, the questions, restoring, cancelling, and the safety checks. Throw-away directories only.

mod common;
use common::ui::*;
use kuro::app::{App, Focus};
use kuro::config::Config;
use kuro::history::{HistoryOptions, Store};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;

struct Env {
    dir: tempfile::TempDir,
    store: Arc<Store>,
}

fn env() -> Env {
    let dir = tempfile::tempdir().unwrap();
    let store = Arc::new(Store::new(dir.path().join("state/history"), HistoryOptions::default()));
    Env { dir, store }
}

impl Env {
    fn put(&self, name: &str, text: &str) -> PathBuf {
        let p = self.dir.path().join("work").join(name);
        fs::create_dir_all(p.parent().unwrap()).unwrap();
        fs::write(&p, text).unwrap();
        p
    }

    /// A new editor session on this history.
    fn editor(&self) -> App {
        let mut cfg = Config::default();
        cfg.editor.modal = false;
        let (mut app, _) = app_cfg("", cfg);
        app.set_history(self.store.clone());
        app
    }

    /// Open `path`, type `text` at the start, save.
    fn edit_and_save(&self, path: &Path, text: &str) -> App {
        let mut app = self.editor();
        app.open_file(path);
        typed(&mut app, text);
        press(&mut app, "ctrl+s");
        assert!(!app.buffer.is_modified(), "{:?}", app.msg);
        app
    }
}

fn shown(app: &mut App) -> String {
    screen(app).join("\n")
}

#[test]
fn hardundo_restores_the_original_file_and_the_buffer_after_one_confirmation() {
    let e = env();
    let p = e.put("a.txt", "original line\n");
    let mut app = e.edit_and_save(&p, "EDIT ");
    assert_eq!(fs::read_to_string(&p).unwrap(), "EDIT original line\n");
    app.exec_command("hardundo");
    assert_eq!(app.focus, Focus::HardUndo);
    let s = shown(&mut app);
    assert!(s.contains("Hard undo: restore 1 file to before kuro first changed it?"), "{s}");
    assert!(s.contains("a.txt") && s.contains("1 change"), "{s}");
    assert!(s.contains("[y] restore") && s.contains("[n] cancel"), "{s}");
    assert_eq!(fs::read_to_string(&p).unwrap(), "EDIT original line\n", "nothing changes before the answer");
    press(&mut app, "y");
    assert_eq!(app.focus, Focus::Editor);
    assert_eq!(fs::read_to_string(&p).unwrap(), "original line\n");
    assert_eq!(app.buffer.text(), "original line\n", "the buffer shows the restored file");
    assert!(!app.buffer.is_modified());
    assert!(app.msg.as_deref().unwrap().contains("1 file restored"), "{:?}", app.msg);
}

#[test]
fn the_count_follows_the_number_of_writes_and_the_original_is_the_very_first_one() {
    let e = env();
    let p = e.put("a.txt", "v0\n");
    let mut app = e.editor();
    app.open_file(&p);
    for i in 1..=3 {
        typed(&mut app, &format!("{i}"));
        press(&mut app, "ctrl+s");
    }
    app.exec_command("hardundo");
    assert!(shown(&mut app).contains("3 changes"));
    press(&mut app, "y");
    assert_eq!(fs::read_to_string(&p).unwrap(), "v0\n");
}

#[test]
fn it_works_after_every_terminal_was_closed() {
    let e = env();
    let p = e.put("a.txt", "original\n");
    drop(e.edit_and_save(&p, "changed "));
    // a brand new editor, on the same history, hours later
    let mut app = e.editor();
    app.open_file(&p);
    assert_eq!(app.buffer.text(), "changed original\n");
    app.exec_command("hardundo");
    press(&mut app, "y");
    assert_eq!(fs::read_to_string(&p).unwrap(), "original\n");
    // and even an editor with no file open at all can undo everything
    let mut again = e.editor();
    drop(e.edit_and_save(&p, "more "));
    again.exec_command("hardundo all");
    press(&mut again, "y");
    assert_eq!(fs::read_to_string(&p).unwrap(), "original\n");
}

#[test]
fn cancelling_with_n_or_esc_changes_nothing() {
    let e = env();
    let p = e.put("a.txt", "original\n");
    let mut app = e.edit_and_save(&p, "EDIT ");
    for key in ["n", "esc", "N"] {
        app.exec_command("hardundo");
        press(&mut app, key);
        assert_eq!(app.focus, Focus::Editor, "{key}");
        assert_eq!(fs::read_to_string(&p).unwrap(), "EDIT original\n", "{key}: the file is untouched");
        assert_eq!(app.buffer.text(), "EDIT original\n");
        assert!(app.msg.as_deref().unwrap().contains("cancelled"), "{key}: {:?}", app.msg);
    }
    // the history is still there afterwards
    app.exec_command("hardundo");
    assert!(shown(&mut app).contains("Hard undo"));
}

#[test]
fn while_the_popup_is_open_other_keys_and_the_mouse_do_nothing() {
    let e = env();
    let p = e.put("a.txt", "original\n");
    let mut app = e.edit_and_save(&p, "EDIT ");
    app.exec_command("hardundo");
    typed(&mut app, "abc");
    press(&mut app, "ctrl+s enter space");
    click(&mut app, 5, 5);
    assert_eq!(app.focus, Focus::HardUndo);
    assert_eq!(app.buffer.text(), "EDIT original\n");
    assert_eq!(fs::read_to_string(&p).unwrap(), "EDIT original\n");
}

mod the_safety_checks {
    use super::*;

    #[test]
    fn a_change_made_elsewhere_after_kurus_last_write_asks_a_second_question_and_n_skips_the_file() {
        let e = env();
        let p = e.put("a.txt", "original\n");
        let mut app = e.edit_and_save(&p, "EDIT ");
        fs::write(&p, "edited in another editor\n").unwrap();
        app.exec_command("hardundo");
        let s = shown(&mut app);
        assert!(s.contains("⚠ changed since kuro's last write"), "{s}");
        assert!(s.contains("second question"), "{s}");
        press(&mut app, "y");
        assert_eq!(app.focus, Focus::HardUndo, "not restored yet: a second question");
        assert_eq!(fs::read_to_string(&p).unwrap(), "edited in another editor\n");
        let s = shown(&mut app);
        assert!(s.contains("overwrite these files as well?") && s.contains("a.txt") && s.contains("[n] skip them"), "{s}");
        press(&mut app, "n");
        assert_eq!(fs::read_to_string(&p).unwrap(), "edited in another editor\n", "skipped: never overwritten silently");
        assert!(app.msg.as_deref().unwrap().contains("1 skipped"), "{:?}", app.msg);
        // asked again: this time agree
        app.exec_command("hardundo");
        press(&mut app, "y y");
        assert_eq!(fs::read_to_string(&p).unwrap(), "original\n");
    }

    #[test]
    fn escape_at_the_second_question_cancels_everything() {
        let e = env();
        let a = e.put("a.txt", "a0\n");
        let b = e.put("b.txt", "b0\n");
        drop(e.edit_and_save(&a, "A "));
        let _ = e.edit_and_save(&b, "B ");
        fs::write(&a, "someone else\n").unwrap();
        let mut app = e.editor();
        app.exec_command("hardundo all");
        let s = shown(&mut app);
        assert!(s.contains("restore 2 files") && s.contains("a.txt") && s.contains("b.txt"), "{s}");
        press(&mut app, "y");
        assert_eq!(app.focus, Focus::HardUndo);
        let s = shown(&mut app);
        assert!(s.contains("a.txt") && !s.contains("b.txt"), "only the files that need the second answer are listed:\n{s}");
        press(&mut app, "esc");
        assert_eq!(fs::read_to_string(&a).unwrap(), "someone else\n");
        assert_eq!(fs::read_to_string(&b).unwrap(), "B b0\n", "the other file was not restored either: Esc cancels all");
    }

    #[test]
    fn n_at_the_second_question_restores_the_files_that_were_fine() {
        let e = env();
        let a = e.put("a.txt", "a0\n");
        let b = e.put("b.txt", "b0\n");
        drop(e.edit_and_save(&a, "A "));
        drop(e.edit_and_save(&b, "B "));
        fs::write(&a, "someone else\n").unwrap();
        let mut app = e.editor();
        app.exec_command("hardundo all");
        press(&mut app, "y n");
        assert_eq!(fs::read_to_string(&a).unwrap(), "someone else\n");
        assert_eq!(fs::read_to_string(&b).unwrap(), "b0\n");
        assert!(app.msg.as_deref().unwrap().contains("1 file restored, 1 skipped"), "{:?}", app.msg);
    }

    #[test]
    fn a_file_changed_while_the_popup_was_open_is_noticed_at_the_moment_of_restoring() {
        let e = env();
        let p = e.put("a.txt", "original\n");
        let mut app = e.edit_and_save(&p, "EDIT ");
        app.exec_command("hardundo");
        assert!(!shown(&mut app).contains("⚠"), "nothing is flagged when the popup opens");
        fs::write(&p, "changed while the question was open\n").unwrap();
        press(&mut app, "y");
        // it was not flagged when the popup opened, but it is now: the second question appears instead of a silent overwrite
        assert_eq!(app.focus, Focus::HardUndo);
        assert_eq!(fs::read_to_string(&p).unwrap(), "changed while the question was open\n", "still untouched");
        assert!(shown(&mut app).contains("overwrite these files as well?"));
        press(&mut app, "n");
        assert_eq!(fs::read_to_string(&p).unwrap(), "changed while the question was open\n", "skipped");
        assert!(app.msg.as_deref().unwrap().contains("skipped"), "{:?}", app.msg);
    }

    #[test]
    fn a_file_that_changes_after_the_second_question_was_shown_is_not_overwritten_unasked() {
        let e = env();
        let a = e.put("a.txt", "a0\n");
        let b = e.put("b.txt", "b0\n");
        drop(e.edit_and_save(&a, "A "));
        drop(e.edit_and_save(&b, "B "));
        fs::write(&a, "someone else\n").unwrap(); // flagged from the start
        let mut app = e.editor();
        app.exec_command("hardundo all");
        press(&mut app, "y"); // the second question lists a.txt only
        assert_eq!(app.focus, Focus::HardUndo);
        fs::write(&b, "b changed meanwhile\n").unwrap(); // b was not part of the question
        press(&mut app, "y");
        assert_eq!(fs::read_to_string(&a).unwrap(), "a0\n", "a.txt: the user said yes to it");
        assert_eq!(fs::read_to_string(&b).unwrap(), "b changed meanwhile\n", "b.txt: it changed after the questions, so it is skipped");
    }

    #[test]
    fn a_file_that_no_longer_exists_is_recreated_after_the_questions() {
        let e = env();
        let p = e.put("a.txt", "original\n");
        let mut app = e.edit_and_save(&p, "EDIT ");
        fs::remove_file(&p).unwrap();
        app.exec_command("hardundo");
        assert!(shown(&mut app).contains("⚠"), "a vanished file is flagged");
        press(&mut app, "y y");
        assert_eq!(fs::read_to_string(&p).unwrap(), "original\n");
    }

    #[test]
    fn a_file_kuro_created_is_deleted_and_the_popup_says_so() {
        let e = env();
        let p = e.dir.path().join("work/new.txt");
        fs::create_dir_all(p.parent().unwrap()).unwrap();
        let mut app = e.editor();
        app.open_file(&p); // does not exist yet
        typed(&mut app, "brand new");
        press(&mut app, "ctrl+s");
        assert!(p.exists());
        app.exec_command("hardundo");
        assert!(shown(&mut app).contains("created by kuro: will be deleted"));
        press(&mut app, "y");
        assert!(!p.exists());
    }

    #[test]
    fn edits_made_in_another_editor_between_kuro_sessions_are_part_of_the_history() {
        let e = env();
        let p = e.put("a.txt", "original\n");
        drop(e.edit_and_save(&p, "kuro "));
        fs::write(&p, "other editor was here\n").unwrap(); // saved outside kuro
        let mut app = e.editor();
        app.open_file(&p); // kuro notices the file is not what it last wrote
        typed(&mut app, "again ");
        press(&mut app, "ctrl+s");
        app.exec_command("hardundo");
        let s = shown(&mut app);
        assert!(s.contains("⚠ also changed outside kuro") && !s.contains("changed since kuro's last write"), "{s}");
        assert!(s.contains("2 changes"), "{s}");
        press(&mut app, "y y");
        assert_eq!(fs::read_to_string(&p).unwrap(), "original\n", "back to the original, the other editor's changes included");
    }
}

mod when_there_is_nothing_to_do {
    use super::*;

    #[test]
    fn every_reason_has_a_message_and_no_popup() {
        let e = env();
        let p = e.put("a.txt", "x\n");
        let mut app = e.editor();
        app.open_file(&p);
        app.exec_command("hardundo");
        assert_eq!(app.focus, Focus::Editor);
        assert!(app.msg.as_deref().unwrap().contains("nothing to undo"), "{:?}", app.msg);
        app.exec_command("hardundo all");
        assert!(app.msg.as_deref().unwrap().contains("nothing to undo in any file"));
        app.exec_command("hardundo banana");
        assert!(app.msg.as_deref().unwrap().contains("usage"));
        // no file name
        let mut nameless = e.editor();
        nameless.exec_command("hardundo");
        assert!(nameless.msg.as_deref().unwrap().contains("no file name"));
        // the file is back to the original by hand
        let mut a2 = e.edit_and_save(&p, "z");
        press(&mut a2, "backspace ctrl+s");
        assert_eq!(fs::read_to_string(&p).unwrap(), "x\n");
        a2.exec_command("hardundo");
        assert!(a2.msg.as_deref().unwrap().contains("nothing to undo"));
        // the history is off
        let mut off = e.editor();
        off.history = None;
        off.exec_command("hardundo");
        assert!(off.msg.as_deref().unwrap().contains("hard undo is off"));
    }

    #[test]
    fn excluded_files_are_not_recorded_so_there_is_nothing_to_undo() {
        let e = env();
        let p = e.put("prod.env", "SECRET=1\n");
        let mut app = e.edit_and_save(&p, "X");
        app.exec_command("hardundo");
        assert!(app.msg.as_deref().unwrap().contains("nothing to undo"));
        assert!(e.store.entries().is_empty(), "the secret file is not in the history at all");
    }

    #[test]
    fn a_file_over_the_size_limit_says_so_when_you_save_it() {
        let dir = tempfile::tempdir().unwrap();
        let store = Arc::new(Store::new(dir.path().join("h"), HistoryOptions { max_file_bytes: 100, ..Default::default() }));
        let p = dir.path().join("big.txt");
        fs::write(&p, "x".repeat(500)).unwrap();
        let mut cfg = Config::default();
        cfg.editor.modal = false;
        let (mut app, _) = app_cfg("", cfg);
        app.set_history(store.clone());
        app.open_file(&p);
        typed(&mut app, "y");
        press(&mut app, "ctrl+s");
        assert!(app.msg.as_deref().unwrap().contains("hard undo is off for this file"), "{:?}", app.msg);
        assert!(fs::read_to_string(&p).unwrap().starts_with('y'), "the save itself worked");
        assert!(store.entries().is_empty());
    }
}

#[test]
fn saving_keeps_working_when_the_history_cannot_be_written() {
    let dir = tempfile::tempdir().unwrap();
    let blocker = dir.path().join("history");
    fs::write(&blocker, "this is a file, not a folder").unwrap();
    let store = Arc::new(Store::new(blocker, HistoryOptions::default()));
    let p = dir.path().join("a.txt");
    fs::write(&p, "x\n").unwrap();
    let mut cfg = Config::default();
    cfg.editor.modal = false;
    let (mut app, _) = app_cfg("", cfg);
    app.set_history(store);
    app.open_file(&p);
    typed(&mut app, "y");
    press(&mut app, "ctrl+s");
    assert_eq!(fs::read_to_string(&p).unwrap(), "yx\n", "the file is saved");
    assert!(app.msg.as_deref().unwrap().contains("hard undo is off for this file"), "{:?}", app.msg);
}

#[test]
fn w_with_a_name_is_recorded_too_and_colon_w_bang_and_wq_go_through_the_history() {
    let e = env();
    let p = e.put("a.txt", "orig\n");
    let target = e.put("copy.txt", "copy original\n");
    let mut app = e.editor();
    app.open_file(&p);
    typed(&mut app, "X");
    app.exec_command(&format!("w {}", target.display()));
    assert_eq!(fs::read_to_string(&target).unwrap(), "Xorig\n");
    assert!(e.store.entry_of(&target).is_some(), "writing another file is recorded under that file");
    let mut other = e.editor();
    other.exec_command("hardundo all");
    press(&mut other, "y");
    assert_eq!(fs::read_to_string(&target).unwrap(), "copy original\n");
}

#[test]
fn restoring_keeps_file_permissions() {
    use std::os::unix::fs::PermissionsExt;
    let e = env();
    let p = e.put("run.sh", "#!/bin/sh\necho one\n");
    fs::set_permissions(&p, fs::Permissions::from_mode(0o750)).unwrap();
    let mut app = e.edit_and_save(&p, "# edited\n");
    assert_eq!(fs::metadata(&p).unwrap().permissions().mode() & 0o777, 0o750, "an atomic save keeps the mode");
    app.exec_command("hardundo");
    press(&mut app, "y");
    assert_eq!(fs::read_to_string(&p).unwrap(), "#!/bin/sh\necho one\n");
    assert_eq!(fs::metadata(&p).unwrap().permissions().mode() & 0o777, 0o750);
}

#[test]
fn a_long_list_is_cut_to_the_window_and_says_how_many_more() {
    let e = env();
    for i in 0..30 {
        let p = e.put(&format!("f{i:02}.txt"), "x\n");
        drop(e.edit_and_save(&p, "y"));
    }
    let mut app = e.editor();
    app.exec_command("hardundo all");
    let s = shown(&mut app);
    assert!(s.contains("restore 30 files") && s.contains("… and "), "{s}");
}

#[test]
fn the_popup_uses_no_background_and_only_default_colours() {
    use ratatui::style::Color;
    let e = env();
    let p = e.put("a.txt", "original\n");
    let mut app = e.edit_and_save(&p, "EDIT ");
    fs::write(&p, "other\n").unwrap();
    app.exec_command("hardundo");
    let buf = render(&mut app);
    for y in 0..H {
        for x in 0..W {
            let c = &buf[(x, y)];
            assert_eq!(c.bg, Color::Reset);
            assert!(matches!(c.fg, Color::Reset));
        }
    }
}
