# eggplant-code

A custom, AI-native terminal editor in Rust — helix's headless editing core,
a declarative ratatui UI (React/Compose-style: components are pure functions
of state), and an opinionated layer system with floating windows, docked
panels, dialogs, and notifications.

**Status**: early development. The AI agent is on hold while the editor
UI/UX matures — see [PLAN.md](PLAN.md) for the roadmap.

## Usage

```sh
cargo run                   # scratch buffer
cargo run -- foo.rs         # edit a file
cargo run -- .              # open a directory: file explorer + welcome screen
```

## Keys

| Key | Action |
| --- | --- |
| `h j k l` / arrows | move |
| `w` `e` `b` | word forward / end / backward |
| `0` `$` `G` | line start / end / bottom |
| `gg` | first line (`ngg` → line n) |
| `zz` | center the cursor line vertically |
| `C-f` / `C-u` | page down / up (cursor and scroll follow) |
| `n` / `N` | next / previous search match |
| `Esc` | clear search highlight |
| `5j` … | count prefixes on motions |
| `i` `a` `o` `O` | insert / append / open below / above |
| `v` / `V` | visual charwise / linewise: motions extend, `d`/`x`/`y` act, `v`/`V`/`Esc` exits |
| `x` | delete char under cursor |
| `d{motion}` / `dd` | delete (to motion / whole line); also yanks |
| `y{motion}` / `yy` | yank (to motion / whole line) |
| `p` | paste after cursor / below line (linewise) |
| `u` / `C-r` | undo / redo (one revision per insert session) |
| `Esc` | back to normal mode |

### Global keys

| Key | Action |
| --- | --- |
| `C-S-p` (or `C-p`) | command palette — fuzzy over the complete command list |
| `C-e` | toggle file explorer |
| `C-h` / `C-l` | move window focus left / right (editor ↔ explorer) |
| `C-s` | save |
| `C-q` / `C-c` | quit (confirm on unsaved) / force-quit |

Explorer keys: `j`/`k` move, `l`/`h` expand / collapse-or-parent, `Enter`
open file / toggle directory, `I` toggle full / filtered listing (filtered
hides dotfiles + ignore-rule matches — the same rules as the picker).
`C-h`/`C-l` move focus between explorer and editor like any window split.

### Keybinding levels

Commands live at three levels, by frequency:

1. **Command palette** (`C-S-p`) — the complete reference: every command,
   fuzzy-searchable. If it exists, it's here.
2. **`Space` prefix tree** (which-key) — common functionality, grouped for
   discovery. Menus cost one extra key but show themselves.
3. **Direct modal sequences** — the few daily-driver commands get two-key
   normal-mode sequences via the pending-prefix mechanism (the same state
   machine behind `gg`): today `gg`/`dd`/`yy`; planned `◌ g d` goto
   definition, `◌ s r` grep project root. These shadow nothing — `g`/`s`
   are prefixes, not single-key commands.

### The `Space` tree (which-key)

Mnemonic groups, helix-style. Group letters are **reserved up front** so
future LSP / AI / navigation features extend the tree without ever moving
an existing binding. `◌` = designed, not yet implemented.

```
Space
├── f +file
│   ├── s save
│   ├── q save & quit
│   ├── e explorer
│   └── p picker           — fuzzy over workspace files
├── b +buffer
│   ├── n next
│   ├── p prev
│   └── d close
├── s +search
│   ├── b in buffer        — live /-style search, n/N cycle
│   ├── c grep lines       — live picker over buffer lines
│   └── p in project  ◌    — workspace live-grep (fast path: `s r`)
├── g +goto
│   ├── c char             — 2-char leap jump
│   ├── d definition  ◌    — LSP (fast path: `g d`)
│   └── r references  ◌    — LSP (fast path: `g r`)
├── w +window  ◌           — focus/split management (beyond C-h/C-l)
├── l +lsp  ◌              — hover, rename, code actions, diagnostics
├── a +ai  ◌               — agent chat, inline edit, …
├── p command palette
├── t cycle theme
└── q quit
```

Rules of the tree: groups are nouns (`f`ile, `b`uffer, `s`earch…), leaves
are verbs; every leaf maps to a registry command id (so the palette and the
which-key menu can never disagree); the root holds only the few cross-group
singletons (`p` palette, `t` theme, `q` quit). Keybindings will become
user-configurable (`keys.toml`) once the config loader lands (M8).

## Configuration

One file, ghostty-style: `$XDG_CONFIG_HOME/eggplant/config.toml`
(`~/.config/eggplant/config.toml`). Everything is optional; bad entries
warn in-app and fall back to defaults.

```toml
theme = "classic"          # built-in theme name. Unset: inside ghostty we
                           # follow its theme = light:X,dark:Y and switch with
                           # the OS; elsewhere the built-in default is used

[keys.global]              # any mode; value = command id (palette names)
"C-x" = "app.quit"

[keys.normal]              # modal tables; value = edit.* action id
";" = "edit.enter-insert"

[keys.visual]
# ...

[keys.insert]
# ...

# Layer-local bindings: stroke → action id. Action ids per layer:
#   explorer: down up expand collapse toggle-all open unfocus
#   picker:   down up confirm close
#   prompt / dialog / leap: confirm close / confirm cancel / close
[keys.explorer]
# "u" = "up"
[keys.picker]
# "C-j" = "down"

[files]                    # file picker ignore list (gitignore syntax)
ignore = ["dist/", "!target/"]   # add a pattern; ! re-includes a default
```

Stroke syntax: `"C-S-p"`, `"Space"`, `"Esc"`, `"left"`, `"F2"`, or a single
char (case matters: `"G"` ≠ `"g"`). User bindings shadow defaults for the
same stroke. Command ids: palette entries (e.g. `file.save`) or `edit.*`
actions; the which-key tree itself stays code-defined for now.

The picker always respects `.gitignore` and skips hidden files. Built-in
ignores cover dependency/build dirs (`target/`, `node_modules/`,
`__pycache__/`, `.venv/`, `venv/`, `*.egg-info/`); `[files] ignore` extends
them with gitignore semantics — add `dist/`, remove with `!target/`.

## Themes

Semantic theme slots; built-ins `tokyo-night` (default) and `classic`.
Cycle at runtime with `Space t`, or set `theme` in the config. Custom theme
files (ghostty-style) are on the roadmap (M8).

## Language support (syntax highlighting)

Fully **helix-compatible** — if you already use helix, there is nothing new
to set up. Language configs, queries, and grammars are discovered from
helix's runtime directories, in priority order:

1. `~/.config/helix/runtime` (where `hx --grammar build` puts grammars)
2. `$HELIX_RUNTIME`
3. a `runtime/` dir next to the executable
4. `languages.toml`: helix's built-in default, merged with your
   `~/.config/helix/languages.toml` if present

So: grammars built once for helix (queries + `.so` files) are shared with
eggplant-code as-is. If your helix came from a distro package, point at its
runtime once:

```sh
export HELIX_RUNTIME=/usr/lib64/helix/runtime   # or /usr/lib/helix/runtime
# or: ln -s /usr/lib64/helix/runtime ~/.config/helix/runtime
```

Adding a language never requires code changes: drop the grammar `.so` +
`queries/<lang>/highlights.scm` into a runtime dir and (for brand-new
languages) add an entry to `~/.config/helix/languages.toml`.

Files without a grammar fall back to plain text — the editor always works.
Highlight colors come from the active theme's syntax slots.

## Development

```sh
cargo build --workspace    # warning-free
cargo clippy --workspace   # warning-free
cargo test --workspace
cargo fmt --all
```

Repo rules live in [AGENTS.md](AGENTS.md) (quality first, SOLID, declarative
UI, branch-per-work). Architecture decisions and milestone history live in
[PLAN.md](PLAN.md).

```
crates/
├── eggplant/         # binary: thin launcher (clap CLI)
├── eggplant-ui/      # compositor, layers, components, element renderer
├── eggplant-core/    # editor facade over helix-core/helix-view
└── eggplant-agent/   # native Rust AI agent (placeholder — on hold)
```
