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
cargo run -- .              # open a directory: scratch + file explorer (netrw-style)
```

## Keys

| Key | Action |
| --- | --- |
| `h j k l` / arrows | move |
| `w` `e` `b` | word forward / end / backward |
| `0` `$` `G` | line start / end / last line (`nG` → line n) |
| `5j` … | count prefixes on motions |
| `i` `a` `o` `O` | insert / append / open below / above |
| `x` | delete char under cursor |
| `Esc` | back to normal mode |
| `:` | command line (`w` `q` `wq` `q!` `e` `b` `bd` `bn` `bp` `ls` `theme`) |
| `Space` | command palette (fuzzy) |
| `C-e` | toggle file explorer |
| `C-w` | cycle focus (editor ↔ panels) |
| `C-s` | save |
| `C-q` / `C-c` | quit (confirm on unsaved) / force-quit |

## Themes

Semantic theme slots; built-ins `tokyo-night` (default) and `classic`.
Switch at runtime with `:theme <name>`. Custom theme files
(ghostty-style) are on the roadmap (M7).

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
