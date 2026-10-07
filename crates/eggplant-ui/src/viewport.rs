//! The viewport: the window over the text. Owns vertical scroll state and
//! scroll *policy* — pure arithmetic, no buffer access. The editor surface
//! composes it; cursor movement stays with the facade (buffer state).

/// One screen row: the char segment `[start_col, start_col + width)` of
/// document line `line`. Both fitting modes (soft-wrap, horizontal scroll)
/// produce these — one rendering path below.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DisplayRow {
    pub line: usize,
    pub start_col: usize,
}

/// Rows one logical line occupies when wrapped at `width` (an empty line
/// still takes a row).
pub fn wrap_height(len: usize, width: usize) -> usize {
    len.max(1).div_ceil(width.max(1))
}

/// Display rows spanned by lines `from..=to` when wrapped at `width`.
fn visual_rows(from: usize, to: usize, width: usize, line_len: &dyn Fn(usize) -> usize) -> usize {
    (from..=to)
        .map(|line| wrap_height(line_len(line), width))
        .sum()
}

/// The scroll nearest `scroll` that reveals `cursor_line` (pure).
///
/// Nowrap: one line = one row, so any scroll in
/// `[cursor + 1 − height, cursor]` contains the cursor — `clamp` picks the
/// nearest one (i.e. scroll minimally). Wrap: the cursor's line may occupy
/// several rows, so search for the smallest scroll whose wrapped rows
/// through the cursor fit; the cursor line itself is the fallback — a
/// line taller than the screen shows its first rows.
fn vertical_reveal(
    scroll: usize,
    cursor_line: usize,
    wrap: bool,
    width: usize,
    height: usize,
    line_len: &dyn Fn(usize) -> usize,
) -> usize {
    if wrap {
        (scroll..=cursor_line)
            .find(|&s| visual_rows(s, cursor_line, width, line_len) <= height)
            .unwrap_or(cursor_line)
    } else {
        scroll.clamp((cursor_line + 1).saturating_sub(height), cursor_line)
    }
}

/// The column offset nearest `col_offset` that reveals `cursor_col`
/// (pure). Wrapped lines slide nowhere; nowrap clamps into
/// `[cursor + 1 − width, cursor]`, the same minimal-scroll rule as the
/// vertical axis.
fn horizontal_reveal(col_offset: usize, cursor_col: usize, wrap: bool, width: usize) -> usize {
    if wrap {
        0
    } else {
        col_offset.clamp((cursor_col + 1).saturating_sub(width.max(1)), cursor_col)
    }
}

/// Scroll window over the text, both axes: which document line sits at the
/// top row, which char column at the left edge (horizontal scroll, used in
/// nowrap mode only), and how many rows the screen has. All methods are
/// pure state transitions.
#[derive(Default, Clone, Copy)]
pub struct Viewport {
    /// First visible line (vertical scroll offset).
    scroll: usize,
    /// First visible char column (horizontal scroll offset; pinned to 0
    /// while wrapping — there is nothing to slide to).
    col_offset: usize,
    /// Viewport height in rows, fed by the compositor's `resize` hook
    /// (updated outside `render`, Rule 5).
    height: usize,
    /// Document generation we last saw; a change resets the scroll.
    seen_generation: usize,
}

impl Viewport {
    pub fn first_visible(&self) -> usize {
        self.scroll
    }

    pub fn col_offset(&self) -> usize {
        self.col_offset
    }

    pub fn resize(&mut self, height: usize) {
        self.height = height;
    }

    fn height(&self) -> usize {
        self.height.max(1)
    }

    /// Per-frame sync: drop per-document state when the document changed,
    /// then keep the cursor visible on both axes. The policy is two pure
    /// `reveal` functions — each computes "the offset nearest the current
    /// one that reveals the cursor"; this method only assigns.
    /// `line_len` is the seam to buffer state (DIP): the facade in
    /// production, a closure in tests.
    pub fn sync(
        &mut self,
        generation: usize,
        cursor: (usize, usize),
        wrap: bool,
        width: usize,
        line_len: &dyn Fn(usize) -> usize,
    ) {
        let (scroll, col_offset) = if generation == self.seen_generation {
            (self.scroll, self.col_offset)
        } else {
            (0, 0) // a different document: start over
        };
        self.seen_generation = generation;
        self.scroll = vertical_reveal(scroll, cursor.0, wrap, width, self.height(), line_len);
        self.col_offset = horizontal_reveal(col_offset, cursor.1, wrap, width);
    }

    /// The screen rows to paint, starting at the scroll position. Wrap
    /// mode breaks lines into `width`-sized segments; nowrap mode shows
    /// whole lines offset by `col_offset`. Always exactly `height` rows
    /// (past-EOF lines have length 0, one row each).
    pub fn layout_rows(
        &self,
        wrap: bool,
        width: usize,
        line_len: &dyn Fn(usize) -> usize,
    ) -> Vec<DisplayRow> {
        let height = self.height();
        if !wrap {
            return (self.scroll..self.scroll + height)
                .map(|line| DisplayRow {
                    line,
                    start_col: self.col_offset,
                })
                .collect();
        }
        let width = width.max(1);
        let mut rows = Vec::with_capacity(height);
        let mut line = self.scroll;
        while rows.len() < height {
            for segment in 0..wrap_height(line_len(line), width) {
                if rows.len() == height {
                    break;
                }
                rows.push(DisplayRow {
                    line,
                    start_col: segment * width,
                });
            }
            line += 1;
        }
        rows
    }

    /// `zz`: scroll so `cursor_line` sits at the vertical middle. Not
    /// clamped against EOF — `~` rows fill past the end.
    pub fn center_on(&mut self, cursor_line: usize) {
        self.scroll = cursor_line.saturating_sub(self.height() / 2);
    }

    /// `C-f`: page down. Returns the target cursor line (the caller moves
    /// the cursor via the facade, which clamps columns). The scroll clamps
    /// at `last_line` — nothing new to show beyond it.
    pub fn page_down(&mut self, cursor_line: usize, last_line: usize) -> usize {
        self.scroll = (self.scroll + self.height()).min(last_line);
        cursor_line + self.height() // facade clamps to last_line
    }

    /// `C-b`: page up.
    pub fn page_up(&mut self, cursor_line: usize) -> usize {
        self.scroll = self.scroll.saturating_sub(self.height());
        cursor_line.saturating_sub(self.height())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sync_scrolls_minimally_to_keep_the_cursor_visible() {
        let mut vp = Viewport::default();
        vp.resize(5);
        let len = &|_: usize| 3;
        vp.sync(0, (0, 0), false, 10, len);
        assert_eq!(vp.first_visible(), 0);
        vp.sync(0, (3, 0), false, 10, len); // inside: no scroll
        assert_eq!(vp.first_visible(), 0);
        vp.sync(0, (7, 0), false, 10, len); // below: scroll just enough
        assert_eq!(vp.first_visible(), 3);
        vp.sync(0, (1, 0), false, 10, len); // above: scroll up to the cursor
        assert_eq!(vp.first_visible(), 1);
    }

    #[test]
    fn horizontal_scroll_follows_the_cursor_in_nowrap() {
        let mut vp = Viewport::default();
        vp.resize(3);
        let len = &|_: usize| 100;
        vp.sync(0, (0, 0), false, 10, len);
        assert_eq!(vp.col_offset(), 0);
        vp.sync(0, (0, 25), false, 10, len); // beyond right edge: slide
        assert_eq!(vp.col_offset(), 16);
        vp.sync(0, (0, 20), false, 10, len); // inside: no slide
        assert_eq!(vp.col_offset(), 16);
        vp.sync(0, (0, 4), false, 10, len); // left of the window: slide back
        assert_eq!(vp.col_offset(), 4);
        // Wrap mode pins the offset to zero.
        vp.sync(0, (0, 25), true, 10, len);
        assert_eq!(vp.col_offset(), 0);
    }

    #[test]
    fn wrap_sync_counts_visual_rows() {
        // Lines of 25 chars at width 10 wrap to 3 rows each.
        let mut vp = Viewport::default();
        vp.resize(5);
        let len = &|_: usize| 25;
        vp.sync(0, (1, 0), true, 10, len); // lines 0..=1 = 6 rows > 5
        assert_eq!(vp.first_visible(), 1, "scrolled until the cursor fits");
        // A short cursor line fits again without scrolling further.
        vp.sync(0, (1, 0), true, 10, &|l| if l == 1 { 5 } else { 25 });
        assert_eq!(vp.first_visible(), 1);
    }

    #[test]
    fn layout_rows_nowrap_offsets_by_col() {
        let mut vp = Viewport::default();
        vp.resize(3);
        let len = &|_: usize| 100;
        vp.sync(0, (0, 25), false, 10, len);
        let rows = vp.layout_rows(false, 10, len);
        assert_eq!(rows.len(), 3);
        assert!(rows.iter().all(|row| row.start_col == vp.col_offset()));
        assert_eq!(rows[0].line, 0);
    }

    #[test]
    fn layout_rows_wrap_breaks_long_lines_and_pads_empty() {
        let mut vp = Viewport::default();
        vp.resize(4);
        // Line 0: 25 chars → 3 rows at width 10; line 1: empty → 1 row.
        let len = &|line: usize| if line == 0 { 25 } else { 0 };
        let rows = vp.layout_rows(true, 10, len);
        assert_eq!(
            rows,
            vec![
                DisplayRow {
                    line: 0,
                    start_col: 0
                },
                DisplayRow {
                    line: 0,
                    start_col: 10
                },
                DisplayRow {
                    line: 0,
                    start_col: 20
                },
                DisplayRow {
                    line: 1,
                    start_col: 0
                },
            ]
        );
        // Exact-fit line takes exactly one row; past-EOF lines fill.
        let rows = vp.layout_rows(true, 10, &|line| if line == 0 { 10 } else { 0 });
        assert_eq!(rows.len(), 4);
        assert_eq!(
            rows[1],
            DisplayRow {
                line: 1,
                start_col: 0
            }
        );
    }

    #[test]
    fn wrap_height_rounds_up_and_empty_lines_take_a_row() {
        assert_eq!(wrap_height(0, 10), 1);
        assert_eq!(wrap_height(10, 10), 1);
        assert_eq!(wrap_height(11, 10), 2);
        assert_eq!(wrap_height(25, 10), 3);
    }

    #[test]
    fn a_line_taller_than_the_screen_shows_its_first_rows() {
        // 100 chars at width 10 = 10 rows, but the window has 2: the
        // scroll must sit stably on the cursor line (the old while-loop
        // overshot to scroll = cursor + 1 and oscillated).
        let mut vp = Viewport::default();
        vp.resize(2);
        let len = &|_: usize| 100;
        vp.sync(0, (0, 0), true, 10, len);
        assert_eq!(vp.first_visible(), 0);
        vp.sync(0, (0, 95), true, 10, len);
        assert_eq!(vp.first_visible(), 0, "stable — no oscillation");
    }

    #[test]
    fn reveal_is_a_single_clamp_in_nowrap() {
        let len = &|_: usize| 3;
        // The nowrap vertical rule: scroll ∈ [cursor + 1 − height, cursor].
        assert_eq!(
            vertical_reveal(16, 20, false, 10, 5, len),
            16,
            "inside: unchanged"
        );
        assert_eq!(
            vertical_reveal(0, 7, false, 10, 5, len),
            3,
            "below: minimal"
        );
        assert_eq!(
            vertical_reveal(5, 1, false, 10, 5, len),
            1,
            "above: to cursor"
        );
        // …and the horizontal axis is the same rule on columns.
        assert_eq!(horizontal_reveal(16, 25, false, 10), 16);
        assert_eq!(horizontal_reveal(16, 4, false, 10), 4);
        assert_eq!(horizontal_reveal(16, 95, true, 10), 0, "wrap pins to 0");
    }

    #[test]
    fn a_new_document_generation_resets_the_scroll() {
        let mut vp = Viewport::default();
        vp.resize(5);
        let len = &|_: usize| 3;
        vp.sync(0, (20, 0), false, 10, len);
        assert_eq!(vp.first_visible(), 16);
        vp.sync(1, (20, 0), false, 10, len); // generation bumped
        assert_eq!(vp.first_visible(), 16, "reset, then re-reveal the cursor");
        assert_eq!(vp.seen_generation, 1);
    }

    #[test]
    fn center_clamps_at_the_top_and_centers_at_eof() {
        let mut vp = Viewport::default();
        vp.resize(5);
        vp.center_on(10);
        assert_eq!(vp.first_visible(), 8);
        vp.center_on(1);
        assert_eq!(vp.first_visible(), 0);
        vp.center_on(19); // no EOF clamp: ~ rows fill
        assert_eq!(vp.first_visible(), 17);
    }

    #[test]
    fn paging_clamps_at_both_ends() {
        let mut vp = Viewport::default();
        vp.resize(10);
        let last = 28;

        let target = vp.page_down(0, last);
        assert_eq!((vp.first_visible(), target), (10, 10));
        let target = vp.page_down(10, last);
        let target = vp.page_down(target, last);
        assert_eq!(vp.first_visible(), last, "scroll ceiling");
        assert_eq!(target, 30, "facade clamps the overshoot to last_line");

        let target = vp.page_up(last);
        assert_eq!((vp.first_visible(), target), (18, 18));
        let target = vp.page_up(target);
        let target = vp.page_up(target);
        assert_eq!((vp.first_visible(), target), (0, 0));
    }
}
