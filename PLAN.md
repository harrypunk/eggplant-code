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
5. **AI-native** — not a full IDE, but AI as a first-class citizen. Investigate:
   - integrating with **opencode** (agent CLI, has a server/HTTP API) and/or
     **pi** (coding-agent harness, node SDK) as an external agent backend, vs
   - translating/porting the agent loop to Rust (own provider abstraction over LLM APIs,
     tool calling, streaming into editor surfaces).
   AI UX ideas: chat/prompt panel (toggle), inline edits/diffs in buffer, agent status in
   notifications, apply-patch style edits through the document layer.

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
│  │ integration  │                                │
│  │ (opencode/pi │      ┌──────────────────────┐  │
│  │  or native)  │      │ keymaps / commands   │  │
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

1. **AI integration path** (top priority):
   - a) shell out to opencode/pi as subprocess & stream results (fastest),
   - b) talk to opencode's HTTP server API (opencode serve),
   - c) embed a native Rust agent loop (port the pi/opencode core ideas: provider
        abstraction, tool use, streaming) — most work, most "native".
   - What capabilities first: chat panel? inline completion? agentic edits with diff review?
2. **Compositor/layer model**: z-ordered layers vs ratatui layout-tree? How do floats/dialogs
   capture focus & keys? (study ratatui examples + how helix-term does overlays like pickers)
3. **Editor facade API**: define the trait surface (documents, selections, edits, syntax)
   that the UI talks to, so helix-core can be replaced later.
4. **Keymaps**: reuse helix's TOML keymap/modal system, or define our own?
5. **Runtime assets**: helix runtime dir (themes, tree-sitter queries, languages.toml) — how
   do we load/locate them (XDG, vendored, $EGGPLANT_RUNTIME)?
6. **LSP**: reuse `helix-lsp` in v1 or defer?

## Milestones (draft)

- [ ] **M0 — Skeleton**: git deps to gitea forks wired up; ratatui + crossterm event loop;
      basic compositor with editor surface + one floating dialog + notification toast.
- [ ] **M1 — Headless helix core**: load file into helix Document, normal/insert editing,
      cursor/viewport, render through our ratatui surface.
- [ ] **M2 — Compositor v1**: layer/focus system — toggle panels, modal dialogs, notification
      queue; opinionated layout (statusline, gutter, etc.).
- [ ] **M3 — Keymaps/commands**: modal keys, command palette, save/quit, buffers.
- [ ] **M4 — AI v1**: first AI surface (decided in Q1) — e.g. prompt panel streaming a
      response into a buffer/notification, or opencode subprocess integration.
- [ ] **M5 — Syntax highlighting**: tree-sitter via helix-core, themes.
- [ ] **M6 — LSP / AI v2**: LSP features; agentic edits w/ diff review.
- [ ] **M7+ — extras**: file picker/tree, splits/tabs, git (helix-vcs), DAP, own core R&D.

## Notes

- Fork pins (gitea mirrors): helix `079a789` (25.7.1), rataui `7023d4f` (ratatui 0.30.2).
- gitea over plain HTTP on LAN — fine for cargo git deps; consider `[net] git-fetch-with-cli`
  or `.gitconfig` insteadOf if cargo has issues.
- helix pins `rust-toolchain.toml` inside its repo — doesn't affect us as a dependency, but
  watch MSRV (helix 25.7 needs recent stable; our crate is edition 2024).
- opencode is TS (bun), pi is TS/node — "translate to Rust" means reimplementing their agent
  loop concepts, not a literal port; study their protocol/tool schemas first.
