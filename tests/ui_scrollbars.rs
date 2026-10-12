//! Vertical and horizontal scrollbars (task Feature 1): geometry on screen, clicking, dragging, hiding, colours.

mod common;
use common::ui::*;
use crossterm::event::{MouseButton, MouseEventKind};
use kuro::app::App;
use kuro::config::Config;
use ratatui::buffer::Buffer as Cells;
use ratatui::style::{Color, Modifier};

const BIG: u16 = 120;

fn lines(n: usize) -> String {
    (0..n).map(|i| format!("line {i}\n")).collect()
}

fn wide(cols: usize) -> String {
    format!("{}\nshort\n", "w".repeat(cols))
}

/// (arrow-start glyph present, thumb cells, arrow-end glyph present) for the vertical bar at column `x`.
fn vthumb(buf: &Cells, x: u16, rows: u16) -> Vec<u16> {
    (0..rows).filter(|&y| buf[(x, y)].modifier.contains(Modifier::REVERSED)).collect()
}

fn hthumb(buf: &Cells, y: u16, from: u16, to: u16) -> Vec<u16> {
    (from..to).filter(|&x| buf[(x, y)].modifier.contains(Modifier::REVERSED)).collect()
}

fn click_at(a: &mut App, x: u16, y: u16) {
    mouse(a, MouseEventKind::Down(MouseButton::Left), x, y);
    mouse(a, MouseEventKind::Up(MouseButton::Left), x, y);
}

#[test]
fn hidden_when_the_content_fits() {
    let (mut a, _) = app_with(&lines(10));
    let s = screen(&mut a);
    for row in &s[..22] {
        assert!(!row.contains('▲') && !row.contains('▼') && !row.contains('◀') && !row.contains('▶') && !row.contains('│'), "{row:?}");
    }
    let l = a.layout(ratatui::layout::Rect::new(0, 0, W, H));
    assert!(l.vbar.is_none() && l.hbar.is_none());
    assert_eq!(l.text.width, W, "the text uses the full width when nothing scrolls");
}

#[test]
fn a_vertical_bar_appears_when_there_are_more_lines_than_rows_and_goes_away_again() {
    let (mut a, _) = app_with(&lines(100));
    let buf = render(&mut a);
    assert_eq!(buf[(W - 1, 0)].symbol(), "▲");
    assert_eq!(buf[(W - 1, 21)].symbol(), "▼");
    assert_eq!(buf[(W - 1, 10)].symbol(), "│", "track");
    // delete almost everything: it fits again and the bar is gone
    a.buffer.remove(0..a.buffer.len_chars() - 5);
    let buf = render(&mut a);
    assert_eq!(buf[(W - 1, 0)].symbol(), " ");
}

#[test]
fn thumb_size_and_position_follow_the_content_and_the_viewport() {
    // 80x24: 22 text rows, so a 22-cell bar with 20 track cells; 100 lines -> thumb ceil(20 * 22 / 100) = 5 cells
    let (mut a, _) = app_with(&lines(100));
    let buf = render(&mut a);
    assert_eq!(vthumb(&buf, W - 1, 22), vec![1, 2, 3, 4, 5], "thumb at the top, right under the arrow");
    // scrolled to the end: the thumb touches the bottom of the track
    a.top_line = 79; // 101 lines (the text ends in a newline) minus 22 rows
    a.follow = false;
    let buf = render(&mut a);
    assert_eq!(vthumb(&buf, W - 1, 22), vec![16, 17, 18, 19, 20]);
    // halfway
    a.top_line = 39;
    let buf = render(&mut a);
    let t = vthumb(&buf, W - 1, 22);
    assert_eq!(t.len(), 5);
    assert!((8..=10).contains(&t[0]), "middle of the track: {t:?}");
    // a longer file gives a smaller thumb
    let (mut b, _) = app_with(&lines(1000));
    let buf = render(&mut b);
    assert_eq!(vthumb(&buf, W - 1, 22).len(), 1, "never smaller than one cell");
}

#[test]
fn the_horizontal_bar_has_its_own_geometry() {
    let (mut a, _) = app_with(&wide(200));
    let buf = render(&mut a);
    let gw = gutter_w(&a);
    // the bar sits on the last text row, after the gutter, and is as wide as the text
    let y = 21; // 22 editor rows: the bottom one
    assert_eq!(buf[(gw, y)].symbol(), "◀");
    assert_eq!(buf[(W - 1, y)].symbol(), "▶");
    let avail = (W - gw) as usize;
    let track = avail - 2;
    let thumb = hthumb(&buf, y, gw, W);
    assert_eq!(thumb.len(), (track * avail).div_ceil(200), "visible fraction of the longest line");
    assert_eq!(thumb[0], gw + 1, "at the start");
    // scrolled to the end
    a.left_col = 200 - avail;
    a.follow = false;
    let buf = render(&mut a);
    let thumb = hthumb(&buf, y, gw, W);
    assert_eq!(*thumb.last().unwrap(), W - 2, "the thumb touches the right arrow");
}

#[test]
fn both_bars_share_the_corner_and_shrink_the_text_area() {
    let text = format!("{}{}", lines(60), "z".repeat(150));
    let (mut a, _) = app_with(&text);
    render(&mut a);
    let l = a.layout(ratatui::layout::Rect::new(0, 0, W, H));
    let (v, h) = (l.vbar.unwrap(), l.hbar.unwrap());
    assert_eq!((v.x, v.height), (W - 1, l.text.height));
    assert_eq!(h.y, l.text.bottom());
    assert_eq!(l.text.width, W - 1);
    assert_eq!(l.text.height, 21);
    assert!(h.right() < W, "the horizontal bar stops before the vertical one");
}

#[test]
fn clicking_the_arrows_steps_one_line_or_column() {
    let (mut a, _) = app_with(&lines(100));
    render(&mut a);
    click_at(&mut a, W - 1, 21); // down arrow
    assert_eq!(a.top_line, 1);
    click_at(&mut a, W - 1, 21);
    assert_eq!(a.top_line, 2);
    click_at(&mut a, W - 1, 0); // up arrow
    assert_eq!(a.top_line, 1);
    let (mut b, _) = app_with(&wide(200));
    render(&mut b);
    let gw = gutter_w(&b);
    click_at(&mut b, W - 1, 21);
    assert_eq!(b.left_col, 1);
    click_at(&mut b, gw, 21);
    assert_eq!(b.left_col, 0);
}

#[test]
fn clicking_the_track_pages_and_stops_at_the_ends() {
    let (mut a, _) = app_with(&lines(100));
    render(&mut a);
    click_at(&mut a, W - 1, 15); // track below the thumb
    assert_eq!(a.top_line, 21, "one page (rows - 1)");
    click_at(&mut a, W - 1, 20);
    assert_eq!(a.top_line, 42);
    for _ in 0..10 {
        render(&mut a);
        click_at(&mut a, W - 1, 20);
    }
    assert_eq!(a.top_line, 101 - 22, "clamped: the last line is at the bottom, not beyond (101 lines)");
    for _ in 0..10 {
        render(&mut a);
        click_at(&mut a, W - 1, 1);
    }
    assert_eq!(a.top_line, 0, "and at the start");
    // the horizontal track pages by the width of the text
    let (mut b, _) = app_with(&wide(300));
    render(&mut b);
    let gw = gutter_w(&b);
    click_at(&mut b, W - 3, 21);
    assert_eq!(b.left_col, (W - gw) as usize - 1);
}

#[test]
fn dragging_the_thumb_scrolls_proportionally_and_clamps() {
    let (mut a, _) = app_with(&lines(100));
    render(&mut a);
    // grab the thumb on its first cell (row 1) and drag down to the bottom of the track
    mouse(&mut a, MouseEventKind::Down(MouseButton::Left), W - 1, 1);
    mouse(&mut a, MouseEventKind::Drag(MouseButton::Left), W - 1, 11);
    let mid = a.top_line;
    assert!(mid > 20 && mid < 60, "ten of fifteen free cells: about two thirds of 79, got {mid}");
    mouse(&mut a, MouseEventKind::Drag(MouseButton::Left), W - 1, 21);
    assert_eq!(a.top_line, 79, "the thumb is at the bottom: the last page");
    mouse(&mut a, MouseEventKind::Drag(MouseButton::Left), W - 1, 40); // far below the bar
    assert_eq!(a.top_line, 79, "clamped");
    mouse(&mut a, MouseEventKind::Drag(MouseButton::Left), W - 1, 0);
    assert_eq!(a.top_line, 0, "dragged back above the bar: the start");
    mouse(&mut a, MouseEventKind::Up(MouseButton::Left), W - 1, 0);
    // after the release the pointer no longer moves the thumb
    mouse(&mut a, MouseEventKind::Drag(MouseButton::Left), W - 1, 15);
    assert_eq!(a.top_line, 0);
    // the screen follows the drag
    mouse(&mut a, MouseEventKind::Down(MouseButton::Left), W - 1, 1);
    mouse(&mut a, MouseEventKind::Drag(MouseButton::Left), W - 1, 6);
    let top = a.top_line;
    assert!(screen(&mut a)[0].contains(&format!("line {top}")));
}

#[test]
fn dragging_the_horizontal_thumb() {
    let (mut a, _) = app_with(&wide(200));
    render(&mut a);
    let gw = gutter_w(&a);
    mouse(&mut a, MouseEventKind::Down(MouseButton::Left), gw + 1, 21);
    mouse(&mut a, MouseEventKind::Drag(MouseButton::Left), W - 2, 21);
    assert_eq!(a.left_col, 200 - (W - gw) as usize, "dragged to the end of the longest line");
    mouse(&mut a, MouseEventKind::Drag(MouseButton::Left), 0, 21);
    assert_eq!(a.left_col, 0);
}

#[test]
fn grabbing_the_thumb_in_the_middle_does_not_make_it_jump() {
    let (mut a, _) = app_with(&lines(100));
    render(&mut a);
    a.top_line = 39;
    a.follow = false;
    let buf = render(&mut a);
    let t = vthumb(&buf, W - 1, 22);
    let mid = t[2];
    mouse(&mut a, MouseEventKind::Down(MouseButton::Left), W - 1, mid);
    assert_eq!(a.top_line, 39, "pressing on the thumb alone changes nothing");
    mouse(&mut a, MouseEventKind::Drag(MouseButton::Left), W - 1, mid);
    assert_eq!(a.top_line, 39);
}

#[test]
fn keyboard_navigation_keeps_the_bar_in_sync() {
    let (mut a, _) = modeless(&lines(100));
    let buf = render(&mut a);
    assert_eq!(vthumb(&buf, W - 1, 22)[0], 1);
    press(&mut a, "ctrl+end");
    let buf = render(&mut a);
    let t = vthumb(&buf, W - 1, 22);
    assert_eq!(*t.last().unwrap(), 20, "at the bottom after Ctrl+End");
    press(&mut a, "ctrl+home");
    let buf = render(&mut a);
    assert_eq!(vthumb(&buf, W - 1, 22)[0], 1);
    // typing and the wheel too
    mouse(&mut a, MouseEventKind::ScrollDown, 10, 5);
    let buf = render(&mut a);
    assert!(vthumb(&buf, W - 1, 22)[0] >= 1);
    // horizontally: End on a long line
    let (mut b, _) = modeless(&wide(300));
    render(&mut b);
    press(&mut b, "end");
    let buf = render(&mut b);
    let gw = gutter_w(&b);
    assert_eq!(*hthumb(&buf, 21, gw, W).last().unwrap(), W - 2);
}

#[test]
fn wheel_events_over_the_bars_scroll_the_text() {
    let (mut a, _) = app_with(&lines(100));
    render(&mut a);
    mouse(&mut a, MouseEventKind::ScrollDown, W - 1, 10);
    assert_eq!(a.top_line, 3);
}

#[test]
fn it_works_in_insert_mode_and_not_when_mouse_support_is_off() {
    let (mut a, _) = modeless(&lines(100));
    render(&mut a);
    click_at(&mut a, W - 1, 21);
    assert_eq!(a.top_line, 1);
    let mut cfg = Config::default();
    cfg.editor.mouse = false;
    let (mut b, _) = app_cfg(&lines(100), cfg);
    let buf = render(&mut b);
    assert_eq!(buf[(W - 1, 0)].symbol(), "▲", "still drawn");
    click_at(&mut b, W - 1, 21);
    assert_eq!(b.top_line, 0, "but the mouse is off");
}

#[test]
fn it_can_be_turned_off() {
    let mut cfg = Config::default();
    cfg.editor.scrollbars = false;
    let (mut a, _) = app_cfg(&format!("{}{}", lines(100), "q".repeat(300)), cfg);
    let l = a.layout(ratatui::layout::Rect::new(0, 0, W, H));
    assert!(l.vbar.is_none() && l.hbar.is_none());
    let s = screen(&mut a);
    assert!(!s.iter().any(|r| r.contains('▲')));
}

#[test]
fn colours_follow_the_terminal_theme_and_there_is_never_a_background() {
    let text = format!("{}{}", lines(80), "z".repeat(200));
    let (mut a, _) = app_with(&text);
    let buf = render_size(&mut a, BIG, 40);
    for y in 0..40 {
        for x in 0..BIG {
            let c = &buf[(x, y)];
            assert_eq!(c.bg, Color::Reset, "no background at ({x},{y})");
            assert!(!matches!(c.fg, Color::Rgb(..) | Color::Indexed(_)), "({x},{y}) {:?}", c.fg);
        }
    }
    // thumb and track differ by reverse video and dim, not by colour
    let thumb = &buf[(BIG - 1, 2)];
    assert!(thumb.modifier.contains(Modifier::REVERSED));
    let track = &buf[(BIG - 1, 30)];
    assert!(track.modifier.contains(Modifier::DIM) && !track.modifier.contains(Modifier::REVERSED));
}

#[test]
fn tiny_windows_do_not_panic() {
    for (w, h) in [(1, 1), (3, 3), (5, 4), (10, 5), (20, 3)] {
        let (mut a, _) = app_with(&format!("{}{}", lines(50), "z".repeat(100)));
        render_size(&mut a, w, h);
        mouse(&mut a, MouseEventKind::Down(MouseButton::Left), w.saturating_sub(1), 0);
        mouse(&mut a, MouseEventKind::Drag(MouseButton::Left), w.saturating_sub(1), h);
        mouse(&mut a, MouseEventKind::Up(MouseButton::Left), w.saturating_sub(1), h);
        render_size(&mut a, w, h);
    }
}
