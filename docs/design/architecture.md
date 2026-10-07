# Crate architecture: headless core, terminal shell

`eggplant-core` is the **headless editor engine** — everything that makes
an editor an editor, with zero terminal dependencies (no ratatui, no
crossterm). `eggplant-ui` is one possible *shell* over it: a ratatui
compositor driven by crossterm events. `eggplant-agent` (M10) will be
another consumer, driving the same core without a TTY.

## The boundary rule

**Core never imports a terminal crate.** Input arrives as core-owned data
(`input::KeyEvent`); output is data (spans, rows, hits). The shell
translates at exactly one place: the runner maps crossterm events into
`core::input` types at the event boundary.

## What lives where, and why

| Module | Crate | Why |
|---|---|---|
| `editor.rs` facade | core | the helix Document wrapper (DIP over helix) |
| `highlight.rs`, `mode.rs` | core | editing vocabulary |
| `input.rs` | core | KeyCode/KeyEvent/KeyStroke/Modifiers — plain data + parsing, no I/O |
| `editing.rs` | core | the editing pipeline: keymaps (data), `resolve` (state machine), `interpret` (semantics), `EditorCtx` (narrow trait the shell implements) |
| `viewport.rs` | core | scroll policy (helix-view precedent: views are headless) |
| `files.rs` | core | Workspace + IgnoreRules + file walking |
| `filetree.rs` | core | tree policy with injected `DirLister` |
| `grep.rs` | core | project search: pure core + fs shell |
| `fuzzy.rs` | core | pure matcher |
| `app.rs` | ui | the composition root's state: core objects + UI-only state (notifications, theme, leap, pending overlays) |
| `commands.rs` | ui | the registry: commands orchestrate layers and notifications — shell glue |
| `keymaps.rs` | ui | LayerKeymaps references layer-local action enums (shell concepts) |
| `config.rs` | ui | parses TOML into core + theme types (theme is shell) |
| `theme/`, `compositor.rs`, `element.rs`, `components/`, `layers/`, `runner.rs`, `terminal.rs`, `startup.rs`, `topbar.rs`, `statusline.rs` | ui | rendering, event loop, chrome |

## Shell state: slices + intents (React-style)

`App` is a **composition of cohesive slices**, not a flat bag — each
slice owns its data and behavior:

| slice | owns |
|---|---|
| `editor` (core facade) | buffers, cursor, modes, history |
| `workspace` | project root + ignore rules |
| `notifications` | the toast queue |
| `theme: ThemeState` | active theme, follow source, probe cadence |
| `input: InputState` | registry, modal keymaps, layer keymaps, pending input |
| `leap`, `wrap` | active overlay; the line-fitting pref |

`App` itself holds only the composition, the lifecycle, and
**cross-slice selectors** (`line_labels`, `dims_editor_text`) — narrow
queries that new overlays extend without touching consumers.

Layers receive `App` uniformly (heterogeneous dispatch demands it) but
follow the React contract: **reads** flow through `&App` at event time;
**writes** to other slices are expressed as **intents** (data),
interpreted in one place. The model instance is `layers::picker`:
`on_select: fn(&T) -> Select` — picker specs are pure data + pure fns;
the `Select` enum (`OpenAt`, `JumpToLine`, `Execute`, …) is interpreted
by the picker layer, the only place picker effects live. Precedent:
`editing.rs`'s resolve/interpret split and `KeyResult::Execute` were
already this pattern at smaller scales.

## Consequences

- **The agent seam is real**: `eggplant-agent` can construct an `Editor`,
  feed it `input::KeyEvent`s or call facade methods directly, and read
  state — no terminal involved.
- **Tests simplify**: core tests construct `KeyEvent::char('j')` data
  directly; no crossterm in any test fixture.
- **Swapping terminals** (a future GUI, a test harness) means writing a
  new shell, not touching the engine.

## What did NOT move (and why)

- **Leap domain** (`layers/leap.rs`): small, entangled with `App` state
  (labels are computed from editor matches but stored as App overlay
  state). A future candidate if the agent needs it.
- **`commands.rs` / `keymaps.rs`**: the registry and layer keymaps are
  about *shell* orchestration (pushing layers, focus), not editing.
- **`theme/`**: colors are a rendering concern (ratatui `Color`).
