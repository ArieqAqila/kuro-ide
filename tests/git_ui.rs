//! `kuro git`: the four views, keys, mouse, colours and the read-only file view, on throw-away repositories.

mod common;
use common::gitrepo::*;
use common::snap::assert_snapshot;
use common::ui::key;
use crossterm::event::{MouseButton, MouseEventKind};
use kuro::clipboard::MemoryClipboard;
use kuro::config::Config;
use kuro::gitview::{GitApp, View};
use kuro::screen::Screen;
use ratatui::buffer::Buffer as Cells;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier};

const W: u16 = 100;
const H: u16 = 30;

fn rust_main(extra: &str) -> String {
    let mut s = String::from("fn main() {\n");
    for i in 0..40 {
        s.push_str(&format!("    println!(\"line {i} of main\");\n"));
    }
    s.push_str(extra);
    s.push_str("}\n");
    s
}

/// Four commits: add files; edit + add; rename src/lib.rs and delete the guide; edit the readme.
fn scenario() -> Repo {
    let mut r = Repo::new();
    r.write("README.md", "# demo\n\nA demo repository.\n");
    r.write("src/main.rs", &rust_main(""));
    r.write("src/lib.rs", &lines(40, "library"));
    r.write("docs/guide.txt", &lines(10, "guide"));
    r.commit("Initial import\n\nAdds the first files of the demo.");
    r.write("src/main.rs", &rust_main("    println!(\"added later\");\n"));
    r.write("notes.txt", "remember the milk\n");
    r.commit("Edit main and add notes");
    r.git(&["mv", "src/lib.rs", "src/core.rs"]);
    std::fs::remove_file(r.path().join("docs/guide.txt")).unwrap();
    r.commit("Rename lib to core, drop the guide");
    r.write("README.md", "# demo\n\nA demo repository.\n\nMore words here.\n");
    r.commit("Update the readme");
    r
}

fn app(r: &Repo, file: Option<&str>) -> GitApp {
    let mut a = GitApp::new(r.open(), Config::default(), file.map(String::from)).unwrap();
    a.make_clipboard = Box::new(|_| Box::new(MemoryClipboard::default()));
    a.repo_name = "demo".into(); // the temp folder's name is random
    a.wait_loaded(); // a file's history arrives in the background: most tests look at the settled screen
    a
}

fn draw(a: &mut GitApp) -> Cells {
    let area = Rect::new(0, 0, W, H);
    let mut buf = Cells::empty(area);
    a.render(&mut buf, area);
    buf
}

fn rows(a: &mut GitApp) -> Vec<String> {
    kuro::render::rows_text(&draw(a))
}

fn screen(a: &mut GitApp) -> String {
    rows(a).join("\n")
}

/// Press keys named like `Enter`, `Esc`, `PageDown`, `j`, `G`, `ctrl+q` (names longer than one character are case-insensitive).
fn press(a: &mut GitApp, keys: &str) {
    for k in keys.split_whitespace() {
        let k = if k.chars().count() > 1 { k.to_lowercase() } else { k.to_string() };
        a.handle_key(key(&k));
    }
    a.wait_loaded();
}

fn row_with(a: &mut GitApp, needle: &str) -> u16 {
    rows(a).iter().position(|r| r.contains(needle)).unwrap_or_else(|| panic!("{needle:?} not on screen:\n{}", screen(a))) as u16
}

/// Walk to the diff of the commit at history row `i` (the whole commit).
fn to_whole_diff(a: &mut GitApp, i: usize) {
    press(a, "a");
    for _ in 0..i {
        press(a, "j");
    }
    press(a, "d");
}

mod views {
    use super::*;

    #[test]
    fn the_tree_lists_folders_collapsed_and_opens_them() {
        let r = scenario();
        let mut a = app(&r, None);
        assert_eq!(a.view, View::Tree);
        let s = screen(&mut a);
        assert!(s.contains("kuro git") && s.contains("on main"), "{s}");
        assert!(s.contains("▸ src/") && s.contains("README.md") && s.contains("notes.txt"), "{s}");
        assert!(!s.contains("main.rs"), "folders start collapsed:\n{s}");
        assert!(!s.contains("guide.txt"), "a file deleted at HEAD is not in the tree");
        // open src/
        let src_row = a.rows.iter().position(|r| r.name == "src").unwrap();
        for _ in 0..src_row {
            press(&mut a, "j");
        }
        press(&mut a, "Enter");
        let s = screen(&mut a);
        assert!(s.contains("▾ src/") && s.contains("main.rs") && s.contains("core.rs"), "{s}");
        press(&mut a, "Enter");
        assert!(!screen(&mut a).contains("main.rs"), "Enter on an open folder folds it");
    }

    #[test]
    fn enter_on_a_file_shows_its_history_and_escape_returns() {
        let r = scenario();
        let mut a = app(&r, None);
        let readme = a.rows.iter().position(|r| r.name == "README.md").unwrap();
        for _ in 0..readme {
            press(&mut a, "Down");
        }
        press(&mut a, "Enter");
        assert_eq!(a.view, View::History);
        let s = screen(&mut a);
        assert!(s.contains("history of README.md"), "{s}");
        assert!(!s.contains("reading…"), "finished: {s}");
        assert!(
            s.contains("Update the readme") && s.contains("Initial import") && !s.contains("Edit main"),
            "only this file's commits:\n{s}"
        );
        press(&mut a, "Esc");
        assert_eq!(a.view, View::Tree);
    }

    #[test]
    fn a_shows_the_whole_history_and_tab_goes_back_to_the_tree() {
        let r = scenario();
        let mut a = app(&r, None);
        press(&mut a, "a");
        assert_eq!(a.view, View::History);
        let s = screen(&mut a);
        for subject in ["Update the readme", "Rename lib to core", "Edit main and add notes", "Initial import"] {
            assert!(s.contains(subject), "{subject}:\n{s}");
        }
        assert!(s.contains("Test User") && s.contains("2026-01-04 10:00"), "{s}");
        press(&mut a, "Tab");
        assert_eq!(a.view, View::Tree);
        press(&mut a, "Tab");
        assert_eq!(a.view, View::History);
        press(&mut a, "t");
        assert_eq!(a.view, View::Tree);
    }

    #[test]
    fn a_commit_shows_author_date_message_and_the_changed_files() {
        let r = scenario();
        let mut a = app(&r, None);
        press(&mut a, "a j j Enter"); // the third newest = "Edit main and add notes"? (0 readme, 1 rename, 2 edit)
        assert_eq!(a.view, View::Commit);
        let s = screen(&mut a);
        assert!(s.contains("Author: Test User <test@example.invalid>") && s.contains("Date:   2026-01-02 10:00"), "{s}");
        assert!(s.contains("Edit main and add notes") && s.contains("Changed files (2)"), "{s}");
        assert!(s.contains("A notes.txt") && s.contains("M src/main.rs"), "{s}");
        // the root commit has a body and only additions
        press(&mut a, "Esc j Enter");
        let s = screen(&mut a);
        assert!(s.contains("Adds the first files of the demo.") && s.contains("A README.md") && s.contains("Changed files (4)"), "{s}");
    }

    #[test]
    fn renames_and_deletions_are_labelled() {
        let r = scenario();
        let mut a = app(&r, None);
        press(&mut a, "a j Enter");
        let s = screen(&mut a);
        assert!(s.contains("R src/lib.rs → src/core.rs") && s.contains("D docs/guide.txt"), "{s}");
    }

    #[test]
    fn a_file_history_follows_a_rename_and_selects_the_followed_file_in_each_commit() {
        let r = scenario();
        let mut a = app(&r, Some("src/core.rs"));
        assert_eq!(a.view, View::History);
        let s = screen(&mut a);
        assert!(s.contains("history of src/core.rs"), "{s}");
        assert!(s.contains("Rename lib to core") && s.contains("Initial import"), "history continues past the rename:\n{s}");
        assert!(s.contains("[R src/lib.rs → src/core.rs]"), "the rename is marked:\n{s}");
        assert!(!s.contains("Update the readme"), "{s}");
        press(&mut a, "Enter");
        assert_eq!(
            a.detail.as_ref().unwrap().changes[a.files_scroll.sel].path,
            "src/core.rs",
            "the rename commit opens on the followed file"
        );
        // the diff of the followed file only
        press(&mut a, "Esc d");
        assert_eq!(a.view, View::Diff);
        let s = screen(&mut a);
        assert!(
            s.contains("rename from src/lib.rs") && !s.contains("docs/guide.txt"),
            "only the followed file, not the whole commit:\n{s}"
        );
    }

    #[test]
    fn a_file_with_no_history_is_an_error_and_a_repository_with_no_commits_is_handled() {
        let r = scenario();
        assert!(GitApp::new(r.open(), Config::default(), Some("no/such/file".into())).is_err());
        let empty = Repo::new();
        let mut a = app(&empty, None);
        assert_eq!(a.view, View::History);
        let s = screen(&mut a);
        assert!(s.contains("no commits yet"), "{s}");
        press(&mut a, "j k Enter d Esc Tab a");
        assert!(screen(&mut a).contains("no commits yet"));
        assert!(GitApp::new(empty.open(), Config::default(), Some("a.txt".into())).is_err());
    }

    #[test]
    fn a_detached_head_is_named_in_the_header() {
        let r = scenario();
        let first = r.git(&["rev-list", "--max-parents=0", "HEAD"]).trim().to_string();
        r.git(&["checkout", "-q", "--detach", &first]);
        let mut a = app(&r, None);
        let s = screen(&mut a);
        assert!(s.contains(&format!("HEAD detached at {}", &first[..7])), "{s}");
        press(&mut a, "a");
        assert!(screen(&mut a).contains("Initial import") && !screen(&mut a).contains("Update the readme"));
    }
}

mod diffs {
    use super::pointer::mouse_at;
    use super::*;

    fn fg_of(buf: &Cells, y: u16, x: u16) -> Color {
        buf[(x, y)].fg
    }

    #[test]
    fn diff_lines_are_coloured_by_role_with_palette_colours_only() {
        let r = scenario();
        let mut a = app(&r, None);
        to_whole_diff(&mut a, 2); // Edit main and add notes
        assert_eq!(a.view, View::Diff);
        let buf = draw(&mut a);
        let s = kuro::render::rows_text(&buf);
        let y_of =
            |needle: &str| s.iter().position(|r| r.contains(needle)).unwrap_or_else(|| panic!("{needle:?}:\n{}", s.join("\n"))) as u16;
        assert_eq!(fg_of(&buf, y_of("+remember the milk"), 0), Color::Green, "added lines: slot 2");
        assert_eq!(fg_of(&buf, y_of("+    println!(\"added later\")"), 0), Color::Green);
        assert_eq!(fg_of(&buf, y_of("@@"), 0), Color::Cyan, "hunk headers: slot 6");
        let meta = y_of("diff --git");
        assert!(buf[(0, meta)].modifier.contains(Modifier::BOLD), "file headers are bold in the default colour");
        assert_eq!(fg_of(&buf, meta, 0), Color::Reset);
        // a removal
        let mut b = app(&r, None);
        to_whole_diff(&mut b, 1);
        let buf = draw(&mut b);
        let s = kuro::render::rows_text(&buf);
        let y = s.iter().position(|r| r.contains("-line 0 of the guide file")).unwrap() as u16;
        assert_eq!(fg_of(&buf, y, 0), Color::Red, "removed lines: slot 1");
        for yy in 0..H {
            for x in 0..W {
                let c = &buf[(x, yy)];
                assert_eq!(c.bg, Color::Reset, "no background at ({x},{yy})");
                assert!(!matches!(c.fg, Color::Rgb(..) | Color::Indexed(_)));
            }
        }
    }

    #[test]
    fn the_colours_can_be_mapped_in_the_config() {
        let r = scenario();
        let mut cfg = Config::default();
        cfg.colors.diff_add = 10;
        cfg.colors.diff_del = -1;
        let mut a = GitApp::new(r.open(), cfg, None).unwrap();
        to_whole_diff(&mut a, 2);
        let buf = draw(&mut a);
        let s = kuro::render::rows_text(&buf);
        let y = s.iter().position(|r| r.contains("+remember the milk")).unwrap() as u16;
        assert_eq!(buf[(0, y)].fg, Color::LightGreen);
    }

    #[test]
    fn scrolling_keys_move_the_view_and_clamp_at_both_ends() {
        let r = scenario();
        let mut a = app(&r, None);
        to_whole_diff(&mut a, 3); // the initial import: a long diff
        assert!(a.diff.len() > 100);
        assert_eq!(a.diff_top, 0);
        press(&mut a, "j j j");
        assert_eq!(a.diff_top, 3);
        press(&mut a, "k k k k k");
        assert_eq!(a.diff_top, 0, "clamped at the start");
        press(&mut a, "PageDown");
        let page = a.diff_top;
        assert!(page > 15, "a page is about a screenful: {page}");
        press(&mut a, "End");
        let end = a.diff_top;
        press(&mut a, "PageDown j j");
        assert_eq!(a.diff_top, end, "clamped at the end");
        assert!(screen(&mut a).contains(a.diff.last().unwrap().as_str()), "the last line is visible at the end");
        press(&mut a, "Home");
        assert_eq!(a.diff_top, 0);
    }

    #[test]
    fn a_long_diff_has_a_vertical_scrollbar_that_follows_the_view() {
        let r = scenario();
        let mut a = app(&r, None);
        to_whole_diff(&mut a, 3);
        let buf = draw(&mut a);
        assert!(!"▲▼".contains(buf[(W - 1, 1)].symbol()) && !"▲▼".contains(buf[(W - 1, H - 2)].symbol()), "no arrow buttons");
        let thumb = |buf: &Cells| (1..H - 1).filter(|&y| buf[(W - 1, y)].symbol() == "┃").collect::<Vec<_>>();
        let top = thumb(&buf);
        assert_eq!(top[0], 1, "the thumb starts at the top of the bar");
        press(&mut a, "End");
        let bottom = thumb(&draw(&mut a));
        assert_eq!(*bottom.last().unwrap(), H - 2, "and ends at the bottom of the bar at the end");
        // a short diff has none
        let mut b = app(&r, None);
        to_whole_diff(&mut b, 0);
        let short = draw(&mut b);
        assert!((1..H - 1).all(|y| !["┃", "│"].contains(&short[(W - 1, y)].symbol())), "a short diff has no bar");
    }

    #[test]
    fn wide_lines_scroll_sideways_with_keys_and_the_horizontal_wheel() {
        let mut r = scenario();
        r.write("wide.txt", &format!("{}\n", "w".repeat(250)));
        r.commit("a very wide line");
        let mut a = app(&r, None);
        press(&mut a, "a Enter");
        press(&mut a, "d");
        assert_eq!(a.view, View::Diff);
        let buf = draw(&mut a);
        assert!(["━", "─"].contains(&buf[(0, H - 2)].symbol()), "a horizontal bar");
        press(&mut a, "l l");
        assert_eq!(a.diff_left, 16);
        press(&mut a, "h");
        assert_eq!(a.diff_left, 8);
        for _ in 0..100 {
            press(&mut a, "Right");
        }
        let max = a.diff_left;
        assert!((150..260).contains(&max), "clamped at the end of the widest line: {max}");
        mouse_at(&mut a, MouseEventKind::ScrollLeft, 10, 5);
        assert_eq!(a.diff_left, max - 6);
        for _ in 0..100 {
            mouse_at(&mut a, MouseEventKind::ScrollLeft, 10, 5);
        }
        assert_eq!(a.diff_left, 0);
    }

    #[test]
    fn n_and_p_step_through_commits_and_keep_the_same_file_when_it_is_there() {
        let r = scenario();
        let mut a = app(&r, None);
        // the diff of src/main.rs in "Edit main and add notes"
        press(&mut a, "a j j Enter");
        let main_row = a.detail.as_ref().unwrap().changes.iter().position(|c| c.path == "src/main.rs").unwrap();
        for _ in 0..main_row {
            press(&mut a, "j");
        }
        press(&mut a, "Enter");
        assert!(a.diff_title.contains("src/main.rs") && a.diff_title.contains("Edit main"));
        press(&mut a, "n");
        assert!(
            a.diff_title.contains("Initial import") && a.diff_title.contains("src/main.rs"),
            "the older commit, same file: {}",
            a.diff_title
        );
        press(&mut a, "n");
        assert!(screen(&mut a).contains("oldest commit"));
        press(&mut a, "p p");
        assert!(a.diff_title.contains("Rename lib"), "the next commit does not touch main.rs: the whole diff is shown: {}", a.diff_title);
        assert!(!a.diff_title.contains("src/main.rs"));
        press(&mut a, "p p p");
        assert!(screen(&mut a).contains("newest commit"));
        // also from the commit view
        press(&mut a, "Esc");
        assert_eq!(a.view, View::Commit);
        press(&mut a, "n");
        assert!(screen(&mut a).contains("Update the readme") || screen(&mut a).contains("Rename lib"));
    }
}

mod file_view {
    use super::*;

    fn open_main_at_edit_commit(r: &Repo) -> GitApp {
        let mut a = app(r, None);
        press(&mut a, "a j j Enter");
        let row = a.detail.as_ref().unwrap().changes.iter().position(|c| c.path == "src/main.rs").unwrap();
        for _ in 0..row {
            press(&mut a, "j");
        }
        press(&mut a, "o");
        a
    }

    #[test]
    fn o_opens_the_file_as_it_was_at_the_commit_in_the_editor() {
        let r = scenario();
        let mut a = open_main_at_edit_commit(&r);
        let v = a.viewer.as_ref().expect("the editor opened");
        assert_eq!(v.buffer.text(), rust_main("    println!(\"added later\");\n"), "the content at that commit, not the current one");
        assert_eq!(v.lang.name, "Rust", "syntax colours by the file name");
        let s = screen(&mut a);
        assert!(s.contains("println!") && s.contains("Rust"), "{s}");
        assert!(a.viewer.as_ref().unwrap().view_only.is_some());
    }

    #[test]
    fn the_file_view_refuses_every_edit_and_every_write() {
        let r = scenario();
        let mut a = open_main_at_edit_commit(&r);
        let before = a.viewer.as_ref().unwrap().buffer.text();
        let disk = std::fs::read(r.path().join("src/main.rs")).unwrap();
        // typing in normal mode, entering insert mode, deleting, replacing, pasting, undo
        press(&mut a, "i x Esc x d d p u ctrl+x ctrl+v");
        for c in "abc".chars() {
            a.handle_key(key(&c.to_string()));
        }
        assert_eq!(a.viewer.as_ref().unwrap().buffer.text(), before, "nothing changed");
        assert!(!a.viewer.as_ref().unwrap().buffer.is_modified());
        // writes
        press(&mut a, "ctrl+s");
        let v = a.viewer.as_mut().unwrap();
        for c in ["w", "w!", "wq", "x", "w /tmp/kuro-should-not-exist", "e other"] {
            v.exec_command(c);
            assert!(v.msg.as_deref().unwrap_or("").contains("read-only"), "{c}: {:?}", v.msg);
        }
        assert!(!std::path::Path::new("/tmp/kuro-should-not-exist").exists());
        assert_eq!(std::fs::read(r.path().join("src/main.rs")).unwrap(), disk, "the working tree file is untouched");
        // but looking around and copying work
        press(&mut a, "G g shift+right shift+right");
        assert!(a.viewer.as_ref().unwrap().buffer.selection().is_some());
    }

    #[test]
    fn q_ctrl_q_and_colon_q_close_the_file_view_and_return_to_the_commit() {
        let r = scenario();
        for how in ["q", "ctrl+q", ":q"] {
            let mut a = open_main_at_edit_commit(&r);
            if how == ":q" {
                a.viewer.as_mut().unwrap().exec_command("q");
                press(&mut a, "x"); // any key: the closed view is noticed
            } else {
                press(&mut a, how);
            }
            assert!(a.viewer.is_none(), "{how}");
            assert!(!a.quit, "{how}: closing the file view does not quit kuro git");
            assert_eq!(a.view, View::Commit);
        }
    }

    #[test]
    fn a_deleted_file_opens_as_it_was_in_the_parent_and_binary_files_are_refused() {
        let mut r = scenario();
        let mut a = app(&r, None);
        press(&mut a, "a j Enter"); // the rename + deletion commit
        let row = a.detail.as_ref().unwrap().changes.iter().position(|c| c.path == "docs/guide.txt").unwrap();
        for _ in 0..row {
            press(&mut a, "j");
        }
        press(&mut a, "o");
        assert_eq!(a.viewer.as_ref().expect("opened from the parent").buffer.text(), lines(10, "guide"));
        press(&mut a, "q");
        // a binary file
        std::fs::write(r.path().join("img.bin"), [0u8, 159, 146, 150, 0]).unwrap();
        r.commit("binary");
        let mut b = app(&r, None);
        press(&mut b, "a Enter o");
        assert!(b.viewer.is_none());
        assert!(screen(&mut b).contains("binary file"), "{}", screen(&mut b));
    }
}

pub mod pointer {
    use super::*;

    pub fn mouse_at(a: &mut GitApp, kind: MouseEventKind, x: u16, y: u16) {
        a.handle_mouse(crossterm::event::MouseEvent { kind, column: x, row: y, modifiers: crossterm::event::KeyModifiers::NONE });
    }

    #[test]
    fn clicking_selects_and_a_second_click_opens() {
        let r = scenario();
        let mut a = app(&r, None);
        press(&mut a, "a");
        let y = row_with(&mut a, "Edit main and add notes");
        mouse_at(&mut a, MouseEventKind::Down(MouseButton::Left), 20, y);
        assert_eq!(a.history.sel, 2);
        assert_eq!(a.view, View::History);
        mouse_at(&mut a, MouseEventKind::Down(MouseButton::Left), 20, y);
        assert_eq!(a.view, View::Commit);
        // a click on the tree opens a folder
        let mut t = app(&r, None);
        let y = row_with(&mut t, "src/");
        mouse_at(&mut t, MouseEventKind::Down(MouseButton::Left), 10, y);
        mouse_at(&mut t, MouseEventKind::Down(MouseButton::Left), 10, y);
        assert!(screen(&mut t).contains("main.rs"));
    }

    #[test]
    fn the_wheel_scrolls_lists_and_the_diff() {
        let mut r = Repo::new();
        r.many_commits(80);
        let mut a = app(&r, None);
        press(&mut a, "a");
        assert_eq!(a.history.top, 0);
        mouse_at(&mut a, MouseEventKind::ScrollDown, 20, 5);
        assert_eq!(a.history.top, 3);
        mouse_at(&mut a, MouseEventKind::ScrollUp, 20, 5);
        mouse_at(&mut a, MouseEventKind::ScrollUp, 20, 5);
        assert_eq!(a.history.top, 0, "clamped");
        assert!(screen(&mut a).contains("commit number 79"));
    }

    #[test]
    fn scrollbar_track_and_thumb_work_in_the_history_and_there_are_no_arrow_buttons() {
        let mut r = Repo::new();
        r.many_commits(120);
        let mut a = app(&r, None);
        press(&mut a, "a");
        draw(&mut a); // the window size is known after the first draw
        let x = W - 1;
        // the last cell of the bar is track like any other (it was a down arrow once): a page, not one line
        mouse_at(&mut a, MouseEventKind::Down(MouseButton::Left), x, H - 2);
        assert_eq!(a.history.top, 27, "one page (rows - 1)");
        mouse_at(&mut a, MouseEventKind::Down(MouseButton::Left), x, 1); // the thumb is not here any more: the track above it
        assert_eq!(a.history.top, 0, "paged back");
        mouse_at(&mut a, MouseEventKind::Down(MouseButton::Left), x, H - 4); // the track below the thumb: a page
        assert_eq!(a.history.top, 27, "one page (rows - 1)");
        // drag the thumb to the bottom
        let buf = draw(&mut a);
        let grab = (1..H - 1).find(|&y| buf[(x, y)].symbol() == "┃").expect("a thumb is drawn");
        mouse_at(&mut a, MouseEventKind::Down(MouseButton::Left), x, grab);
        mouse_at(&mut a, MouseEventKind::Drag(MouseButton::Left), x, H - 3);
        assert!(a.history.top > 60, "{}", a.history.top);
        mouse_at(&mut a, MouseEventKind::Up(MouseButton::Left), x, H - 3);
        let top = a.history.top;
        mouse_at(&mut a, MouseEventKind::Drag(MouseButton::Left), x, 3);
        assert_eq!(a.history.top, top, "after the release nothing follows the pointer");
    }

    #[test]
    fn more_commits_are_loaded_as_you_go_down() {
        let mut r = Repo::new();
        r.many_commits(450);
        let mut a = app(&r, None);
        press(&mut a, "a");
        assert_eq!(a.commits.len(), 200);
        press(&mut a, "End");
        assert!(a.commits.len() >= 400, "the next page was loaded: {}", a.commits.len());
        press(&mut a, "End End End");
        assert_eq!(a.commits.len(), 450);
        assert!(a.all_loaded);
        assert!(screen(&mut a).contains("commit number 0"));
    }

    #[test]
    fn a_file_history_opens_at_once_and_fills_in_while_git_is_still_reading() {
        let mut r = Repo::new();
        r.many_commits(450);
        let mut a = GitApp::new(r.open(), Config::default(), None).unwrap();
        a.repo_name = "demo".into();
        let row = a.rows.iter().position(|r| r.name == "count.txt").unwrap();
        for _ in 0..row {
            a.handle_key(key("down"));
        }
        a.handle_key(key("enter"));
        // nothing was waited for: the history view is already there and says that it is reading
        assert_eq!(a.view, View::History);
        assert!(a.loading() || a.all_loaded, "started in the background");
        let s = screen(&mut a);
        assert!(s.contains("history of count.txt"), "{s}");
        // the commits arrive without any key being pressed, and every one exactly once
        a.wait_loaded();
        assert!(!a.loading());
        assert_eq!(a.commits.len(), 450, "none twice, none missing");
        assert!(a.all_loaded);
        let subjects: std::collections::HashSet<_> = a.commits.iter().map(|c| c.subject.clone()).collect();
        assert_eq!(subjects.len(), 450);
        assert_eq!(a.commits[0].subject, "commit number 449", "newest first");
    }

    #[test]
    fn going_back_to_the_tree_stops_git_and_a_file_without_history_says_so() {
        let mut r = Repo::new();
        r.many_commits(300);
        let mut a = GitApp::new(r.open(), Config::default(), Some("count.txt".into())).unwrap();
        assert_eq!(a.view, View::History);
        a.handle_key(key("esc"));
        a.tick();
        assert_eq!(a.view, View::Tree);
        assert!(!a.loading(), "git was stopped when the tree came back");
        // a file that exists but was never committed has no history: back at the tree with a message
        r.write("untracked.txt", "x\n");
        let mut b = GitApp::new(r.open(), Config::default(), None).unwrap();
        b.show_file_history("untracked.txt");
        b.wait_loaded();
        assert_eq!(b.view, View::Tree);
        assert!(b.msg.contains("no history for untracked.txt"), "{}", b.msg);
    }
}
mod inline_marks {
    use super::*;

    fn changed_repo() -> Repo {
        let mut r = Repo::new();
        r.write("v.txt", "name   INIT   \"20260710\"\nkeep   this   line\nsgm  tbtcab  lsgmstr1\n");
        r.commit("first");
        r.write("v.txt", "name   INIT   \"20260721\"\nkeep   this   line\nsgm  lM2W  lsgmstr1\n");
        r.commit("second");
        r
    }

    /// The text of the cells of row `y` that are in reverse video, in groups.
    fn reversed_groups(buf: &Cells, y: u16) -> Vec<String> {
        let mut groups: Vec<String> = vec![];
        let mut open = false;
        for x in 0..W {
            let c = &buf[(x, y)];
            if c.modifier.contains(Modifier::REVERSED) {
                if !open {
                    groups.push(String::new());
                }
                groups.last_mut().unwrap().push_str(c.symbol());
                open = true;
            } else {
                open = false;
            }
        }
        groups
    }

    #[test]
    fn the_words_that_changed_inside_a_line_are_in_reverse_video_on_both_sides() {
        let r = changed_repo();
        let mut a = app(&r, None);
        to_whole_diff(&mut a, 0);
        let buf = draw(&mut a);
        let s = rows(&mut a);
        let minus = s.iter().position(|l| l.starts_with("-name")).unwrap() as u16;
        let plus = s.iter().position(|l| l.starts_with("+name")).unwrap() as u16;
        assert_eq!(reversed_groups(&buf, minus), vec!["10"], "{}", s.join("\n"));
        assert_eq!(reversed_groups(&buf, plus), vec!["21"]);
        let minus2 = s.iter().position(|l| l.starts_with("-sgm")).unwrap() as u16;
        let plus2 = s.iter().position(|l| l.starts_with("+sgm")).unwrap() as u16;
        assert_eq!(reversed_groups(&buf, minus2), vec!["tbtcab"]);
        assert_eq!(reversed_groups(&buf, plus2), vec!["lM2W"]);
        // the colour is the one of the line (green / red slots), and there is never a background
        assert_eq!(buf[(1, minus)].fg, ratatui::style::Color::Red);
        for y in [minus, plus] {
            for x in 0..W {
                assert_eq!(buf[(x, y)].bg, ratatui::style::Color::Reset, "no background at {x},{y}");
            }
        }
    }

    #[test]
    fn unchanged_and_whole_new_lines_have_no_marks_and_the_view_scrolls_with_the_text() {
        let r = changed_repo();
        let mut a = app(&r, None);
        to_whole_diff(&mut a, 0);
        let buf = draw(&mut a);
        let s = rows(&mut a);
        let keep = s.iter().position(|l| l.starts_with(" keep")).unwrap() as u16;
        assert!(reversed_groups(&buf, keep).is_empty());
        // scrolled sideways, the marks move with their text and are cut at the edge
        let plus = s.iter().position(|l| l.starts_with("+name")).unwrap() as u16;
        let start = s[plus as usize].find("21\"").unwrap();
        a.diff_left = start + 1; // the left half of `21` is out of view
        let buf = draw(&mut a);
        assert_eq!(reversed_groups(&buf, plus), vec!["1"], "only the visible part is marked");
        a.diff_left = start + 2; // all of it is out of view
        let buf = draw(&mut a);
        assert!(reversed_groups(&buf, plus).is_empty());
    }
}

mod keys_and_docs {
    use super::*;

    #[test]
    fn q_and_ctrl_q_quit_from_every_view_and_esc_goes_back_step_by_step() {
        let r = scenario();
        for path in ["", "a", "a Enter", "a Enter d"] {
            for quit in ["q", "ctrl+q"] {
                let mut a = app(&r, None);
                press(&mut a, path);
                press(&mut a, quit);
                assert!(a.quit, "{path:?} then {quit}");
            }
        }
        let mut a = app(&r, None);
        press(&mut a, "a Enter d");
        assert_eq!(a.view, View::Diff);
        press(&mut a, "Esc");
        assert_eq!(a.view, View::Commit);
        press(&mut a, "BackSpace");
        assert_eq!(a.view, View::History);
        press(&mut a, "Esc");
        assert_eq!(a.view, View::Tree);
        press(&mut a, "Esc");
        assert_eq!(a.view, View::Tree, "nothing before the tree");
        assert!(!a.quit);
    }

    #[test]
    fn list_keys_move_page_and_jump() {
        let mut r = Repo::new();
        r.many_commits(60);
        let mut a = app(&r, None);
        press(&mut a, "a");
        press(&mut a, "j j k");
        assert_eq!(a.history.sel, 1);
        press(&mut a, "Down Up Up");
        assert_eq!(a.history.sel, 0);
        press(&mut a, "Up");
        assert_eq!(a.history.sel, 0, "clamped");
        press(&mut a, "PageDown");
        assert!(a.history.sel > 15);
        press(&mut a, "End");
        assert_eq!(a.history.sel, 59);
        press(&mut a, "Home");
        assert_eq!(a.history.sel, 0);
        press(&mut a, "G");
        assert_eq!(a.history.sel, 59);
        press(&mut a, "g");
        assert_eq!(a.history.sel, 0);
        press(&mut a, "PageDown PageUp");
        assert_eq!(a.history.sel, 0);
        // the selection stays on screen
        press(&mut a, "End");
        assert!(screen(&mut a).contains("commit number 0"));
    }

    #[test]
    fn every_documented_key_is_in_docs_git_md_and_the_reverse() {
        let docs = include_str!("../docs/GIT.md");
        for (view, keys, what) in kuro::gitview::KEYS {
            let row = docs.lines().find(|l| l.starts_with(&format!("| {view} | `{keys}` |")));
            let row = row.unwrap_or_else(|| panic!("docs/GIT.md has no row `| {view} | `{keys}` |`"));
            assert!(row.contains(what), "the description of {keys} differs: {row}");
        }
        let rows = docs.lines().filter(|l| l.starts_with("| ") && l.contains("| `")).count();
        assert_eq!(rows, kuro::gitview::KEYS.len(), "docs/GIT.md lists keys the program does not have (or the reverse)");
    }
}

mod looks {
    use super::*;

    #[test]
    fn tiny_windows_do_not_panic() {
        let r = scenario();
        for (w, h) in [(1, 1), (3, 3), (9, 4), (20, 5), (40, 6)] {
            let mut a = app(&r, None);
            for step in ["", "a", "Enter", "d", "o"] {
                press(&mut a, step);
                let area = Rect::new(0, 0, w, h);
                let mut buf = Cells::empty(area);
                a.render(&mut buf, area);
            }
        }
    }

    #[test]
    fn snapshots_of_the_four_views() {
        let r = scenario();
        let mut a = app(&r, None);
        assert_snapshot("git_tree", &draw(&mut a));
        press(&mut a, "a");
        assert_snapshot("git_history", &draw(&mut a));
        press(&mut a, "j Enter");
        assert_snapshot("git_commit", &draw(&mut a));
        press(&mut a, "Enter");
        assert_snapshot("git_diff", &draw(&mut a));
    }

    #[test]
    fn the_screen_never_uses_a_background_or_a_non_palette_colour() {
        let r = scenario();
        let mut a = app(&r, None);
        for step in ["", "a", "Enter", "Enter", "Esc d"] {
            press(&mut a, step);
            let buf = draw(&mut a);
            for y in 0..H {
                for x in 0..W {
                    let c = &buf[(x, y)];
                    assert_eq!(c.bg, Color::Reset);
                    assert!(!matches!(c.fg, Color::Rgb(..) | Color::Indexed(_)));
                }
            }
        }
    }
}
