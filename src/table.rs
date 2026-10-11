//! Pure layout for the SQL result table: column widths, truncation with an
//! ellipsis, header, and bounded horizontal/vertical scrolling.

use std::collections::BTreeSet;
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

pub const SEP: &str = " │ ";
pub const SEP_WIDTH: usize = 3;
const ELLIPSIS: char = '…';

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Column {
    pub name: String,
    pub type_name: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TableOpts {
    /// Maximum visible width of the whole table, in cells.
    pub max_width: usize,
    /// Maximum number of data rows shown at once.
    pub max_rows: usize,
    /// Width above which a column is truncated (until expanded).
    pub column_max_width: usize,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TableState {
    pub row_off: usize,
    pub col_off: usize,
    pub cur_col: usize,
    pub expanded: BTreeSet<usize>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rendered {
    pub header: String,
    pub separator: String,
    pub rows: Vec<String>,
    pub row_off: usize,
    pub col_off: usize,
    pub visible_w: usize,
    pub visible_rows: usize,
    pub total_w: usize,
    pub total_rows: usize,
}

/// Newlines/tabs/control chars would break the grid; show them visibly.
pub fn sanitize(s: &str) -> String {
    s.chars()
        .map(|c| match c {
            '\n' => '⏎',
            '\t' | '\r' => ' ',
            c if c.is_control() => '·',
            c => c,
        })
        .collect()
}

pub fn display_width(s: &str) -> usize {
    UnicodeWidthStr::width(s)
}

/// Fit `s` to exactly `width` cells: truncate with `…` or pad with spaces.
pub fn fit(s: &str, width: usize) -> String {
    let w = display_width(s);
    if w <= width {
        return format!("{s}{}", " ".repeat(width - w));
    }
    if width == 0 {
        return String::new();
    }
    let mut out = String::new();
    let mut used = 0;
    for c in s.chars() {
        let cw = UnicodeWidthChar::width(c).unwrap_or(0);
        if used + cw > width - 1 {
            break;
        }
        out.push(c);
        used += cw;
    }
    out.push(ELLIPSIS);
    used += 1;
    out.push_str(&" ".repeat(width - used));
    out
}

/// Take display columns `[start, start+width)` of `s`. A wide char cut by the
/// edge becomes a space so the result is always exactly `width` cells (padded).
pub fn slice_cols(s: &str, start: usize, width: usize) -> String {
    let mut out = String::new();
    let mut x = 0;
    for c in s.chars() {
        let cw = UnicodeWidthChar::width(c).unwrap_or(0);
        let (a, b) = (x, x + cw);
        x = b;
        if b <= start {
            continue;
        }
        if a >= start + width {
            break;
        }
        if a < start || b > start + width {
            for _ in 0..(b.min(start + width) - a.max(start)) {
                out.push(' ');
            }
        } else {
            out.push(c);
        }
    }
    let w = display_width(&out);
    out.push_str(&" ".repeat(width.saturating_sub(w)));
    out
}

pub fn header_label(c: &Column) -> String {
    if c.type_name.is_empty() { sanitize(&c.name) } else { format!("{} {}", sanitize(&c.name), c.type_name) }
}

pub fn column_widths(cols: &[Column], rows: &[Vec<String>], opts: &TableOpts, st: &TableState) -> Vec<usize> {
    cols.iter()
        .enumerate()
        .map(|(i, c)| {
            let natural = rows
                .iter()
                .filter_map(|r| r.get(i))
                .map(|v| display_width(&sanitize(v)))
                .chain(std::iter::once(display_width(&header_label(c))))
                .max()
                .unwrap_or(1)
                .max(1);
            let cap = if st.expanded.contains(&i) { opts.max_width } else { opts.column_max_width };
            natural.min(cap.max(1))
        })
        .collect()
}

pub fn total_width(widths: &[usize]) -> usize {
    widths.iter().sum::<usize>() + SEP_WIDTH * widths.len().saturating_sub(1)
}

/// Display-column span `[start, end)` of every column in the unscrolled table.
pub fn col_spans(widths: &[usize]) -> Vec<(usize, usize)> {
    let mut x = 0;
    widths
        .iter()
        .map(|w| {
            let s = (x, x + w);
            x += w + SEP_WIDTH;
            s
        })
        .collect()
}

/// Which column is under display column `x` (already offset by `col_off`)?
pub fn column_at(widths: &[usize], col_off: usize, x: usize) -> Option<usize> {
    let abs = x + col_off;
    col_spans(widths).iter().position(|(s, e)| abs >= *s && abs < *e)
}

pub fn visible_width(opts: &TableOpts, viewport_w: usize) -> usize {
    viewport_w.min(opts.max_width)
}

pub fn visible_row_count(opts: &TableOpts, viewport_h: usize) -> usize {
    viewport_h.min(opts.max_rows)
}

impl TableState {
    /// Keep offsets inside the data and the current column inside the table.
    pub fn clamp(&mut self, ncols: usize, nrows: usize, widths: &[usize], vw: usize, vrows: usize) {
        self.row_off = self.row_off.min(nrows.saturating_sub(vrows));
        self.col_off = self.col_off.min(total_width(widths).saturating_sub(vw));
        self.cur_col = self.cur_col.min(ncols.saturating_sub(1));
    }

    /// Scroll horizontally just enough to show the current column (as much as fits).
    pub fn reveal_cur_col(&mut self, widths: &[usize], vw: usize) {
        let spans = col_spans(widths);
        if let Some(&(s, e)) = spans.get(self.cur_col) {
            if s < self.col_off {
                self.col_off = s;
            } else if e > self.col_off + vw {
                self.col_off = (e.saturating_sub(vw)).min(s);
            }
        }
    }

    pub fn toggle_expand(&mut self, col: usize) {
        if !self.expanded.remove(&col) {
            self.expanded.insert(col);
        }
    }
}

/// Lay out the visible window of the table.
pub fn layout(
    cols: &[Column],
    rows: &[Vec<String>],
    opts: &TableOpts,
    st: &TableState,
    viewport_w: usize,
    viewport_h: usize,
) -> Rendered {
    let widths = column_widths(cols, rows, opts, st);
    let vw = visible_width(opts, viewport_w);
    let vrows = visible_row_count(opts, viewport_h);
    let mut st = st.clone();
    st.clamp(cols.len(), rows.len(), &widths, vw, vrows);
    let total_w = total_width(&widths);

    let join = |cells: Vec<String>| cells.join(SEP);
    let header = join(cols.iter().zip(&widths).map(|(c, w)| fit(&header_label(c), *w)).collect());
    let separator = widths.iter().map(|w| "─".repeat(*w)).collect::<Vec<_>>().join("─┼─");
    let body: Vec<String> = rows
        .iter()
        .skip(st.row_off)
        .take(vrows)
        .map(|r| {
            join(
                widths
                    .iter()
                    .enumerate()
                    .map(|(i, w)| fit(&sanitize(r.get(i).map(String::as_str).unwrap_or("")), *w))
                    .collect(),
            )
        })
        .collect();

    Rendered {
        header: slice_cols(&header, st.col_off, vw),
        separator: slice_cols(&separator, st.col_off, vw),
        rows: body.iter().map(|l| slice_cols(l, st.col_off, vw)).collect(),
        row_off: st.row_off,
        col_off: st.col_off,
        visible_w: vw,
        visible_rows: vrows,
        total_w,
        total_rows: rows.len(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn col(n: &str, t: &str) -> Column {
        Column { name: n.into(), type_name: t.into() }
    }
    fn opts() -> TableOpts {
        TableOpts { max_width: 120, max_rows: 25, column_max_width: 40 }
    }
    fn rows(v: &[&[&str]]) -> Vec<Vec<String>> {
        v.iter().map(|r| r.iter().map(|s| s.to_string()).collect()).collect()
    }

    #[test]
    fn fit_pads_and_truncates() {
        assert_eq!(fit("ab", 4), "ab  ");
        assert_eq!(fit("abcdef", 4), "abc…");
        assert_eq!(fit("abcd", 4), "abcd");
        assert_eq!(fit("abc", 0), "");
        assert_eq!(fit("abc", 1), "…");
    }

    #[test]
    fn fit_respects_wide_chars() {
        // each CJK char is 2 cells wide
        assert_eq!(display_width(&fit("日本語テキスト", 7)), 7);
        assert_eq!(fit("日本語", 5), "日本…");
        assert_eq!(fit("日本語", 6), "日本語");
        assert_eq!(display_width(&fit("a😀b", 3)), 3);
    }

    #[test]
    fn sanitize_hides_control_chars() {
        assert_eq!(sanitize("a\nb\tc"), "a⏎b c");
    }

    #[test]
    fn slice_cols_basic_and_wide_edges() {
        assert_eq!(slice_cols("abcdef", 2, 3), "cde");
        assert_eq!(slice_cols("abc", 5, 3), "   ");
        assert_eq!(slice_cols("a日b", 1, 1), " ", "wide char cut on the right edge");
        assert_eq!(slice_cols("a日b", 2, 2), " b", "wide char cut on the left edge");
        assert_eq!(slice_cols("a日b", 1, 2), "日");
    }

    #[test]
    fn header_shows_name_and_type() {
        let r = layout(&[col("id", "int4"), col("name", "")], &rows(&[&["1", "x"]]), &opts(), &TableState::default(), 80, 10);
        assert!(r.header.starts_with("id int4 │ name"), "{:?}", r.header);
        assert!(r.separator.contains('┼'));
    }

    #[test]
    fn empty_result_set_still_has_header() {
        let r = layout(&[col("id", "int")], &[], &opts(), &TableState::default(), 80, 10);
        assert_eq!(r.rows.len(), 0);
        assert!(r.header.contains("id int"));
        assert_eq!(r.total_rows, 0);
    }

    #[test]
    fn no_columns_does_not_panic() {
        let r = layout(&[], &[], &opts(), &TableState::default(), 80, 10);
        assert!(r.rows.is_empty());
        assert_eq!(r.total_w, 0);
    }

    #[test]
    fn one_row_one_column() {
        let r = layout(&[col("n", "")], &rows(&[&["42"]]), &opts(), &TableState::default(), 80, 10);
        assert_eq!(r.rows.len(), 1);
        assert!(r.rows[0].starts_with("42"));
        assert_eq!(r.total_w, 2.max(1));
    }

    #[test]
    fn very_wide_column_is_truncated_then_expandable() {
        let long = "x".repeat(300);
        let data = rows(&[&[long.as_str(), "b"]]);
        let cols = [col("a", ""), col("b", "")];
        let mut st = TableState::default();
        let w = column_widths(&cols, &data, &opts(), &st);
        assert_eq!(w[0], 40, "capped at column_max_width");
        let r = layout(&cols, &data, &opts(), &st, 200, 10);
        assert!(r.rows[0].starts_with(&format!("{}…", "x".repeat(39))), "{}", r.rows[0]);
        st.toggle_expand(0);
        let w = column_widths(&cols, &data, &opts(), &st);
        assert_eq!(w[0], 120, "expanded columns are bounded by table max width");
        st.toggle_expand(0);
        assert_eq!(column_widths(&cols, &data, &opts(), &st)[0], 40, "toggle collapses again");
    }

    #[test]
    fn visible_width_never_exceeds_max_width() {
        let long = "y".repeat(100);
        let data = rows(&[&[long.as_str(), long.as_str(), long.as_str()]]);
        let cols = [col("a", ""), col("b", ""), col("c", "")];
        let r = layout(&cols, &data, &opts(), &TableState::default(), 500, 10);
        assert_eq!(r.visible_w, 120);
        for line in std::iter::once(&r.header).chain(&r.rows) {
            assert_eq!(display_width(line), 120);
        }
    }

    #[test]
    fn narrow_viewport_wins_over_max_width() {
        let r = layout(&[col("a", "")], &rows(&[&["hello"]]), &opts(), &TableState::default(), 3, 10);
        assert_eq!(r.visible_w, 3);
        assert_eq!(display_width(&r.rows[0]), 3);
    }

    #[test]
    fn at_most_max_rows_visible() {
        let data: Vec<Vec<String>> = (0..100).map(|i| vec![i.to_string()]).collect();
        let r = layout(&[col("n", "")], &data, &opts(), &TableState::default(), 80, 60);
        assert_eq!(r.rows.len(), 25);
        let small = TableOpts { max_rows: 5, ..opts() };
        assert_eq!(layout(&[col("n", "")], &data, &small, &TableState::default(), 80, 60).rows.len(), 5);
        assert_eq!(layout(&[col("n", "")], &data, &opts(), &TableState::default(), 80, 3).rows.len(), 3, "viewport smaller than max_rows");
    }

    #[test]
    fn vertical_scroll_and_clamp() {
        let data: Vec<Vec<String>> = (0..100).map(|i| vec![i.to_string()]).collect();
        let st = TableState { row_off: 10, ..Default::default() };
        let r = layout(&[col("n", "")], &data, &opts(), &st, 80, 20);
        assert_eq!(r.rows[0].trim(), "10");
        assert_eq!(r.rows.len(), 20);
        let st = TableState { row_off: 9999, ..Default::default() };
        let r = layout(&[col("n", "")], &data, &opts(), &st, 80, 20);
        assert_eq!(r.row_off, 80, "clamped so the last page is full");
        assert_eq!(r.rows.last().unwrap().trim(), "99");
    }

    #[test]
    fn horizontal_scroll_and_clamp() {
        let cols: Vec<Column> = (0..6).map(|i| col(&format!("c{i}"), "")).collect();
        let data = vec![(0..6).map(|i| format!("{}", i).repeat(20)).collect::<Vec<_>>()];
        let st = TableState { col_off: 25, ..Default::default() };
        let r = layout(&cols, &data, &opts(), &st, 40, 10);
        assert_eq!(r.col_off, 25);
        assert_eq!(display_width(&r.rows[0]), 40);
        let st = TableState { col_off: 99999, ..Default::default() };
        let r = layout(&cols, &data, &opts(), &st, 40, 10);
        assert_eq!(r.col_off, r.total_w - 40, "cannot scroll past the right edge");
        assert!(r.rows[0].ends_with('5'));
    }

    #[test]
    fn header_stays_aligned_with_scrolled_rows() {
        let cols = [col("aaaa", ""), col("bbbb", "")];
        let data = rows(&[&["1111", "2222"]]);
        let st = TableState { col_off: 7, ..Default::default() };
        let r = layout(&cols, &data, &opts(), &st, 4, 10);
        assert_eq!(r.header, "bbbb");
        assert_eq!(r.rows[0], "2222");
    }

    #[test]
    fn unicode_cells_keep_columns_aligned() {
        let cols = [col("k", ""), col("v", "")];
        let data = rows(&[&["日本", "a"], &["x", "b"]]);
        let r = layout(&cols, &data, &opts(), &TableState::default(), 80, 10);
        assert_eq!(display_width(&r.rows[0]), display_width(&r.rows[1]));
        assert_eq!(display_width(&r.header), display_width(&r.rows[0]));
    }

    #[test]
    fn column_navigation_reveals_current_column() {
        let widths = [10, 10, 10];
        let mut st = TableState { cur_col: 2, ..Default::default() };
        st.reveal_cur_col(&widths, 15);
        assert_eq!(st.col_off, 36 - 15, "right edge of col 2 aligned to the right");
        st.cur_col = 0;
        st.reveal_cur_col(&widths, 15);
        assert_eq!(st.col_off, 0);
    }

    #[test]
    fn column_at_maps_x_to_columns() {
        let widths = [4, 4];
        assert_eq!(column_at(&widths, 0, 0), Some(0));
        assert_eq!(column_at(&widths, 0, 3), Some(0));
        assert_eq!(column_at(&widths, 0, 4), None, "separator");
        assert_eq!(column_at(&widths, 0, 7), Some(1));
        assert_eq!(column_at(&widths, 5, 2), Some(1), "respects scroll offset");
    }
}
