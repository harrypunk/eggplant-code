# State flow

How a keypress becomes a pixel — the unidirectional data model this codebase
is converging on, an honest scorecard of where we are, and the target.

This is the React/Redux contract, mapped to a terminal editor:

```
 key event ──► RESOLVE ──► ACTION ──► INTERPRET ──► STATE ──► VIEW ──► RENDER
               (pure fns:   (plain     (the ONLY      (slices;  (pure:    (element.rs
               key+state     data —     place shared   App is    props →   — the only
               → intent)    no fns)     state mutates) data)     Element)   painter)
```

The load-bearing rules:

1. **State changes flow one way.** Views never mutate; resolvers never
   mutate; actions are data. Mutation lives exclusively in interpreters.
2. **Local state mutates locally; shared state changes only via actions.**
   React's `useState` vs Redux distinction: a picker's cursor position is
   *local* — the layer mutates it directly. Notifications, buffers, theme,
   focus, layer stack are *shared* — they change only by dispatching an
   action to its interpreter.
3. **UI describes semantics, never colors.** Components say *what* a thing
   is (`StyleClass::SearchMatch`), never *which color* it is. One
   stylesheet maps classes → colors — the HTML/CSS separation.

## Vocabulary

| Role | Shape | Today |
|---|---|---|
| Event | `input::KeyEvent` (core vocabulary) | ✅ one translation point in the runner |
| Resolver | pure `(state, event) → Action` | ✅ `editing::resolve`, layer keymap lookup, leap's `resolve_input` |
| Action | plain-data enum | 🟡 three vocabularies, not one (see scorecard) |
| Interpreter | the only mutator of shared state | 🟡 several, plus scattered direct writes |
| State | slices with behavior | ✅ `App` = composition (ThemeState, InputState, …) |
| View | pure `props → Element` | ✅ Rule 5 holds |
| Stylesheet | `(Theme, StyleClass) → Style`, once | ❌ 11 components pick concrete colors directly |
| Renderer | paints `Element` onto `Frame` | ✅ `element.rs` alone |

## Scorecard — what conforms today

- **Editing is already Redux.** `editing::resolve` (pure) →
  `Resolved`/`EditorAction` (data) → `interpret` (one interpreter, via the
  `EditorCtx` trait — program-to-interface, testable with a stub ctx).
  This is the model everything else should copy.
- **Components are pure and swappable.** props → Element, no Frame, no
  App, no I/O. Our "storybook" is the `TestBackend` paint tests: render a
  component with fixture props, assert cells. This already works.
- **The picker is close.** `on_select: fn(&T) -> Select` — pure specs,
  intents interpreted in one place.
- **Commands funnel through one `apply`.** …but `apply` itself is a
  grab-bag of imperative mutations, and other writers bypass it.

## Scorecard — the violations

| Violation | Where | Fix (below) |
|---|---|---|
| `app.notifications.push(...)` at 11 sites in 4 files | commands, editor layer, files_panel, leap | `AppAction::Notify` |
| `app.editor.open_buffer/jump_to/…` outside the editing pipeline | files_panel, editor layer (view intents) | `AppAction` / keep tier |
| `app.theme.cycle()` called directly by a command | commands.rs | `AppAction::CycleTheme` |
| Components choose colors: 60+ `theme.*` reads in 11 files | components/ | stylesheet (§ Styling) |
| Three intent vocabularies: `Command`, `KeyResult`, `Select` | compositor, picker | unify (§ Actions) |

## Target: one action vocabulary, one dispatch

Today intents are fragmented: `KeyResult` (structural), `Select` (picker),
`Command` (registry). They all mean "please change shared state" — so they
should be one enum, interpreted once:

```rust
/// Every way shared state can change. Plain data — constructible in
/// tests, loggable, replayable. Layers RETURN these; they never perform
/// them.
pub enum AppAction {
    // notifications
    Notify { level: Level, message: String },
    // buffers (the editor slice's app-level operations)
    OpenBuffer { path: PathBuf, at: Option<(usize, usize)> },
    CloseCurrentBuffer,
    SwitchBuffer(usize),
    // theme
    CycleTheme,
    SetTheme(String),
    // layers / focus (absorbs today's KeyResult arms)
    PushLayer(Box<dyn Layer>),
    CloseSelf,
    Unfocus,
    // lifecycle
    Quit,
}

/// The single interpreter. The compositor owns it: layers hand back
/// actions, the compositor dispatches them against App + itself.
fn dispatch(action: AppAction, app: &mut App, compositor: &mut Compositor)
```

`KeyResult` collapses into this (its `Close`/`Unfocus`/`Push`/`Execute`
arms are already actions-in-disguise); `Select` becomes constructors of
`AppAction`; `Command` survives as the *registry's* vocabulary but `apply`
becomes a thin translator `Command → Vec<AppAction>` fed to the same
dispatch. One funnel, one place to audit "how can shared state change".

**The tier boundary stays.** Editor-internal actions (`EditorAction` —
cursor moves, edits) remain the editing pipeline's own vocabulary, because
they're high-frequency and domain-rich; `AppAction` is for *cross-slice*
effects. Rule of thumb: if only `app.editor` changes, it's editing; if
notifications/theme/layers/lifecycle change, it's an `AppAction`.

**Layers keep their local state.** The picker's `input`/`selected`, the
explorer's tree expansion, scroll offsets — local, mutated directly, like
`useState`. Only crossing a slice boundary requires an action. This is the
honest line; a purer "everything is an action" buys boilerplate, not
clarity.

## Target: a stylesheet, not scattered color choices

HTML doesn't set colors; it names classes. Components should do the same:

```rust
/// The semantic style vocabulary — the ONLY thing components may say
/// about appearance. Adding a variant is a design decision, made here.
pub enum StyleClass {
    Text, Muted, Accent, AccentAlt,
    Surface,            // raised background (panels, floats)
    Selection,          // visual selection / focused row
    SearchMatch, SearchCurrent,
    ModeNormal, ModeInsert, ModeVisual,
    Info, Warn, Error,
    Syntax(SyntaxScope), // the one parameterized class
}

/// The stylesheet: the ONLY place (Theme, class) → Style is decided.
/// Swapping themes swaps this mapping; components don't change.
pub fn style(class: StyleClass, theme: &Theme) -> Style
```

Components emit `Element::Text { spans: [(class, text), …] }` —
class-tagged text, no ratatui `Style` construction anywhere in
`components/`. The renderer (`element.rs`) resolves classes through the
stylesheet at paint time. Consequences:

- **Theme changes touch one file.** Today a new color slot means editing
  every component that should use it; with classes, components already say
  what things are.
- **The design is reviewable in one place** — the stylesheet is the CSS
  file; you can read the entire visual language top to bottom.
- **A snapshot test can enumerate every `StyleClass`** and assert the
  mapping — the storybook "all variants" page, as a unit test.

Syntax highlighting keeps `scope_style` (it already *is* a mini-stylesheet);
`StyleClass::Syntax` routes through it so all color resolution lives under
one roof.

## What we deliberately don't do

- **No global event sourcing / action log (v1).** Actions-as-data *enables*
  logging, replay, time-travel — we don't build them until a need exists.
  (Undo is already the editor's own history; that's enough.)
- **No actions for local widget state.** See the tier boundary above.
- **No cascade.** Classes are flat — one class per span, no inheritance or
  specificity rules. CSS's cascade is the part we don't want.
- **Views keep `&App`.** Reads don't mutate; narrowing reads into per-layer
  view-models would add structs without removing risk. (Props adapters at
  the container boundary already keep components App-free.)

## Migration path (each step independently mergeable)

1. ✅ **`AppAction` + `dispatch`** — landed: `action.rs` holds the enum;
   `Compositor::dispatch` is the single interpreter; `Layer::handle_key`
   is read-only on shared state (`&App`) and returns `Handled`. `KeyResult`
   and `Select` are gone — merged outright. Commands are pure translators
   `fn(&App) -> Vec<AppAction>`; `ConfirmDialog` holds actions, not
   callbacks; `editing::resolve` is pure (pending travels as data).
2. ✅ **Absorbed** in step 1 (one atomic migration beats two interim
   shapes).
3. **Stylesheet extraction** (next branch) — mechanical, one component at a time:
   replace `Style::default().fg(theme.x)` with `StyleClass::X` spans;
   components stop importing `Theme`. Land `stylesheet.rs` first with the
   full current mapping so behavior doesn't change mid-flight.
4. **Statusline/topbar/compositor chrome** — last, they're the most
   style-dense.

## Testing seams this buys

- **Resolvers**: pure — already tested as data-in/data-out.
- **Actions**: assert a layer *emits* `AppAction::Notify{…}` for a key,
  without constructing the notification machinery.
- **Interpreters**: one place; test with a fixture App.
- **Stylesheet**: pure fn; enumerate all classes × both builtin themes in
  one test (no unreachable color paths).
- **Views**: unchanged — TestBackend paint tests stay the "storybook".
