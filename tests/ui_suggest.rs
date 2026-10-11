//! Word suggestions popup (Part 3.6).

mod common;
use common::ui::*;
use kuro::app::App;
use kuro::config::Config;
use ratatui::style::Modifier;

/// A modeless app with `text`, cursor at the end of the buffer.
fn typing(text: &str) -> App {
    let mut cfg = Config::default();
    cfg.editor.modal = false;
    let (mut app, _) = app_cfg(text, cfg);
    app.buffer.cursor = app.buffer.len_chars();
    app
}

fn popup_words(app: &mut App) -> Vec<String> {
    app.suggest.as_ref().map(|s| s.sug.items.clone()).unwrap_or_default()
}

fn selected(app: &App) -> Option<String> {
    app.suggest.as_ref().and_then(|s| s.sug.items.get(s.selected).cloned())
}

const WORDS: &str = "hello help helmet world\n";

#[test]
fn popup_appears_while_typing_and_lists_words_from_the_file() {
    let mut app = typing(WORDS);
    typed(&mut app, "he");
    let words = popup_words(&mut app);
    assert!(
        words.contains(&"hello".to_string()) && words.contains(&"help".to_string()) && words.contains(&"helmet".to_string()),
        "{words:?}"
    );
    assert!(!words.contains(&"world".to_string()));
    let s = screen(&mut app).join("\n");
    assert!(s.contains("hello") && s.contains("helmet"), "popup is drawn:\n{s}");
    assert!(s.contains('╭'), "with a border");
}

#[test]
fn nothing_is_suggested_below_the_minimum_length_or_without_matches() {
    let mut app = typing(WORDS);
    typed(&mut app, "h");
    assert!(app.suggest.is_none(), "one character is below suggest_min_chars = 2");
    typed(&mut app, "x");
    assert!(app.suggest.is_none(), "`hx` matches nothing");
}

#[test]
fn matching_ignores_case() {
    let mut app = typing(WORDS);
    typed(&mut app, "HEL");
    assert!(popup_words(&mut app).contains(&"hello".to_string()));
}

#[test]
fn popup_is_modal_aware_only_insert_mode_shows_it() {
    let (mut app, _) = app_with("hello help\n");
    press(&mut app, "G");
    assert!(app.suggest.is_none(), "Normal mode");
    press(&mut app, "A");
    typed(&mut app, "he");
    assert!(app.suggest.is_some(), "Insert mode");
    press(&mut app, "esc esc");
    assert!(app.suggest.is_none(), "first Esc dismisses the popup, second leaves Insert mode");
    assert_eq!(app.mode_label(), "NORMAL");
}

#[test]
fn arrows_and_ctrl_n_ctrl_p_move_the_selection_and_wrap() {
    let mut app = typing("hello help helmet\n");
    typed(&mut app, "hel");
    let words = popup_words(&mut app);
    assert_eq!(words.len(), 3);
    assert_eq!(selected(&app).as_deref(), Some(words[0].as_str()));
    press(&mut app, "down");
    assert_eq!(selected(&app).as_deref(), Some(words[1].as_str()));
    press(&mut app, "ctrl+n");
    assert_eq!(selected(&app).as_deref(), Some(words[2].as_str()));
    press(&mut app, "down");
    assert_eq!(selected(&app).as_deref(), Some(words[0].as_str()), "wraps to the top");
    press(&mut app, "up");
    assert_eq!(selected(&app).as_deref(), Some(words[2].as_str()), "wraps to the bottom");
    press(&mut app, "ctrl+p");
    assert_eq!(selected(&app).as_deref(), Some(words[1].as_str()));
    assert_eq!(text_of(&app), "hello help helmet\nhel", "navigation does not edit or move the cursor");
}

#[test]
fn selected_row_is_reverse_video_and_nothing_has_a_background() {
    let mut app = typing("hello help helmet\n");
    typed(&mut app, "hel");
    press(&mut app, "down");
    let word = selected(&app).unwrap();
    let buf = render(&mut app);
    // text of the reverse-video cells on each row
    let reversed: Vec<String> = (0..24u16)
        .map(|y| {
            (0..80u16)
                .filter(|x| buf[(*x, y)].modifier.contains(Modifier::REVERSED))
                .map(|x| buf[(x, y)].symbol().to_string())
                .collect::<String>()
        })
        .filter(|r| !r.is_empty())
        .collect();
    assert_eq!(reversed.len(), 1, "exactly one highlighted row: {reversed:?}");
    assert_eq!(reversed[0].trim(), word, "and it is the selected suggestion");
    assert!(buf.content.iter().all(|c| c.bg == ratatui::style::Color::Reset), "no background");
}

#[test]
fn tab_accepts_and_replaces_the_typed_prefix() {
    let mut app = typing("hello help\n");
    typed(&mut app, "he");
    let first = selected(&app).unwrap();
    press(&mut app, "tab");
    assert_eq!(text_of(&app), format!("hello help\n{first}"));
    assert_eq!(app.buffer.cursor, app.buffer.len_chars(), "cursor ends after the word");
    assert!(app.suggest.is_none(), "popup closes after accepting");
    typed(&mut app, " ");
    assert_eq!(text_of(&app), format!("hello help\n{first} "), "typing continues normally");
}

#[test]
fn enter_accepts_instead_of_inserting_a_newline() {
    let mut app = typing("alphabet\n");
    typed(&mut app, "alp");
    press(&mut app, "enter");
    assert_eq!(text_of(&app), "alphabet\nalphabet");
    assert_eq!(app.buffer.line_count(), 2, "no extra line");
    press(&mut app, "enter");
    assert_eq!(app.buffer.line_count(), 3, "with the popup closed, Enter is a newline again");
}

#[test]
fn accepting_a_substring_match_replaces_the_prefix_with_the_whole_word() {
    let mut app = typing("my_counter\n");
    typed(&mut app, "coun");
    assert_eq!(selected(&app).as_deref(), Some("my_counter"));
    press(&mut app, "tab");
    assert_eq!(text_of(&app), "my_counter\nmy_counter");
}

#[test]
fn accept_keeps_text_before_the_word_and_after_the_cursor() {
    let mut app = typing("foobar\nx = fo");
    let end = app.buffer.len_chars();
    app.buffer.insert(end, " tail");
    app.buffer.cursor = end;
    app.refresh_suggestions();
    press(&mut app, "tab");
    assert_eq!(text_of(&app), "foobar\nx = foobar tail");
}

#[test]
fn esc_dismisses_without_changing_text_and_stays_in_insert_mode() {
    let (mut app, _) = app_with("hello help\n");
    press(&mut app, "G A");
    typed(&mut app, "he");
    assert!(app.suggest.is_some());
    press(&mut app, "esc");
    assert!(app.suggest.is_none());
    assert_eq!(text_of(&app), "hello help\nhe", "text unchanged");
    assert_eq!(app.mode_label(), "INSERT", "Esc only closed the popup");
    typed(&mut app, "l");
    assert!(app.suggest.is_some(), "the next keystroke brings it back");
}

#[test]
fn popup_closes_when_the_cursor_leaves_the_word() {
    let mut app = typing("hello help\n");
    typed(&mut app, "he");
    assert!(app.suggest.is_some());
    typed(&mut app, " ");
    assert!(app.suggest.is_none(), "a space ends the word");
    typed(&mut app, "he");
    assert!(app.suggest.is_some());
    press(&mut app, "home");
    assert!(app.suggest.is_none(), "cursor moved to the start of the line");
    press(&mut app, "end");
    assert!(app.suggest.is_none(), "moving the cursor back into a word does not reopen it; only typing does");
    typed(&mut app, "l");
    assert!(app.suggest.is_some(), "typing does");
    press(&mut app, "left");
    assert!(app.suggest.is_some(), "still inside the same word: the list just follows the shorter prefix");
    press(&mut app, "ctrl+home");
    assert!(app.suggest.is_none(), "jumping to another place closes it");
}

#[test]
fn clicking_elsewhere_closes_the_popup() {
    let mut app = typing("hello help\n");
    typed(&mut app, "he");
    render(&mut app);
    let gw = gutter_w(&app);
    click(&mut app, gw, 0);
    assert!(app.suggest.is_none(), "cursor is now at the start of `hello`");
}

#[test]
fn accepting_is_its_own_undo_step() {
    let mut app = typing("alphabet\n");
    typed(&mut app, "alp");
    press(&mut app, "tab");
    assert_eq!(text_of(&app), "alphabet\nalphabet");
    press(&mut app, "ctrl+z");
    assert_eq!(text_of(&app), "alphabet\nalp", "undo returns to the typed prefix");
    press(&mut app, "ctrl+z");
    assert_eq!(text_of(&app), "alphabet\n", "and then to before typing");
}

#[test]
fn ctrl_p_still_opens_the_command_line_when_there_is_no_popup() {
    let mut app = typing("x");
    press(&mut app, "ctrl+p");
    assert!(screen(&mut app)[23].starts_with(':'));
}

#[test]
fn suggestions_can_be_disabled_and_the_threshold_configured() {
    let mut cfg = Config::default();
    cfg.editor.modal = false;
    cfg.editor.word_suggestions = false;
    let (mut app, _) = app_cfg("hello\n", cfg);
    app.buffer.cursor = app.buffer.len_chars();
    typed(&mut app, "hel");
    assert!(app.suggest.is_none());

    let mut cfg = Config::default();
    cfg.editor.modal = false;
    cfg.editor.suggest_min_chars = 3;
    let (mut app, _) = app_cfg("hello\n", cfg);
    app.buffer.cursor = app.buffer.len_chars();
    typed(&mut app, "he");
    assert!(app.suggest.is_none());
    typed(&mut app, "l");
    assert!(app.suggest.is_some());
}

#[test]
fn popup_is_not_shown_for_a_selection_or_in_other_focus() {
    let mut app = typing("hello help\n");
    typed(&mut app, "he");
    assert!(app.suggest.is_some());
    press(&mut app, "shift+left");
    assert!(app.suggest.is_none(), "with a selection active");
    let mut app = typing("hello help\n");
    typed(&mut app, "he");
    assert!(app.suggest.is_some());
    press(&mut app, "ctrl+f");
    assert!(app.suggest.is_none(), "while the find bar has focus");
    press(&mut app, "esc");
    assert!(app.suggest.is_none(), "and it does not come back by itself");
}

#[test]
fn popup_flips_above_the_cursor_near_the_bottom_and_never_overflows() {
    let lines: String = (0..30).map(|i| format!("word{i}\n")).collect();
    let mut app = typing(&lines);
    typed(&mut app, "wor");
    for (w, h) in [(80, 24), (30, 8), (12, 6), (80, 5)] {
        let _ = render_size(&mut app, w, h);
    }
    let buf = render(&mut app);
    let rows = rows(&buf);
    assert!(rows.iter().any(|r| r.contains("word")), "popup visible somewhere");
}

#[test]
fn wide_characters_are_suggested_and_accepted() {
    let mut app = typing("日本語テキスト\n");
    typed(&mut app, "日本");
    assert_eq!(selected(&app).as_deref(), Some("日本語テキスト"));
    press(&mut app, "tab");
    assert_eq!(text_of(&app), "日本語テキスト\n日本語テキスト");
}
