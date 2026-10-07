# Theme system

How colors flow from "somewhere out there" (ghostty config, built-ins, future
user files) to pixels — and why it's shaped this way.

Code: `crates/eggplant-ui/src/theme/`

## The pipeline

```
 sources ───────► ThemeSpec ───────► derive ───────► Theme ───────► App.theme ───────► UI
 (who has        (raw terminal      (one pure fn:   (semantic      (plain state;       (f(state),
  colors?)        colors, no        semantic        slots the      swapped at will)     re-render
                   semantics)        mapping)        app speaks)                         on change)

 ghostty.rs       spec.rs           derive.rs       mod.rs         app.rs              components/
 builtins         ── boundary type ──               SyntaxTheme
 user files (→)
```

The load-bearing idea: **`ThemeSpec` is a boundary type**. Everything left of
it knows nothing about eggplant; everything right of it knows nothing about
where colors came from. Sources and consumers can each grow without touching
the other side.

## The pieces (single responsibility)

| Module | One job | Knows about |
|---|---|---|
| `spec.rs` | The `ThemeSpec` type: bg/fg, optional cursor/selection, ANSI palette 0–15 | nothing |
| `derive.rs` | `ThemeSpec → Theme`: the **only** place raw colors get meaning | both types |
| `ghostty.rs` | Find + parse ghostty's config and theme files → `ThemeSpec` pair | ghostty formats/paths |
| `probe.rs` | Answer "is the terminal dark right now?" | OSC 11 / `termbg` |
| `resolve.rs` | Policy: which source wins, when to re-derive | sources + probe |
| `mod.rs` | `Theme` / `SyntaxTheme` (semantic slots) + built-in themes | the app's color vocabulary |

`mod.rs` predates the rest and is untouched by them: components still read
`theme.accent`, `theme.syntax.keyword`, … and have no idea any of this exists.

## The semantic mapping lives exactly once

`derive.rs` is a table, not logic:

```
accent        = palette[4]   (blue)
comment       = palette[8]   (bright black — the canonical "muted")
error/warn    = palette[1] / palette[3]
surface       = blend(bg, fg, 6%)
syntax.string = palette[2]   (green)
…one line per slot
```

Want to change "what blue means"? One line. Every source — ghostty today,
user TOML tomorrow — inherits the decision. This is why derive is *pure*:
it's the most-tested code in the system and it never does I/O.

## Runtime switching is just state

`App` holds two fields:

```rust
theme: Theme,                 // what the UI reads (Rule 5: f(state))
theme_follow: Follow,         // Fixed | Ghostty { pair, dark } — plain data
```

A light/dark flip is an *event*, handled like any other:

```
focus-in / 3s tick ──► probe.is_dark() ──► refresh(follow, probe)
                                              │ flipped?
                                              ▼
                                     derive(other variant) ──► app.theme = …
```

No layer, component, or command participates. The compositor re-renders from
the new state on the next frame, the same way it would for any state change.

## SOLID, concretely

- **Single responsibility** — each module has one reason to change:
  ghostty changes its config format → `ghostty.rs`. We invent a new semantic
  slot → `Theme` + one line in `derive.rs`. Detection tech changes →
  `probe.rs`. Policy changes → `resolve.rs`.
- **Open/closed** — add a source (user TOML files, phase B) = one new module
  producing `ThemeSpec` + one arm in `resolve::initial`. Nothing else edits.
- **Liskov substitution** — `DarknessProbe` contract: `None` means "can't
  tell", and callers must keep their current theme. `TerminalProbe` and test
  stubs both honor it; `refresh` is correct for any implementation.
- **Interface segregation** — two *small* traits, not one fat theme manager:
  sources `load() -> Result<ThemeSpec pair>`, probes `is_dark() -> Option<bool>`.
- **Dependency inversion** — `resolve.rs` (policy) depends on the
  `DarknessProbe` trait, not on OSC 11. `App` stores `Theme`/`Follow` (data),
  never a source or probe. The probe lives at the runner boundary with the
  rest of the I/O.

## Failure philosophy

Every step degrades, nothing crashes:

| Failure | Result |
|---|---|
| no ghostty config / not inside ghostty | built-in default |
| theme file missing, malformed directive | warning notification → default |
| terminal doesn't answer OSC 11 | probe **dies permanently** (no repeated 500ms stalls); theme stays |
| probe flickers | `refresh` only re-derives on an actual flip |

## Testing seams

- `derive`, ghostty parsing, `blend`, `luminance`: pure — table-driven unit tests.
- `ghostty::load` internals take `&str`; only path discovery touches the fs.
- `resolve::initial` / `refresh` take `inside_ghostty: bool` and
  `&mut impl DarknessProbe` — policy is tested with `StubProbe`, no env, no tty.
- The only untestable-in-CI code is the actual escape-sequence I/O, quarantined
  in `TerminalProbe` and `terminal.rs`, verified manually.

## Extension points (phase B and beyond)

- **User TOML themes** → `theme/file.rs` (new source), one precedence arm.
- **Explicit `dark = "…", light = "…"`** → same `Follow` shape, pair built
  from two specs instead of ghostty's.
- **Non-ghostty terminals** → the probe already works anywhere that answers
  OSC 11; only source discovery is ghostty-specific.
