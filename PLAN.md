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
5. **Runtime assets**: helix runtime dir (themes, tree-sitter queries, languages.toml) — how
   do we load/locate them (XDG, vendored, $EGGPLANT_RUNTIME)?
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
- [ ] **M5 — Command & window UX** (current focus):
      - **Buffer topbar**: tabline chrome above the editor showing open buffers
        (neovim/vscode style): name, modified dot, current highlighted.
      - **Windows, not layers-with-focus**: editor and file explorer are equal windows;
        navigate directionally with `C-h`/`C-l` (neovim `C-w h/l` model), replacing `C-w`
        focus cycling (which has a hang bug when toggling back to the editor — superseded
        by this model, verify gone).
      - **Command entries, two roles** (vscode/lazyvim split):
        - palette = *complete* command list, rebound to `C-S-p` (fuzzy over registry).
        - `Space` = which-key style prefix menu for *common* commands: popup shows
          available subsequent keys, updates per keystroke (`Space f` file, `Space b`
          buffers, …). Builds the keymap-tree infra M7 sequences (`gg`) will reuse.
        - remove the `:` command line + ex table entirely (duplicate of the above;
          fs ops like `:e`/`:ls` are covered by the explorer and later grep).
- [ ] **M6 — Syntax highlighting**: tree-sitter via helix-core; highlight styles read the
      theme's semantic slots (theme system landed first for this reason — `theme.rs`,
      built-ins tokyo-night/classic, `:theme <name>`).
- [ ] **M7 — Editing UX**: undo/redo (`u`/`U`), delete/yank/paste with textobjects
      (`dw`, `yy`, `p`), visual mode (`v`), key sequences (`gg`, `ge` — keymap-tree infra
      lands in M5 via which-key), search (`/`), horizontal scroll or soft-wrap (long lines
      are unreachable today).
- [ ] **M8 — Chrome UX**: custom theme files (ghostty-style
      `~/.config/eggplant/themes/*.toml`), per-buffer view memory (cursor+scroll survive
      buffer switches), pending-key/count display in statusline, mouse support.
- [ ] **M9 — LSP**: diagnostics/goto/completion via helix-lsp (reused, behind the facade).
- [ ] **M10 — AI v1 resumes** (unhold M4), then AI v2: agentic edits w/ diff review.
- [ ] **M11+ — extras**: file picker/tree, splits/tabs, git (helix-vcs), DAP, own core R&D.

## Workspace layout

```
eggplant-code/
├── Cargo.toml            # virtual workspace manifest (shared deps)
├── AGENTS.md             # repo rules: quality first, SOLID, branch-per-work
└── crates/
    ├── eggplant/         # binary: event loop + wiring (package: eggplant-code)
    ├── eggplant-ui/      # compositor, layers, widgets, UI state (ratatui)
    ├── eggplant-core/    # editor backend facade (v1 wraps helix-core)
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
