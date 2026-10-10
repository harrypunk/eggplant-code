# Cloud sessions: thick client + session server

The editor today is a monolith: UI + editor + agent + filesystem in one
process on one machine. The goal is to keep it as a **full local editor**
(offline-capable, like neovim/helix/vscode) while adding an optional
**session server** that provides heavy services and cross-device
continuity.

```
eggplant --remote mygit.lan/session/101
```

The client runs the full editor locally. The server provides what's
expensive or needs to persist across machines — LSP, builds, session
state, AI conversation history. One client at a time; this is not a
collaboration app.

## Why

- **Clone/push friction** — switching machines means clone, branch, push,
  pull, hope nothing conflicts. A session lives in one place; every
  machine is a window into it.
- **AI session portability** — most agent tools don't sync context across
  devices. A session-scoped agent remembers the full conversation, tool
  state, and file context regardless of which machine the user is on.
- **Heavy services** — rust-analyzer, cargo builds, large test suites
  don't need to run on a laptop when a k3s pod has more cores and RAM.
- **Self-hosted** — Gitea/Forgejo on the LAN, k3s for pods. No dependency
  on GitHub Codespaces or Gitpod.

## Architecture

```
┌─────────────────────────────────────────────────────────────┐
│  Dev machine (thick client)                                 │
│                                                             │
│  ┌──────────────────────────────────────────────────────┐   │
│  │  eggplant (full local editor)                        │   │
│  │  • eggplant-ui   (rendering, input, viewport)        │   │
│  │  • eggplant-core (buffers, cursor, undo, syntax)     │   │
│  │  • eggplant-agent (AI tool calls, local execution)   │   │
│  │                                                      │   │
│  │  Works offline. All editing is local-first.          │   │
│  └──────────────────────┬───────────────────────────────┘   │
│                         │  session sync (WebSocket)         │
└─────────────────────────│───────────────────────────────────┘
                          │
┌─────────────────────────│───────────────────────────────────┐
│  k3s cluster           │                                    │
│                         │                                    │
│  ┌──────────────────────▼───────────────────────────────┐   │
│  │  Adapter Server                                      │   │
│  │  • session lifecycle (create/destroy/resume)         │   │
│  │  • auth                                              │   │
│  │  • Gitea/Forgejo integration                         │   │
│  └────────────┬─────────────────────────────────────────┘   │
│               │ provisions                                   │
│  ┌────────────▼────────────────────────────────────────┐    │
│  │  Session Pod                                        │    │
│  │                                                     │    │
│  │  LSP servers       (rust-analyzer, etc.)            │    │
│  │  Build orchestration (cargo, etc.)                  │    │
│  │  Session state     (open buffers, agent             │    │
│  │                     conversation)                   │    │
│  │  Git working tree                                   │    │
│  │                                                     │    │
│  │  No rendering. No UI state. No viewport/scroll.     │    │
│  └────────────┬────────────────────────────────────────┘    │
│               │                                              │
│  ┌────────────▼────────────────────────────────────────┐    │
│  │  Persistent storage                                 │    │
│  │  • git repo (source of truth for code)              │    │
│  │  • SQLite session state (open buffers, agent conv)  │    │
│  └─────────────────────────────────────────────────────┘    │
└─────────────────────────────────────────────────────────────┘
```

## What lives where

### Thick client (full editor, runs locally)

The client is a complete editor — same as running `eggplant` without
`--remote`. It works offline and has zero latency for editing operations.

| Client owns | Why local |
|---|---|
| Rendering (ratatui `Frame`) | terminal I/O is inherently local |
| Input → actions (crossterm) | input device is local |
| Scroll position, viewport | high-frequency, rendering-only state |
| Cursor, mode, selections | immediate feedback, no round-trip |
| Undo/redo stack | instant, local operation |
| Syntax highlighting | can run locally; server provides grammars if needed |
| AI tool calls | `eggplant-agent` runs locally for fast tool execution |
| Theme, UI prefs | local taste, not session state |
| Modal pending state | input state machine, no server concern |

The client is the **same binary** as standalone eggplant. `--remote` adds
a session sync layer on top — it doesn't replace the editor.

### Session server (heavy services + state recorder)

The server does **not** run the editor. It provides what's expensive or
needs to persist:

| Server owns | Why remote |
|---|---|
| LSP servers (rust-analyzer, etc.) | heavy memory/CPU, long startup — keep it warm in a pod |
| Build orchestration | cargo builds use pod cores/RAM, not the laptop |
| Open buffer list | which files are open — for resume on another machine |
| Agent conversation | full history + tool results — survives machine switches |
| Git working tree | the single working copy that all machines share |

The server does **not** own: rendering, UI state, scroll, viewport,
modal state, cursor position, undo history, or input processing. Those
are purely client concerns.

### Adapter server (infrastructure, not the editor)

The adapter knows nothing about editing. It manages containers:

| Responsibility | Mechanism |
|---|---|
| Session provisioning | `POST /sessions` → clone repo into PVC, start pod |
| Session resume | `GET /sessions/:id` → reattach to running pod (or cold-start from PVC) |
| Gitea integration | Webhooks: PR events, branch creation → auto-session |
| Auth | Per-user access to repos/sessions |
| Pod lifecycle | Keep warm when active; cold-start only when necessary |

## Single client, no collaboration

One client connected to a session at any time. Connecting from machine B
disconnects machine A (graceful detach, not a conflict). This simplifies
everything — no CRDT, no OT, no conflict resolution. The session state
is a single-writer model: the connected client writes, the server
records.

Switching machines:
1. Disconnect from machine A (explicit or timeout)
2. Connect from machine B
3. Server sends current state: open buffers, agent conversation
4. Client reconstructs the session locally and continues

## Session sync protocol

### What crosses the wire

Only **session-level** events, not editing keystrokes. The client edits
locally; the server records the results:

```
Client → Server:  BufferOpened { path: "src/main.rs" }
Client → Server:  BufferEdited { path: "src/main.rs", diff: ... }
Client → Server:  AgentMessage { role: "user", content: "..." }
Server → Client:  SessionState { buffers: [...], agent: [...] }
Server → Client:  LspResponse { ... }
Server → Client:  BuildResult { ... }
```

The client can batch edits (send diffs periodically, not per-keystroke).
LSP requests go to the server; diagnostics come back as notifications.

### Protocol choice

**WebSocket + JSON** to start:

- Simple, debuggable with `websocat`
- Session events are low-frequency (buffer opens, cursor moves, edits)
- LAN latency makes binary encoding unnecessary at first
- Upgrade to msgpack or protobuf later if profiling shows it matters

## Session storage

| Data | Store | Why |
|---|---|---|
| Committed code | Git repo (Gitea) | Already the source of truth |
| Working tree | PVC (pod-local) | Single working copy shared across machines |
| Open buffer list | SQLite (in PVC) | For resume — which files were open |
| Agent conversation | SQLite (same DB) | Full history, survives pod restarts |
| LSP state | Ephemeral (pod) | Re-indexes on cold start; not worth persisting |

SQLite is the pragmatic default. PostgreSQL only enters the picture if
cross-session queries become a real need (fleet analytics, debugging
across sessions).

## Session fork and export

Sessions can be forked or exported — but keep it simple:

- **Fork** — create a new session from an existing one: branch the
  working tree, copy the agent conversation. Useful for "try this
  approach without losing the current one." Maps to a git branch +
  SQLite copy.
- **Export** — dump session state (agent conversation, edit log) as a
  portable archive. Useful for sharing context or debugging.
- **No overcomplication** — no real-time collaboration, no merge
  conflicts between sessions, no session DAG visualization. Fork and
  export are enough.

## AI agent: hybrid client/server

The agent is split:

- **Client-side** (`eggplant-agent`) — runs tool calls locally for
  immediate feedback: file reads, grep, quick edits. Same as standalone
  mode.
- **Server-side** — the conversation history and high-level context live
  in the session pod. When switching machines, the agent picks up where
  it left off because the conversation is session-scoped, not
  machine-scoped.

Tool call routing: local tools (file I/O, grep) execute on the client
for speed. Heavy tools (build, test, multi-file refactor) are forwarded
to the server.

## Build tool routing: local-first, server for heavy work

The agent calls `cargo` frequently — `check`, `clippy`, `test`, `build`.
Where those run depends on weight and frequency.

### Local (default) — fast feedback, no network overhead

| Operation | Why local |
|---|---|
| `cargo check` | Fast feedback loop, runs on every save |
| `cargo clippy` (single file / small crate) | Seconds on a laptop, immediate results |
| `cargo test --lib` (unit tests) | Quick, local deps already present |
| `cargo build` (debug) | Incremental, hot path during development |

The agent is already client-side. The working tree is local (thick client
model). Running `cargo clippy` locally is a direct `Command::spawn` —
zero network overhead, results in milliseconds. For most AI tool calls
("check if this compiles", "run clippy on this file"), local is right.

### Server — when it would block the user

| Operation | Why server |
|---|---|
| `cargo build --release` | Minutes, ties up the laptop |
| Full test suite (integration, e2e) | Long-running; user keeps editing while it runs |
| Cross-compilation (`--target aarch64-...`) | Needs toolchains the laptop might not have |
| `cargo clippy --workspace --all-targets` on a large monorepo | CPU-heavy, blocks the editor locally |

The server shines when the operation would **block the user** or **exceed
local resources**. A `cargo build --release` that takes 3 minutes on a
laptop is 45 seconds on a pod with 16 cores — and the user can keep
editing while it runs.

The pod already has the working tree (it's the session's git checkout),
so server-side builds need no file sync — just run `cargo` in the pod's
working directory. Results (diagnostics, test output) come back as
structured data over the WebSocket.

### Routing strategy

Start simple — **everything runs locally**. Add server offload when it
hurts:

```
Agent tool: run_cargo(subcommand, args)

if subcommand is "build --release" or full test suite:
    → forward to server (async, results pushed back)
else:
    → run locally (sync, immediate)
```

The agent calls `run_cargo("clippy", ["--workspace"])` without knowing
where it runs. A config knob (`build.remote = auto | always | never`)
lets the user override the heuristic.

**Pragmatic rule:** don't build server offload before it hurts. When
`cargo clippy --workspace` blocks the editor on a laptop, that's when
it earns its complexity.

## Filesystem: git-based sync, not a file API

The client needs local file access — the editor reads files for syntax
highlighting, LSP needs them for indexing, `cargo` needs them for builds,
the agent reads/writes them for tool calls. The question is how the
local working tree stays consistent with the server.

### Why not a server file API (get/put)

Every file operation becomes a network round-trip:

```
editor.highlighted_line("src/main.rs")
  → server.get_file("src/main.rs")     // 1ms+ per read
  → highlight locally
```

A typical editing session involves hundreds of file reads. Even at 1ms
LAN latency, that's noticeable. The editor already knows how to read
local files — routing them through the server adds latency for no
benefit. This model fits a thin client; the thick client reads from
disk.

### Why not a shared filesystem (NFS/PVC mount)

Network filesystems over WiFi are fragile — latency spikes, stale
caches, disconnects. Fine for a desktop on wired LAN, painful for a
laptop. Also couples the editor to the pod being alive, breaking
offline support.

### Git IS the sync layer

Gitea is already self-hosted. The session's working tree is already a
git repo. Don't invent a custom sync protocol on top of it.

```
┌─────────────┐         git push/pull         ┌─────────────┐
│  Machine A  │ ◄─────────────────────────────► │   Gitea     │
│  (editor)   │                                 │  (source of │
└─────────────┘                                 │   truth)    │
                                                └──────┬──────┘
                                                       │ git pull
                                                ┌──────▼──────┐
                                                │ Session Pod │
                                                │ (LSP, build)│
                                                └─────────────┘
```

**Session open:**
```
git clone gitea.lan:user/session-101.git /tmp/eggplant-session-101
cd /tmp/eggplant-session-101
eggplant
```

**Edit locally:** normal editing, cargo, LSP, agent — all against local
files. Fast, offline-capable, no special code paths.

**Switch machines:**
```
git add -A && git commit -m "session checkpoint"
git push
# on machine B:
git pull
```

This gives:
- **Fast local I/O** — editor reads from disk, no network per file
- **Offline support** — local working tree exists without the server
- **Conflict semantics** — git handles merges, already understood
- **No custom sync protocol** — git is battle-tested, Gitea is already
  there
- **Session = git branch** — forking a session is `git checkout -b`

### The pod's working tree

The pod clones the same Gitea repo. When the client pushes, the pod
pulls. When the pod builds, it builds its local copy. Both client and
pod are peers syncing through Gitea.

Gitea is always up; the pod can come and go. The client can work
offline against Gitea directly if the pod is down.

### Agent file tools and server builds

Agent tools (`read_file`, `write_file`, `grep`) operate on the **local**
working tree — fast, no network. When the agent triggers a server-side
build, it syncs first:

```
agent: "run cargo test --workspace"
  → git add -A && git commit -m "agent checkpoint" && git push
  → server pulls, runs tests, returns results
```

Local files for local operations, git sync for server operations.

### When does the server need file access?

Only for operations that **run on the server**: LSP indexing, `cargo
build`, `cargo test`. Those run against the pod's local clone — same
as SSHing into a build server and running `cargo` there. The server
never serves files to the client.

## Migration path

### Phase 1 — session recording (no server yet)

Add session state serialization to the current editor: save open buffers
and agent conversation to a local SQLite file on exit; restore on start.
This is the foundation — the same data model the server will later
persist.

Validates: the session state schema, what "resume" actually needs.

### Phase 2 — remote LSP and builds

Extract LSP and build orchestration into a headless service that the
editor talks to over WebSocket. The editor still runs everything else
locally. One machine runs the LSP server; the editor connects to it.

Validates: the LSP proxy protocol, latency for diagnostics/completion
over LAN.

### Phase 3 — session server + adapter

The session pod takes over state persistence. `eggplant --remote
mygit.lan/session/101` connects to a session, syncs state, and uses
remote LSP/builds. The adapter manages pod lifecycle.

Validates: cross-device resume, agent conversation continuity.

### Phase 4 — Gitea integration

Adapter hooks into Gitea webhooks: opening a PR auto-creates a session,
branch creation provisions a workspace. The editor becomes the frontend
to a git-aware development environment.

## What the current editor becomes

The current eggplant is not a throwaway — it remains the **full local
editor** that also happens to support remote sessions:

- Standalone mode (`eggplant`) — works exactly as today, no server needed
- Remote mode (`eggplant --remote`) — same editor, with session sync and
  remote LSP/builds layered on top
- The crate boundary (`core` vs `ui`) is not the network boundary — the
  network boundary is between the editor (all crates) and the session
  server (a new service)
- `eggplant-core`'s UI-agnostic design still matters: the session server
  consumes the same buffer/cursor/action vocabulary, just as data rather
  than direct calls

The work already done to separate concerns (Rule 5: UI = f(state),
dependency inversion over helix, action-based editing pipeline) pays off:
the editor is already structured so that session state can be serialized
and restored cleanly.
