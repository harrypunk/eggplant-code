//! The viewport: the window over the text. Owns vertical scroll state and
//! scroll *policy* — pure arithmetic, no buffer access. The editor surface
//! composes it; cursor movement stays with the facade (buffer state).

/// Vertical scroll window: which document line sits at the top row, and how
/// many rows the screen has. All methods are pure state transitions.
#[derive(Default)]
pub struct Viewport {
    /// First visible line (vertical scroll offset).
    scroll: usize,
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

    pub fn resize(&mut self, height: usize) {
        self.height = height;
    }

    fn height(&self) -> usize {
        self.height.max(1)
    }

    /// Per-frame sync: drop per-document state when the document changed,
    /// then keep the cursor line inside the window (scroll minimally).
    pub fn sync(&mut self, generation: usize, cursor_line: usize) {
        if generation != self.seen_generation {
            self.seen_generation = generation;
            self.scroll = 0;
        }
        if cursor_line < self.scroll {
            self.scroll = cursor_line;
        } else if cursor_line >= self.scroll + self.height() {
            self.scroll = cursor_line + 1 - self.height();
        }
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
        vp.sync(0, 0);
        assert_eq!(vp.first_visible(), 0);
        vp.sync(0, 3); // inside: no scroll
        assert_eq!(vp.first_visible(), 0);
        vp.sync(0, 7); // below: scroll just enough
        assert_eq!(vp.first_visible(), 3);
        vp.sync(0, 1); // above: scroll up to the cursor
        assert_eq!(vp.first_visible(), 1);
    }

    #[test]
    fn a_new_document_generation_resets_the_scroll() {
        let mut vp = Viewport::default();
        vp.resize(5);
        vp.sync(0, 20);
        assert_eq!(vp.first_visible(), 16);
        vp.sync(1, 20); // generation bumped
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
