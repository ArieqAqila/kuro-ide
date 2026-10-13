//! Scrollbar geometry: pure functions, no drawing and no app state.
//!
//! A bar is a strip of `len` cells: the *track*, with a *thumb* whose length is the visible fraction of the content
//! (no arrow buttons: the wheel and the keys step, the track pages, the thumb drags).
//! Everything is in cells, so the same code serves the vertical and the horizontal bar.

/// Columns the vertical bar is wide: one, drawn with thin line glyphs (the thumb is a heavier line), so it stays slim.
pub const VERTICAL_WIDTH: u16 = 1;

/// Where a point on a bar is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Part {
    /// The track before the thumb: page back.
    BeforeThumb,
    /// The track after the thumb: page forward.
    AfterThumb,
    /// The thumb itself, `grab` cells from its start.
    Thumb { grab: usize },
}

/// Geometry of one bar for a given content, viewport and scroll offset.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Bar {
    /// Cells in the whole strip (all of it is track).
    pub len: usize,
    pub track_len: usize,
    /// Thumb start (within the track) and length.
    pub thumb_start: usize,
    pub thumb_len: usize,
    /// The largest scroll offset (`content - viewport`).
    pub max_offset: usize,
}

/// The scrollbar for `content` units of text seen through a window of `viewport` units, scrolled `offset` units from the
/// start, drawn in a strip of `len` cells. `None` when everything fits (the bar is hidden) or there is no room.
pub fn bar(len: usize, content: usize, viewport: usize, offset: usize) -> Option<Bar> {
    if len == 0 || viewport == 0 || content <= viewport {
        return None;
    }
    let track_len = len;
    let max_offset = content - viewport;
    // thumb length: the visible fraction of the content, at least one cell, at most the track
    let thumb_len = ((track_len * viewport).div_ceil(content)).clamp(1, track_len);
    let room = track_len - thumb_len;
    let offset = offset.min(max_offset);
    // rounded to the nearest cell, so the thumb touches both ends exactly at offset 0 and max_offset
    let thumb_start = (room * offset + max_offset / 2) / max_offset;
    Some(Bar { len, track_len, thumb_start, thumb_len, max_offset })
}

impl Bar {
    /// What is at cell `pos` of the strip (0 = first cell).
    pub fn part_at(&self, pos: usize) -> Part {
        let t = pos;
        if t < self.thumb_start {
            Part::BeforeThumb
        } else if t < self.thumb_start + self.thumb_len {
            Part::Thumb { grab: t - self.thumb_start }
        } else {
            Part::AfterThumb
        }
    }

    /// The scroll offset that puts the thumb's start at track cell `thumb_start` (used while dragging).
    pub fn offset_for_thumb_start(&self, thumb_start: usize) -> usize {
        let room = self.track_len - self.thumb_len;
        if room == 0 {
            return 0;
        }
        let s = thumb_start.min(room);
        (s * self.max_offset + room / 2) / room
    }

    /// The offset for a drag that has the pointer at strip cell `pos`, holding the thumb `grab` cells from its start.
    pub fn offset_for_pointer(&self, pos: isize, grab: usize) -> usize {
        let start = pos - grab as isize;
        self.offset_for_thumb_start(start.max(0) as usize)
    }

    /// Which strip cells are thumb (relative to the strip).
    pub fn thumb_cells(&self) -> std::ops::Range<usize> {
        let s = self.thumb_start;
        s..s + self.thumb_len
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hidden_when_the_content_fits_or_there_is_no_room() {
        assert_eq!(bar(20, 10, 10, 0), None, "exactly fits");
        assert_eq!(bar(20, 5, 10, 0), None, "less than the viewport");
        assert_eq!(bar(0, 100, 10, 0), None);
        assert_eq!(bar(20, 100, 0, 0), None);
    }

    #[test]
    fn thumb_length_is_the_visible_fraction_of_the_track() {
        // the whole strip is track: 20 cells; half the content visible: 10 cells
        let b = bar(20, 100, 50, 0).unwrap();
        assert_eq!((b.track_len, b.thumb_len), (20, 10));
        // a tenth visible: 2 cells
        assert_eq!(bar(20, 1000, 100, 0).unwrap().thumb_len, 2);
        // never smaller than one cell, never larger than the track
        assert_eq!(bar(20, 1_000_000, 10, 0).unwrap().thumb_len, 1);
        assert_eq!(bar(20, 101, 100, 0).unwrap().thumb_len, 20);
    }

    #[test]
    fn thumb_position_is_proportional_and_touches_both_ends() {
        let top = bar(22, 200, 20, 0).unwrap();
        assert_eq!(top.thumb_start, 0);
        let end = bar(22, 200, 20, 180).unwrap();
        assert_eq!(end.thumb_start + end.thumb_len, end.track_len, "at the maximum the thumb touches the end");
        let mid = bar(22, 200, 20, 90).unwrap();
        let room = mid.track_len - mid.thumb_len;
        assert_eq!(mid.thumb_start, room.div_ceil(2), "halfway, rounded to the nearest cell (9.5 -> 10)");
        // offsets past the end clamp
        assert_eq!(bar(22, 200, 20, 9999).unwrap(), end);
    }

    #[test]
    fn a_one_cell_strip_still_has_a_thumb() {
        let b = bar(1, 100, 10, 0).unwrap();
        assert_eq!((b.track_len, b.thumb_len), (1, 1));
    }

    #[test]
    fn parts_are_found_by_position() {
        let b = bar(22, 200, 20, 90).unwrap();
        assert_eq!(b.part_at(0), Part::BeforeThumb, "the first cell is track, not a button");
        assert_eq!(b.part_at(21), Part::AfterThumb, "and so is the last");
        let t = b.thumb_cells();
        assert_eq!(b.part_at(t.start), Part::Thumb { grab: 0 });
        assert_eq!(b.part_at(t.end - 1), Part::Thumb { grab: b.thumb_len - 1 });
        assert_eq!(b.part_at(t.start - 1), Part::BeforeThumb);
        assert_eq!(b.part_at(t.end), Part::AfterThumb);
    }

    #[test]
    fn dragging_maps_the_pointer_back_to_an_offset_and_clamps() {
        let b = bar(22, 200, 20, 0).unwrap();
        assert_eq!(b.offset_for_pointer(0, 0), 0, "the first cell is the start of the track");
        assert_eq!(b.offset_for_pointer(-5, 0), 0, "above the bar");
        assert_eq!(b.offset_for_pointer(500, 0), b.max_offset, "far below the bar");
        let room = b.track_len - b.thumb_len;
        assert_eq!(b.offset_for_thumb_start(room), b.max_offset);
        // holding the thumb in the middle: the pointer at the thumb's middle keeps the offset
        let b = bar(22, 200, 20, 90).unwrap();
        let grab = b.thumb_len / 2;
        let pos = (b.thumb_start + grab) as isize;
        let back = b.offset_for_pointer(pos, grab);
        assert!(back.abs_diff(90) <= b.max_offset.div_ceil(b.track_len - b.thumb_len), "round trip within one cell: {back}");
    }

    #[test]
    fn a_full_track_thumb_cannot_move() {
        let b = bar(10, 11, 10, 1).unwrap();
        assert_eq!(b.thumb_len, b.track_len);
        assert_eq!(b.offset_for_thumb_start(5), 0);
    }
}
