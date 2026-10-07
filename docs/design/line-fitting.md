# Line fitting: soft-wrap & horizontal scroll

How a long line reaches the screen. One policy, two modes — never two
features.

Code: `crates/eggplant-ui/src/viewport.rs` (policy),
`components/editor.rs` (paint), `layers/editor.rs` (container).

## The decision

Vim got this right decades ago: `wrap` and `nowrap` are one setting. A line
that doesn't fit is either **broken into display rows** (wrap) or the window
**slides horizontally** underneath it (nowrap + sidescroll). A wrap toggle
without horizontal scroll would make "off" a mode where content is
unreachable — today's bug as a user preference. So:

```
app.wrap: bool          ← the ONLY state (Rule 5; Space u w flips it)
   ├─ true   → soft-wrap: every char visible, lines cost ≥1 rows
   └─ false  → horizontal scroll: 1 line = 1 row, col_offset slides
```

## The core abstraction: `DisplayRow`

Both modes produce the same thing — a list of *display rows*, where each row
is "the char segment `[start_col, start_col + width)` of document line
`line`":

```rust
struct DisplayRow { line: usize, start_col: usize }

fn layout_rows(scroll, wrap, col_offset, width, height, line_len) -> Vec<DisplayRow>
```

- **wrap**: starting at `scroll`, each line contributes `ceil(len / width)`
  rows (minimum 1 — empty lines exist) until `height` rows are filled.
- **nowrap**: lines `scroll..scroll + height`, all with
  `start_col = col_offset`.

One pure function, one rendering path. NoWrap isn't a second code path — it's
the wrap case where every line happens to fit in one row and the segment
starts at `col_offset`.

## Who owns what (SRP)

| Piece | Owns | Doesn't own |
|---|---|---|
| `Viewport` (pure value) | scroll policy **both axes**: `scroll`, `col_offset`, visibility sync | buffer access, cursor movement |
| `layout_rows` (pure fn) | display-row derivation from state | painting |
| editor layer (container) | holds the `Viewport`, feeds it cursor/width/line-len via `resize`/`sync` | scroll arithmetic |
| `components::editor` (view) | painting rows, gutter, cursor element | deriving rows |
| command `ui.toggle-wrap` | flipping `app.wrap` + notification | viewport details |

`layout_rows` and the sync take `line_len: &dyn Fn(usize) -> usize` — the
facade in production, a closure in tests (DIP; same seam style as
`DirLister`).

## Viewport sync, both modes

Per frame (and after every key), `Viewport::sync` keeps the cursor visible:

- **Vertical, nowrap**: unchanged from today (scroll minimally).
- **Vertical, wrap**: the cursor's line may occupy several rows, so "fits"
  means counting *visual* rows from `scroll` to the cursor line; scroll down
  until they fit in `height`.
- **Horizontal, nowrap only**: keep `cursor_col` inside
  `[col_offset, col_offset + width)`, sliding minimally (vim `sidescrolloff=0`
  behavior). In wrap mode `col_offset` is pinned to 0 — there is nothing to
  slide to.

`zz` and `C-f`/`C-u` keep their current logical-line semantics in both modes
(pages move `height` *lines*, an approximation in wrap mode — honest, and
nobody measures pages in visual rows).

## Rendering: one path

The container maps `DisplayRow`s to props; the component paints:

- **text**: style the full line to per-char cells (existing `style_line`
  logic), then *slice* `[start_col..start_col+width)` — decorations
  (selection, search marks, leap labels) are computed per-char-column before
  the cut, so they survive slicing for free.
- **gutter**: first row of a logical line shows its number (or `~` past
  EOF); continuation rows show a dim `↳`, no number.
- **cursor**: find the row containing `(cursor_line, start_col ≤ col <
  start_col + width)`; screen `x = col − start_col + gutter_width`. Always
  visible, because sync guaranteed it.

## Keybinding

`Space u` — new which-key group **ui** (groups are nouns), leaf `w` →
registry command `ui.toggle-wrap`:

```
flip app.wrap → notification "wrap on" / "wrap off (horizontal scroll)"
```

Runtime toggle only; a `[ui] wrap = …` config default is a later one-liner
(same state, one more writer — open/closed).

## SOLID, concretely

- **SRP** — policy (`Viewport`/`layout_rows`), state (`App.wrap`), paint
  (component), toggle (command) each change for one reason.
- **Open/closed** — a third fitting mode (e.g. truncate-with-ellipsis) is a
  new arm in `layout_rows` + a state variant; the pipeline below doesn't
  edit. Motion semantics never touch fitting code.
- **Liskov** — `line_len: &dyn Fn(usize) -> usize` contract: total char
  length of a logical line. Facade and test closures both honor it.
- **Interface segregation** — the component needs rows + cursor, not the
  `Viewport`; the viewport never sees spans or styles.
- **Dependency inversion** — policy depends on an injected length function,
  not on the facade; `App` holds plain state.

## Testing seams

- `layout_rows`: table-driven — wrap arithmetic, empty lines, exact-fit
  lines, nowrap segment start.
- `Viewport::sync`: cursor visibility both axes, wrap-mode multi-row cursor
  line, `col_offset` pinned in wrap mode.
- Slicing: decorations (selection/search/labels) at and across the cut.
- Cursor placement: row + x for wrapped and scrolled cases.

## Deliberately out of scope (v1)

- **char ≈ cell**: tabs and full-width glyphs render with approximated
  widths (same assumption the editor already makes). A width table is a
  later, separate change.
- **`gj`/`gk`** visual-line motions: `j`/`k` stay logical (vim parity).
- Per-window wrap: the toggle is global (one editor window today).
- Smooth per-display-row scrolling of wrapped lines (`scroll` stays in
  logical lines; a wrapped line is either fully on-screen from `scroll`
  onward or not started).
