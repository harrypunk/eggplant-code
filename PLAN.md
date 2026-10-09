# eggplant-code — Plan

A custom, AI-native terminal editor with an opinionated UI layout.

- **Editing core**: helix's headless core crates (v25.7.1) for now; may build our own core later
- **UI rendering**: ratatui (v0.30.2, declarative React/Vue-like API) with a custom composable layout system

## Decisions

1. **Dependencies via LAN gitea** — git deps from `http://git.z.lan/fork/helix` and
   `http://git.z.lan/fork/rataui` (mirrors of the local clones; gitea is source of truth).
   ```toml
   [dependencies]
   helix-core = { git = "http://git.z.lan/fork/helix" }
   ratatui    = { git = "http://git.z.lan/fork/rataui" }
   ```
   Pin to a rev once stable; keep forks in sync via gitea.
2. **Own term layer** — we do NOT reuse `helix-term` (poor fit for complex layer/surface/layout).
   We implement our own event loop, rendering, and compositor on top of ratatui + crossterm.
3. **Layered UI** — ratatui's declarative API drives an opinionated layout that supports
   first-class **floating windows, togglable panels, dialogs, notifications**, etc., alongside
   the core editor surface.
4. **Helix core first, own core maybe later** — v1 builds on `helix-core` / `helix-view` /
   `helix-loader` (+ `helix-lsp`, `helix-dap`, `helix-vcs` as needed). Design behind our own
   traits/facade so the backend can be swapped later.
5. **AI-native, Rust-native agent** — **ON HOLD** (see milestones; editor UI/UX comes first).
   Not a full IDE, but AI as a first-class citizen. After
   surveying existing agents, we will build a **native Rust agent** adopting the **minimal
   design philosophy of pi** (not a 100% copy): small core, provider abstraction, tool
   calling, streaming. No opencode/pi subprocess or server dependency.
   AI UX ideas: chat/prompt panel (toggle), inline edits/diffs in buffer, agent status in
   notifications, apply-patch style edits through the document layer.
6. **Helix/vim-style modal keys** — the editor adopts helix-style keybindings. First edition
   reuses helix packages for keymaps/commands where feasible (adapt helix's TOML keymap
   system); our own key layer can come later with the own-core effort.

## High-level Architecture

```
┌──────────────────────────────────────────────────┐
│ eggplant-code (binary crate)                     │
│                                                  │
│  ┌──────────────┐      ┌──────────────────────┐  │
│  │ App / Event  │─────▶│ Compositor (ratatui) │  │
│  │   loop       │      │ ├ editor surface     │  │
│  └──────┬───────┘      │ ├ panels (toggle)    │  │
│         │              │ ├ floats / dialogs   │  │
│  ┌──────▼───────┐      │ └ notifications      │  │
│  │ AI agent     │      └──────────────────────┘  │
│  │ (native Rust,│                                │
│  │  pi-inspired │      ┌──────────────────────┐  │
│  │  minimal)    │      │ keymaps / commands   │  │
│  └──────┬───────┘      └──────────┬───────────┘  │
│         │                         │              │
│  ┌──────▼─────────────────────────▼───────────┐  │
│  │ editor backend facade (swappable)          │  │
│  │ v1: helix-core / helix-view / helix-loader │  │
│  │     (+ helix-lsp, helix-dap, helix-vcs)    │  │
│  └────────────────────────────────────────────┘  │
└──────────────────────────────────────────────────┘
```

## Open Questions (detail sessions)

1. **Native Rust agent design** (ON HOLD with M4 — revisit at M9):
   - adopt pi's minimal architecture: small agent loop, provider abstraction (LLM APIs),
     tool-use schema, streaming events into editor surfaces.
   - what tools does the agent get (read/edit buffer, shell, LSP)?
   - what capabilities first: chat panel? inline completion? agentic edits with diff review?
2. **Compositor/layer model**: deferred — study ratatui (examples, widgets, layout) when we
   build M2. Reference how helix-term does overlays (pickers) for ideas, but design our own.
3. **Editor facade API**: define the trait surface (documents, selections, edits, syntax)
   that the UI talks to, so helix-core can be replaced later.
4. **Keymaps**: helix/vim-style; v1 reuses/adapts helix's TOML keymap system — check how
   much of it lives in helix-term vs helix-view, and how separable it is from their UI.
5. **Runtime assets**: ✅ resolved (M6) — language support is config-driven via
   helix-core's `syntax::Loader`: `languages.toml` (user config merged over helix's embedded
   default), queries and grammar `.so`s discovered from helix runtime dirs
   (`~/.config/helix/runtime`, `$HELIX_RUNTIME`, exe-sibling). Nothing vendored or compiled
   in; new languages need zero code changes. Our only baked-in piece is the 12-scope
   highlight vocabulary registered via `Loader::set_scopes` (themes color those slots).
6. **LSP**: reuse `helix-lsp` in v1 or defer?

## Milestones (draft)

- [x] **M0 — Skeleton**: git deps to gitea forks wired up; ratatui + crossterm event loop;
      basic compositor with editor surface + one floating dialog + notification toast.
- [x] **M1 — Headless helix core**: load file into helix Document, normal/insert editing,
      cursor/viewport, render through our ratatui surface.
      (facade `eggplant_core::Editor` wraps helix-view `Document`; block-cursor selection
      model: range direction encodes mode; own `edits_since_save` modified flag)
- [x] **M2 — Compositor v1**: layer/focus system — toggle panels, modal dialogs, notification
      queue; opinionated layout (statusline, gutter, etc.).
      (LayerKind Base/Panel/Float drives pure `compute_layout`; focus: push→focus, `C-w` cycle,
      Esc→Unfocus vs Close; statusline extracted to global chrome; files panel `C-e`;
      confirm-quit dialog `C-q`; notification queue cap 5 + "+N more";
      `eggplant <dir>` opens the files panel netrw-style, clap CLI)
- [x] **M3 — Keymaps/commands**: modal keys, command palette, save/quit, buffers.
      (decision: own keymap tables, NOT helix's TOML system — lives in helix-term, coupled to
      its command enum. Registry: commands defined once, keymap references by index.
      `:` command line → ex_commands table (w/q/wq/q!/e/b/bd/bn/bp/ls); palette on `Space`
      with own fuzzy matcher; editor mode keymaps as data tables + count prefixes (`5j`,`nG`);
      buffers: open/switch/next/prev/close in facade)
- [ ] **M4 — AI v1 (native agent)** — **ON HOLD**: editor UI/UX milestones come first;
      resume when the editing experience is solid. Minimal Rust agent loop (pi-inspired):
      provider abstraction + streaming chat into a toggleable panel; agentic edits later.
- [x] **M5 — Command & window UX** ✅
      - **Buffer topbar**: tabline chrome above the editor showing open buffers
        (neovim/vscode style): name, modified dot, current highlighted.
      - **Windows, not layers-with-focus**: editor and file explorer are equal windows;
        navigate directionally with `C-h`/`C-l` (neovim `C-w h/l` model).
      - **Command entries, two roles** (vscode/lazyvim split):
        - palette = *complete* command list, rebound to `C-S-p` (fuzzy over registry).
        - `Space` = which-key style prefix menu for *common* commands (`Space f` file,
          `Space b` buffers, `Space s` search, `Space g` goto/leap, …). Groups are nouns,
          leaves are verbs; leaves name registry command ids.
        - `:` command line + ex table removed (duplicate of the above).
- [x] **M6 — Syntax highlighting** ✅ — tree-sitter via helix-core's config-driven
      `syntax::Loader` (runtime-dir queries + dynamic grammars; user `languages.toml` merge);
      `Document::detect_language` on open + incremental reparse on edit; facade exposes
      per-line `HighlightedSpan`s over a 12-scope `SyntaxScope` vocabulary
      (`Loader::set_scopes`, longest-prefix); theme `syntax` slots (tokyo-night palette +
      classic) color them. No grammars/queries baked into the binary.
- [x] **M7 — Editing UX** ✅: undo/redo (`u`/`U`) ✅, delete/yank/paste
      (`dw`, `yy`, `p`) ✅, visual charwise + linewise (`v`, `V`) ✅, sequences (`gg`;
      `ge` deliberately skipped) ✅, search (`Space s b`/`s c` + live prompt, `n`/`N`) ✅,
      leap (`Space g c`, two-char jump with labels) ✅, view intents (`zz`, `C-f`/`C-u`
      page scroll — `C-b` dropped, clashes with tmux) ✅, count prefixes ✅,
      pending-key/count hint in statusline ✅ (moved here from M8),
      **soft-wrap + horizontal scroll** ✅ (`Space u w`; one line-fitting
      policy, `DisplayRow` layout — `docs/design/line-fitting.md`).
- [ ] **M8 — Chrome UX**: ~~custom theme files~~ → **ghostty theme following landed**
      (ThemeSpec boundary + pure derive; OSC 11 probe via termbg; focus-in + 3s re-probe;
      design doc: `docs/design/theme.md`; user TOML theme files = phase B),
      ~~per-buffer view memory~~ ✅ (cursor lives in the Document — the facade
      restores instead of resetting; one Viewport per stable buffer slot),
      mouse support.
- [ ] **M9 — LSP**: diagnostics/goto/completion via helix-lsp (reused, behind the facade).
      Ready seams: `g d` is one arm in the resolver; picker infra covers references/symbols.
- [ ] **M10 — AI v1** (resumes; unhold M4): headless agent session in
      `eggplant-agent` (event-stream loop, one OpenAI-compatible adapter +
      preset table: qwen / kimi / openai / custom), tools operating **through the editor facade** (read via
      `Peek`, edit via transactions — edits land live in the buffer and are
      undoable), one session two presentations (popup `Space a i`/`C-i`,
      right-side window `Space a t`). Design: `docs/design/agent.md`.
      Then AI v2: diff review, permissions, steering.
- [ ] **M11+ — extras**: file picker/tree, splits/tabs, git (helix-vcs), DAP, own core R&D.

## Post-M6 hardening (landed, no milestone number)

- **Config file**: single `~/.config/eggplant/config.toml` — `[theme]`, `[keys.*]`,
  `[files]`; errors become startup notifications, never crashes.
- **Every keymap is data**: three tiers — global `Registry` (`[keys.global]`), modal
  `Keymaps` (`[keys.normal/visual/insert]`), layer-local `LayerKeymaps`
  (`[keys.explorer/picker/prompt/dialog/leap]`, closed action enums + `DEFAULT_KEYS`
  tables per layer). Exact-modifier stroke matching designed out the C-l shadowing bug
  class. Text entry (chars/Backspace) is deliberately not a binding.
- **File picker `Space f p`**: `ignore`-crate walker (respects .gitignore, skips hidden,
  20k cap) + gitignore-syntax ignore list (`[files] ignore`, defaults first so `!pattern`
  re-includes); explorer shares the rules + `I` toggle.
- **Refactor pass (user-driven SOLID)**: `Viewport` (scroll policy, pure), overlay
  selectors on App (`line_labels`/`dims_editor_text` — surface is decoration-agnostic),
  `FileTree` (tree policy + injected `DirLister`), `Workspace` (root+ignores cohesive).
- **Editing pipeline**: Redux-style — keymaps are data, `resolve()` is a state machine,
  `interpret()` is semantics, `EditorCtx` a narrow trait; leap got the same
  resolve/interpret split at layer scale.
- **Fixes**: zero-buffer honest state, gutter line count (phantom rope line), `j` clamps
  at last real line, focus = compositor state (`CloseUnfocus`), panels pass Ctrl/Alt keys
  through.

## Workspace layout

```
eggplant-code/
├── Cargo.toml            # virtual workspace manifest (shared deps)
├── AGENTS.md             # repo rules: quality first, SOLID, branch-per-work
├── docs/design/          # subsystem design docs (theme.md, …)
└── crates/
    ├── eggplant/         # binary: event loop + wiring (package: eggplant-code)
    ├── eggplant-ui/      # terminal SHELL: app state, commands/registry,
    │                     # config, theme, compositor/element/components/
    │                     # layers, runner (crossterm → core translation)
    ├── eggplant-core/    # headless ENGINE (no terminal deps): helix facade,
    │                     # input vocabulary, editing pipeline (resolve/
    │                     # interpret/keymaps), viewport, files/filetree,
    │                     # grep, fuzzy — docs/design/architecture.md
    └── eggplant-agent/   # native Rust AI agent (placeholder, lands in M4)
```

## Notes

- Fork pins (gitea mirrors): helix `079a789` (25.7.1), rataui `7023d4f` (ratatui 0.30.2).
- gitea over plain HTTP on LAN — fine for cargo git deps; consider `[net] git-fetch-with-cli`
  or `.gitconfig` insteadOf if cargo has issues.
- helix pins `rust-toolchain.toml` inside its repo — doesn't affect us as a dependency, but
  watch MSRV (helix 25.7 needs recent stable; our crate is edition 2024).
- pi is TS/node — we are NOT porting it; the native Rust agent only borrows pi's minimal
  design ideas (agent loop, provider abstraction, tool schema, streaming).

## Learnings (helix backend)

- `Document` lives in **helix-view**, not helix-core; needs `Arc<dyn DynAccess<Config>>`
  (`Arc<ArcSwap<Config>>`) + `Arc<ArcSwap<syntax::Loader>>` (empty loader for now).
- helix uses **block-cursor selection semantics**: stored selections are always >=1 grapheme
  wide (`ensure_invariants`); range *direction* encodes mode — forward `(pos, pos+1)` =
  normal block at `pos`, backward `(pos+1, pos)` = insert bar at `pos`. `Transaction::insert`
  inserts at `range.head`.
- Word motions return extended ranges (old -> new); destination: `head` for word-starts,
  `prev_grapheme(head)` for word-ends.
- `doc.is_modified()` relies on helix history internals (needs a `View` to flush changes) —
  the facade tracks its own `edits_since_save` counter instead.
- `doc.save()` returns a `Future` using `tokio::fs` — polled on a current-thread runtime
  inside the facade for now.
- **Key routing**: focused layer first (modal layers swallow all), then globals. Char bindings
  must guard against Ctrl/Alt modifiers (`plain_char` helper) or Ctrl-combos leak into text.
- **Declarative UI**: components (`components/`) are pure props→`Element` fns; containers
  (`layers/`) own state/events and adapt `App`→props; `element.rs::paint` is the only
  `Frame` toucher. Cursor is data (`Element::Cursor`); last painted wins (topmost layer).
  Paint smoke-tests use ratatui's `TestBackend`.
- **Layers can't touch the compositor** — they return effects instead: `KeyResult::Push`,
  `Execute(Command)`, `RunEx(input)`; the compositor performs them (close-then-run semantics).
- `Command` is `Copy` (fn ptr + &'static str) so registry lookups return by value — avoids
  borrow conflicts when executing with `&mut App`.
- **Rope line count includes a phantom trailing line** — `last_line()` (= count−1) is the
  law for all vertical motion; `display_line_count()` (= last+1) for gutter/clamping.
  Scratch docs start with a placeholder newline, so tests must use real temp files.
- **`DocumentId::default()` always returns 1** — buffer slots use our own `usize`.
- **crossterm reports Ctrl+char as `Char(c)` + CONTROL** — code-only matches eat global
  keys; exact-modifier `KeyStroke::matches` made the whole guard class unnecessary.
- **`ignore` crate gotchas**: `filter_entry` is a `WalkBuilder` method needing a `'static`
  closure (clone rules in); `GitignoreBuilder::new(root)` + `add_line`, `matched(rel, is_dir)`.
- **OSC 11 is the terminal-theme truth** — ghostty (and anything modern) answers it; the
  OS portal is the wrong layer for child processes. `termbg` does the raw-tty dance next
  to crossterm safely; a failing probe must die permanently or every retry costs a timeout.
- **Semantic resolution scales down**: the Redux split (resolve = pure meaning,
  interpret = apply) works at layer scale too — leap's phase machine became a testable
  pure fn. `handle_key` that still matches `KeyCode` is a smell.
