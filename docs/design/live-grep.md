# Live grep: project search with preview

`Space s p` — search the whole workspace as you type, with a preview pane
showing the selected match in context.

Code: `crates/eggplant-ui/src/project_grep.rs` (searcher),
`layers/picker.rs` (the generalized container), `components/picker.rs` +
`components/preview.rs` (views).

## UX

```
┌─ project grep ────────────────────────────────────────────────┐
│ > config           │ src/config.rs                            │
│  :12  let mut cfg  │   10 │ use std::fs;                      │
│  :48  config.apply │   11 │                                   │
│  :51  // config..  │ > 12 │ let mut cfg = Config::load();     │
│                    │   13 │                                   │
│                    │   14 │ cfg.apply(&mut app);              │
└────────────────────┴──────────────────────────────────────────┘
  left: input + hit list (file:line + matched text)
  right: preview — the selected hit's file, a few lines of context,
         match row and columns highlighted
```

Typing re-searches the workspace; moving the selection re-previews.
`Enter` opens the file at the hit (`CloseUnfocus` — focus follows the
cursor, same rule as buffer grep).

## The pipeline, and where Rule 5 draws the line

```
keystroke ──► re-run query (fs I/O) ──► items: Vec<GrepHit>     ─┐ event time
selection ──► re-materialize preview (fs I/O) ──► PreviewProps  ─┘ (I/O allowed)
                                     │
view() ──► render input + list + preview   ◄── pure, f(state), no I/O, ever
```

Both I/O steps live in `handle_key` (input change, selection move) — the
same rule as everywhere else in the codebase: **state changes in event
handlers, views only read**. The preview is *materialized state*, not
computed during paint.

## Generalizing the picker (open/closed)

Today's picker is "static list + fuzzy filter". Live grep is neither: its
items are *derived* by a query. So the picker source becomes a closed
two-variant type:

```rust
enum PickerSource<T> {
    /// Static items + fuzzy filter (palette, buffer grep, file picker).
    List { items: Vec<T>, text_of: fn(&T) -> &str },
    /// Live derivation: input text → items (project grep).
    Query { run: fn(&str, &App) -> Vec<T> },
}
```

`List` keeps today's behavior exactly. `Query` re-materializes `items`
whenever the input changes (min. 2 chars — below that the list is empty,
like leap); selection/clamping/rendering are shared. Nothing about the
existing three pickers changes semantically.

Preview is a second, orthogonal seam on the spec:

```rust
preview_of: Option<fn(&T, &App) -> PreviewProps>
```

When present, the container materializes the preview for the selected item
after every items-change or selection-move, and the view splits into
list | preview. Project grep supplies a file-reading preview; buffer grep
could supply a buffer-reading one later, the file picker a file head —
**new previews are new function pointers, not edits** (open/closed).

## The searcher

`project_grep.rs` — one module, one job: "find matches in the workspace
and describe their context".

- **File set**: `Workspace::collect_files` — the *same* walker as the file
  picker (respects `.gitignore`, hidden skip, `[files] ignore`, 20k cap).
  One source of truth for "which files are the project".
- **Pattern**: the `regex` crate, rg-style ergonomics:
  - unparseable as regex → search literally (`regex::escape`)
  - smart case: an all-lowercase pattern is case-insensitive
- **Per file**: skip if > 1 MiB, skip if the first 8 KB contain NUL
  (binary sniff, rg's heuristic), then per-line `find_iter`.
- **Caps** (sync searching stays responsive): pattern < 2 chars → no
  search; 500 hits max.

The pure core is split from the fs shell:

```rust
grep_text(pattern, text) -> Vec<(line, col, end)>   // pure, unit-tested
search_workspace(ws, pattern) -> Vec<GrepHit>       // walk + read + core
preview(hit, context) -> PreviewProps               // read around hit.line
```

## Preview: a peek, not hand-built text

The preview is a **read-only core document** (`core::peek::Peek`): opened
through the same backend loader + language detection as buffers, but never
in the buffer list — no cursor, no history. The layer materializes
`PreviewProps` from `peek.highlighted_line(line)` per context row, so the
preview pane renders through **the editor's own styling pipeline**
(`components::editor::style_cells`) — syntax highlighting is free, and
there is exactly one styling implementation in the codebase. Files that
fail to open fall back to plain context lines
(`core::peek::context_lines`, unscoped spans).

```rust
struct PreviewProps {
    title: String,          // "src/config.rs:12"
    first_line: usize,      // gutter numbering base
    rows: Vec<PreviewRow>,  // spans + match band
                            // scroll = hit − 5, rows = pane capacity
    focus_row: usize,       // hit row (number accented)
}
```

The picker composes it: preview present → tall split
`[40% list │ 60% preview]`; absent → the narrow float.

**The preview is a viewport, not a window of text**: its state is
`(Peek, scroll)` where `scroll = hit − 5`. The pane's row budget comes
from one shared layout formula (`components::picker::preview_budget`) —
the view builds its constraints from it, the container derives exactly
that many rows from the scroll. No arbitrary cap; a future "scroll the
preview" key just moves `scroll`.

## SOLID, concretely

- **SRP** — searcher (fs + regex), picker source (item derivation),
  preview materialization (container), preview painting (component) each
  change for one reason.
- **Open/closed** — new live pickers = a `Query` fn; new previews = a
  `preview_of` fn. The picker container and views don't edit.
- **Liskov** — `run`/`preview_of` contracts: pure functions of
  (input/item, App) → data; no layer/compositor access.
- **Interface segregation** — `PreviewProps` is exactly what the preview
  view needs; the picker list never sees file content.
- **Dependency inversion** — the searcher depends on `Workspace`'s
  abstraction of "project files", not on walking itself; the pure core
  (`grep_text`) is injectable-tested without fs.

## Testing seams

- `grep_text`: regex/literal fallback, smart case, multi-match lines.
- Caps & sniffing: big file, binary file, hit cap — temp-dir integration.
- `Query` source: stub `run` fn; input change re-derives, selection
  clamps, min-pattern gate.
- Preview materialization: temp file, hit near top of file (context
  clamps at 0).
- Preview component: paint test (numbers, band, match cols) via
  `TestBackend`.

## Deliberately out of scope (v1)

- **Async streaming**: search runs synchronously in `handle_key`. With
  the caps above it's fine at this repo's scale; if it ever janks, the
  seam is the `Query` fn — a channel-fed source is a new implementation,
  not a refactor.
- Preview scrolling (the context window is fixed ±3).
- Replacement (`s r`): the hit list is the hard part; apply-edits comes
  with its own design.
