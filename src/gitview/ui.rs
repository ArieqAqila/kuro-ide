//! Drawing for `kuro git`: header, one of the four views, a footer with the keys, scrollbars. Palette colours and text styles only.

use super::cmd::Change;
use super::{GitApp, Scope, View};
use crate::render::{draw_bar, slot_style};
use crate::scrollbar;
use crate::table::{fit, slice_cols};
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};

pub struct Layout {
    pub list: Rect,
    pub vbar: Option<Rect>,
    pub hbar: Option<Rect>,
}

fn dim() -> Style {
    Style::default().add_modifier(Modifier::DIM)
}

fn bold() -> Style {
    Style::default().add_modifier(Modifier::BOLD)
}

/// The lines above the file list of a commit: hash, author, date, blank, the message (at most 6 lines).
pub fn commit_header_lines(app: &GitApp) -> Vec<String> {
    let Some(d) = &app.detail else { return vec![] };
    let c = &d.commit;
    let mut v =
        vec![format!("commit {}", c.hash), format!("Author: {} <{}>", c.author, c.email), format!("Date:   {}", c.date), String::new()];
    let msg: Vec<&str> = d.message.lines().collect();
    for l in msg.iter().take(6) {
        v.push(format!("    {l}"));
    }
    if msg.len() > 6 {
        v.push("    …".into());
    }
    v
}

fn list_len(app: &GitApp) -> usize {
    match app.view {
        View::Tree => app.rows.len(),
        View::History => app.commits.len(),
        View::Commit => app.detail.as_ref().map_or(0, |d| d.changes.len()),
        View::Diff => app.diff.len(),
        View::Changes => app.changes.len(),
    }
}

pub fn layout(app: &GitApp, area: Rect) -> Layout {
    let body = Rect::new(area.x, area.y + 1, area.width, area.height.saturating_sub(2));
    let list_area = match app.view {
        View::Commit => {
            let hdr = (commit_header_lines(app).len() as u16 + 1).min(body.height.saturating_sub(1));
            Rect::new(body.x, body.y + hdr, body.width, body.height - hdr)
        }
        _ => body,
    };
    let len = list_len(app);
    let widest = if app.view == View::Diff { app.widest_diff_line() } else { 0 };
    let (mut v, mut h) = (false, false);
    for _ in 0..3 {
        let rows = list_area.height.saturating_sub(h as u16) as usize;
        let cols = list_area.width.saturating_sub(v as u16 * scrollbar::VERTICAL_WIDTH) as usize;
        let nv = len > rows && list_area.width > 4 + scrollbar::VERTICAL_WIDTH && rows > 0;
        let nh = app.view == View::Diff && widest > cols && list_area.height > 2;
        if (nv, nh) == (v, h) {
            break;
        }
        (v, h) = (nv, nh);
    }
    let list = Rect::new(list_area.x, list_area.y, list_area.width - v as u16 * scrollbar::VERTICAL_WIDTH, list_area.height - h as u16);
    Layout {
        list,
        vbar: v.then(|| Rect::new(list.right(), list.y, scrollbar::VERTICAL_WIDTH, list.height)),
        hbar: h.then(|| Rect::new(list.x, list.bottom(), list.width, 1)),
    }
}

pub fn bars(app: &GitApp, area: Rect) -> (Option<Rect>, Option<Rect>) {
    let l = layout(app, area);
    (l.vbar, l.hbar)
}

/// The list row under a click, if there is one (the diff has none).
pub fn row_at(app: &GitApp, area: Rect, x: u16, y: u16) -> Option<usize> {
    let l = layout(app, area).list;
    if x < l.x || x >= l.right() || y < l.y || y >= l.bottom() {
        return None;
    }
    let top = match app.view {
        View::Tree => app.tree.top,
        View::History => app.history.top,
        View::Commit => app.files_scroll.top,
        View::Changes => app.changes_scroll.top,
        View::Diff => return None,
    };
    let row = top + (y - l.y) as usize;
    (row < list_len(app)).then_some(row)
}

fn status_style(app: &GitApp, status: char) -> Style {
    match status {
        'A' => slot_style(app.cfg.colors.diff_add),
        'D' => slot_style(app.cfg.colors.diff_del),
        'R' | 'C' => slot_style(app.cfg.colors.diff_hunk),
        _ => Style::default(),
    }
}

/// The colour of one line of a unified diff.
pub fn diff_style(app: &GitApp, line: &str) -> Style {
    let c = &app.cfg.colors;
    if line.starts_with("+++")
        || line.starts_with("---")
        || line.starts_with("diff ")
        || line.starts_with("index ")
        || line.starts_with("rename ")
        || line.starts_with("similarity ")
        || line.starts_with("new file")
        || line.starts_with("deleted file")
        || line.starts_with("Binary files")
    {
        let base = slot_style(c.diff_meta);
        if c.diff_meta < 0 { base.add_modifier(Modifier::BOLD) } else { base }
    } else if line.starts_with("@@") {
        slot_style(c.diff_hunk)
    } else if line.starts_with('+') {
        slot_style(c.diff_add)
    } else if line.starts_with('-') {
        slot_style(c.diff_del)
    } else {
        Style::default()
    }
}

fn put(buf: &mut Buffer, r: Rect, y: u16, text: &str, style: Style) {
    buf.set_stringn(r.x, y, fit(text, r.width as usize), r.width as usize, style);
}

fn hints(app: &GitApp) -> &'static str {
    match app.view {
        View::Tree => "↑↓ move  Enter open  a all history  c changes  q quit",
        View::History => "↑↓ move  Enter commit  d diff  c changes  Tab tree  Esc back  q quit",
        View::Commit => "↑↓ move  Enter file diff  d whole diff  o open file  n/p commit  Esc back  q quit",
        View::Diff if app.wt_diff.is_some() => "↑↓←→ scroll  n/p changed file  x discard  Esc back  q quit",
        View::Diff => "↑↓←→ scroll  n/p commit  o open file  Esc back  q quit",
        View::Changes => "↑↓ move  Enter diff  x discard  X discard all  r refresh  Esc back  q quit",
    }
}

pub fn render(app: &mut GitApp, buf: &mut Buffer, area: Rect) {
    if area.width < 10 || area.height < 4 {
        buf.set_stringn(area.x, area.y, "window too small", area.width as usize, Style::default());
        return;
    }
    let l = layout(app, area);
    // header
    let repo = app.repo_name.clone();
    let scope = match &app.scope {
        Scope::Repo => "history of the repository".to_string(),
        Scope::File(f) => format!("history of {f}"),
    };
    let title = match app.view {
        View::Tree => {
            let n = app.changes.len();
            let badge = if n == 0 { String::new() } else { format!(" · {n} change{} (c)", if n == 1 { "" } else { "s" }) };
            format!(" kuro git · {repo} · {}{badge}", app.head.label())
        }
        View::Changes => format!(" kuro git · {repo} · {} · changes of the working tree ({})", app.head.label(), app.changes.len()),
        View::History => {
            let reading = if app.loading() { format!(" · reading… {} found so far", app.commits.len()) } else { String::new() };
            format!(" kuro git · {repo} · {} · {scope}{reading}", app.head.label())
        }
        View::Commit => format!(" kuro git · {repo} · commit {}", app.detail.as_ref().map_or("", |d| d.commit.short.as_str())),
        View::Diff => format!(" kuro git · {}", app.diff_title),
    };
    put(buf, Rect::new(area.x, area.y, area.width, 1), area.y, &title, bold());

    match app.view {
        View::Tree => draw_tree(app, buf, l.list),
        View::History => draw_history(app, buf, l.list),
        View::Commit => {
            let hdr = commit_header_lines(app);
            for (i, line) in hdr.iter().enumerate() {
                let y = area.y + 1 + i as u16;
                if y < l.list.y {
                    put(buf, Rect::new(area.x, y, area.width, 1), y, line, if i == 0 { bold() } else { Style::default() });
                }
            }
            let n = app.detail.as_ref().map_or(0, |d| d.changes.len());
            let hy = l.list.y.saturating_sub(1);
            put(buf, Rect::new(area.x, hy, area.width, 1), hy, &format!("Changed files ({n})"), bold());
            draw_files(app, buf, l.list);
        }
        View::Diff => draw_diff(app, buf, l.list),
        View::Changes => draw_changes(app, buf, l.list),
    }
    if let Some(r) = l.vbar
        && let Some(b) = scrollbar::bar(r.height as usize, list_len(app), l.list.height as usize, offset_of(app))
    {
        draw_bar(buf, r, &b, true);
    }
    if let Some(r) = l.hbar
        && let Some(b) = scrollbar::bar(r.width as usize, app.widest_diff_line(), r.width as usize, app.diff_left)
    {
        draw_bar(buf, r, &b, false);
    }
    // footer
    let fy = area.bottom() - 1;
    let text = if app.msg.is_empty() { hints(app).to_string() } else { app.msg.clone() };
    put(buf, Rect::new(area.x, fy, area.width, 1), fy, &format!(" {text}"), if app.msg.is_empty() { dim() } else { bold() });
    if app.confirm.is_some() {
        draw_confirm(app, buf, area);
    }
}

fn offset_of(app: &GitApp) -> usize {
    match app.view {
        View::Tree => app.tree.top,
        View::History => app.history.top,
        View::Commit => app.files_scroll.top,
        View::Diff => app.diff_top,
        View::Changes => app.changes_scroll.top,
    }
}

fn draw_rows(buf: &mut Buffer, r: Rect, top: usize, sel: usize, n: usize, mut text: impl FnMut(usize) -> (String, Style)) {
    for i in 0..r.height as usize {
        let idx = top + i;
        if idx >= n {
            break;
        }
        let (s, style) = text(idx);
        let style = if idx == sel { style.add_modifier(Modifier::REVERSED) } else { style };
        buf.set_stringn(r.x, r.y + i as u16, fit(&s, r.width as usize), r.width as usize, style);
    }
}

fn empty_note(buf: &mut Buffer, r: Rect, text: &str) {
    buf.set_stringn(r.x + 2, r.y, text, (r.width as usize).saturating_sub(2), dim());
}

fn draw_tree(app: &GitApp, buf: &mut Buffer, r: Rect) {
    if app.rows.is_empty() {
        return empty_note(buf, r, if app.git.has_commits() { "no files" } else { "no commits yet: nothing to browse" });
    }
    draw_rows(buf, r, app.tree.top, app.tree.sel, app.rows.len(), |i| {
        let row = &app.rows[i];
        let mark = if row.dir { if row.open { "▾ " } else { "▸ " } } else { "  " };
        (
            format!(" {}{mark}{}{}", "  ".repeat(row.depth), row.name, if row.dir { "/" } else { "" }),
            if row.dir { bold() } else { Style::default() },
        )
    });
}

fn draw_history(app: &GitApp, buf: &mut Buffer, r: Rect) {
    if app.commits.is_empty() && app.loading() {
        return empty_note(buf, r, "reading the history … the commits appear as git finds them (a large repository can take a while)");
    }
    if app.commits.is_empty() {
        return empty_note(buf, r, if app.git.has_commits() { "no commits for this path" } else { "no commits yet" });
    }
    let aw = 14usize;
    draw_rows(buf, r, app.history.top, app.history.sel, app.commits.len(), |i| {
        let c = &app.commits[i];
        // in a file history, a commit that renamed the file says so before the subject, where a narrow window still shows it
        let mark = match app.followed.get(&c.hash) {
            Some(ch) if ch.old_path.is_some() => format!("[{}] ", ch.label()),
            _ => String::new(),
        };
        (format!(" {}  {}  {}  {mark}{}", c.short, c.date, fit(&c.author, aw), c.subject), Style::default())
    });
}

fn draw_files(app: &GitApp, buf: &mut Buffer, r: Rect) {
    let Some(d) = &app.detail else { return };
    if d.changes.is_empty() {
        return empty_note(buf, r, "no file changes (an empty or merge-only commit)");
    }
    draw_rows(buf, r, app.files_scroll.top, app.files_scroll.sel, d.changes.len(), |i| {
        let ch: &Change = &d.changes[i];
        (format!(" {}", ch.label()), status_style(app, ch.status))
    });
}

fn draw_changes(app: &GitApp, buf: &mut Buffer, r: Rect) {
    if app.changes.is_empty() {
        return empty_note(buf, r, "no changes: the working tree is clean");
    }
    draw_rows(buf, r, app.changes_scroll.top, app.changes_scroll.sel, app.changes.len(), |i| {
        let c = &app.changes[i];
        let style = match c.letter() {
            'A' | 'U' => slot_style(app.cfg.colors.diff_add),
            'D' => slot_style(app.cfg.colors.diff_del),
            '!' => bold(),
            _ => slot_style(app.cfg.colors.diff_hunk),
        };
        (format!(" {}", c.label()), style)
    });
}

/// The question before changes are thrown away: bold and dim only, no colour, never a background.
fn draw_confirm(app: &GitApp, buf: &mut Buffer, area: Rect) {
    use ratatui::widgets::{Block, BorderType, Clear, Widget};
    let Some(c) = &app.confirm else { return };
    let (list, title, foot) = if c.stage == 1 {
        let n = c.tracked.len();
        (
            &c.tracked,
            if n == 1 {
                format!(" Discard the changes of {}? ", c.tracked[0].path)
            } else {
                format!(" Discard the changes of {n} files? ")
            },
            "[y] discard   [n] cancel      this cannot be undone",
        )
    } else {
        let n = c.untracked.len();
        (
            &c.untracked,
            if n == 1 {
                format!(" Delete the untracked file {}? ", c.untracked[0].path)
            } else {
                format!(" Also delete {n} untracked files? ")
            },
            if c.all && !c.tracked.is_empty() {
                "[y] delete them   [n] keep them      this cannot be undone"
            } else {
                "[y] delete   [n] cancel      this cannot be undone"
            },
        )
    };
    let max_rows = (area.height as usize).saturating_sub(8).clamp(1, 12);
    let width = (area.width.saturating_sub(4) as usize).clamp(24, 100) as u16;
    let shown = list.len().min(max_rows);
    let height = (shown + 4 + (list.len() > shown) as usize) as u16;
    let rect = Rect::new(
        area.x + area.width.saturating_sub(width) / 2,
        area.y + area.height.saturating_sub(height) / 3,
        width.min(area.width),
        height.min(area.height),
    );
    Clear.render(rect, buf);
    Block::bordered().border_type(BorderType::Rounded).title(title).render(rect, buf);
    let inner = rect.width.saturating_sub(4) as usize;
    let mut y = rect.y + 1;
    for ch in list.iter().take(shown) {
        buf.set_stringn(rect.x + 2, y, fit(&ch.label(), inner), inner, Style::default());
        y += 1;
    }
    if list.len() > shown {
        buf.set_stringn(rect.x + 2, y, format!("… and {} more", list.len() - shown), inner, dim());
        y += 1;
    }
    buf.set_stringn(rect.x + 2, y + 1, fit(foot, inner), inner, bold());
}

fn draw_diff(app: &GitApp, buf: &mut Buffer, r: Rect) {
    for i in 0..r.height as usize {
        let idx = app.diff_top + i;
        let Some(line) = app.diff.get(idx) else { break };
        let style = diff_style(app, line);
        let shown = slice_cols(line, app.diff_left, r.width as usize);
        buf.set_stringn(r.x, r.y + i as u16, &shown, r.width as usize, style);
        // the words that changed inside a changed line: reverse video (never a background colour), only the part in view
        for &(s, e) in app.diff_marks.get(idx).map(|m| m.as_slice()).unwrap_or(&[]) {
            let (s, e) = (s.max(app.diff_left), e.min(app.diff_left + r.width as usize));
            if s < e {
                buf.set_style(Rect::new(r.x + (s - app.diff_left) as u16, r.y + i as u16, (e - s) as u16, 1), Modifier::REVERSED);
            }
        }
    }
}
