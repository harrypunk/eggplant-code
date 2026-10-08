# Agent design

The AI agent (M10): one headless session, two presentations, tools that
operate *through the editor* instead of around it. This document records
what we take from [pi](https://github.com/earendil-works/pi) (studied at
`/home/j9/work/fork/pi`), what we deliberately skip, and how the agent
fits our architecture (state-flow, facade, UI = f(state)).

## Lessons from pi

Pi is a monorepo of small packages: `ai` (LLM API abstraction, provider
adapters, streaming events) → `agent` (headless loop: events, tool
execution, steering, abort) → `coding-agent` (tools, system prompt,
session) → `tui`. Our workspace already mirrors that shape:
`eggplant-agent` is the headless brain; `eggplant-ui` is the shell.

What we take:

1. **The loop is an event stream, UI-agnostic.** Pi's `AgentEvent`
   vocabulary — `agent_start/end`, `turn_start/end`, `message_start/
   update/end` (streaming deltas), `tool_execution_start/update/end` —
   means the UI subscribes and renders; the loop never knows a UI
   exists. That is exactly our state-flow: agent events are just another
   event source next to keyboard and tick, funneled through dispatch
   into state, painted by pure views.

2. **Tools self-describe.** Each pi tool carries its params schema, an
   `execute`, and a *system-prompt contribution* (one-line snippet +
   guideline bullets). The system prompt is composed from the selected
   tools, not written as one monolith. Our `Tool` trait does the same:
   declaration for the model + prompt contribution + execute.

3. **Edit = exact-text multi-replacement.** Pi's edit tool takes
   `edits[]` of `oldText`/`newText`, matched against the *original*
   content (not incrementally), with uniqueness enforcement and a
   fuzzy-match fallback (trailing whitespace, smart quotes, dashes).
   Proven shape; we adopt the contract verbatim.

4. **Serialize mutations per file, parallelize across files.** Pi's
   file-mutation queue. For us this falls out naturally: edits are
   applied on the dispatch thread, so documents are never mutated
   concurrently (see "single-threaded apply" below).

5. **Hooks are the extension seams**: `beforeToolCall` (can block →
   permission system later), steering/follow-up queues (interrupt the
   agent mid-run), `transformContext` (compaction seam), abort signal
   threaded everywhere. We build the seams, not the features.

What we skip (pi features we don't need in v1):

- Sub-agents, plan mode, extensions/skills/packages, MCP, compaction,
  OAuth flows, 40+ provider adapters, RPC/remote sessions.
- Streaming tool-call *arguments* rendering (we render a tool call when
  it starts, with complete args).
- Parallel tool execution (sequential is correct-by-construction for a
  shared editor; revisit if latency demands it).

## Architecture

```
eggplant-agent (headless, owns tokio runtime)
  provider/   one adapter per API flavor (v1: Anthropic Messages,
              OpenAI-compatible completions), normalizes to a
              ChatEvent stream (text_delta, tool_call, done, error)
  loop.rs     the agent loop: turn = stream reply → execute tool calls
              sequentially → feed results → repeat until no tool calls
  tool.rs     Tool trait: name + params schema + prompt contribution +
              execute(&mut AgentHost)
  tools/      read, write, edit, grep, find (v1)
  session.rs  AgentSession: transcript, config, abort handle
  lib.rs      AgentEvent stream out (mpsc), prompt/steer/abort in

eggplant-ui (shell)
  runner.rs   drains the AgentEvent channel in the event loop next to
              crossterm events → Compositor::dispatch(AppAction::Agent…)
  app.rs      AgentState slice: session handle + UiTranscript
  layers/chat_modal.rs    popup float  (Space a i, C-i)
  layers/chat_window.rs   right-side window (Space a t)
  components/chat.rs      pure transcript + input rendering (shared)
```

The boundary mirrors the editor split: `eggplant-agent` never imports
ratatui/crossterm and never touches the terminal; `eggplant-ui` never
speaks HTTP. The event channel is the only coupling.

### Tools operate through the editor, not around it

This is eggplant's differentiator and the reason the agent was deferred
until the core was solid. Shell-based agents edit files on disk; the
editor reloads (or doesn't) and the user watches diffs land from
outside. Our agent edits *through the facade*:

| tool  | implementation |
|-------|----------------|
| read  | `Editor::peek` — rope-backed, already syntax-aware; offset/limit paging like pi |
| write | create/open the `Document`, replace its text, save |
| edit  | resolve `edits[]` against the **Document's rope** (exact match, then pi's fuzzy normalization), apply as one helix **transaction**, save |
| grep  | our `grep` module (ignore + regex, same caps) |
| find  | our `Workspace::files` walk / fuzzy matcher |

Consequences:

- **The user sees the agent's edits live** — in the buffer, syntax
  highlighted, dirty marker accurate, cursor-aware.
- **Undo works.** Agent edits are transactions in the document history:
  `u` reverts the agent's last change exactly like a user's change.
- **One source of truth.** No reload races between agent-written files
  and open buffers; the document *is* the file.
- The seam already exists: `AgentHost` is a trait with the tool-facing
  surface (peek, read rope, apply transaction, save, search) —
  implemented by the real `Editor` in-process and by a test double in
  agent tests. Dependency inversion, same trick as `EditorCtx` and the
  theme `Probe`.

**Single-threaded apply.** Tool `execute` futures run on the agent's
tokio runtime, but every host mutation is posted to the UI thread as an
action and applied in dispatch — the same rule as keyboard edits
(I/O and mutation at event time, on one thread). Tools return their
result via a completion channel the loop awaits. From the loop's
perspective tools are async; from the editor's perspective all mutation
is serial and immediate. The per-file mutation queue is therefore free.

**Saves are explicit.** An agent edit applies the transaction *and
saves*, so the file on disk always matches what the user sees. Autosave
per tool call, not per keystroke — a coarse, predictable policy.

### One session, two presentations

The user's requirement: chat must be available inline anytime (popup),
and as a persistent workspace member (window) — the *same* conversation.

- **One `AgentSession`** lives in `App` (created lazily on first use).
  It owns the transcript and the channel to the runtime. Closing a view
  never ends the session; there is nothing to reconnect or reload.
- **The modal** (`Space a i` or `C-i`) is a float: transcript + input,
  for quick questions. `Esc` closes the float; the session keeps
  running — a spinner in the statusline shows activity.
- **The window** (`Space a t`) is a right-side window via the existing
  split machinery: the chat becomes a workspace member you can keep
  visible while editing — stream an answer, flip to the buffer, apply
  the suggestion.
- **Both render the same state.** `components::chat` is a pure function
  of `(transcript, input, scroll, focus)`. The two layers are two thin
  containers mapping `App.agent` to the same props with different
  frames — the React container/presentational split applied twice.

### State-flow integration

Agent events arrive on a channel; the runner drains it each tick and
dispatches:

```
LLM delta ──► channel ──► runner drain ──► dispatch(AppAction::Agent(AgentEvent))
                                              │  apply: append delta to transcript
                                              │  broadcast: chat views re-render
                                              ▼
                                        UI = f(state)
```

Everything the agent does to the editor arrives as ordinary actions
(`OpenBuffer`, `Edit`-via-host, `Notify`), so observers stay truthful
(explorer sync, dirty chips) and there is still exactly one interpreter.

Keys: `Space a` group gains `i` (chat popup) and `t` (toggle chat
window); `C-i` is a global shortcut for the popup. Inside a chat view:
`Enter` send, `Esc` close (modal) / unfocus (window), scroll keys on
the transcript, `C-c` abort the current run.

### Provider adapters

V1 ships two adapters behind a `Provider` trait returning a normalized
`ChatEvent` stream (pi's `AssistantMessageEvent`, reduced):

```
text_delta(String) | tool_call { id, name, args } | done { usage } | error(message)
```

- **Anthropic Messages** (SSE): first-class tool_use blocks, thinking
  skipped in v1.
- **OpenAI-compatible chat completions** (SSE): covers OpenAI, local
  servers (llama.cpp, Ollama, LM Studio), OpenRouter, DeepSeek, etc.

Config (`config.toml`):

```toml
[agent]
provider = "anthropic"        # or "openai-compatible"
model = "claude-sonnet-4-5"
api_key_env = "ANTHROPIC_API_KEY"
base_url = "https://api.anthropic.com"   # override for compatible endpoints
```

Deps for eggplant-agent: `tokio`, `reqwest` (stream), `serde_json`,
`schemars` (tool param schemas from Rust types, one source of truth).

### The system prompt

Composed, pi-style: a fixed preamble (you are an editing agent inside
eggplant-code, working on the user's project at `cwd`) + each selected
tool's contribution (snippet + guidelines) + the project's `AGENTS.md`
if present (the file you are reading is exactly what the agent should
read). No skills/extensions in v1; the composition seams are the same.

### Explicitly deferred

- Permission prompts (`beforeToolCall` seam exists)
- Steering/interrupt mid-run beyond abort
- Context compaction (`transformContext` seam exists)
- Thinking/reasoning streams, images
- Chat transcript as a real `Document` (search/copy inside chat with
  editor motions) — attractive, but v1 keeps the transcript as plain
  state; the chat component scrolls, that's all
- Sub-agents, MCP, background tasks

## Why this order

The core split (headless engine / terminal shell) was done precisely so
this moment needs no new seams: the agent is a second consumer of the
facade, its events are a second input to dispatch, its views are two
more containers over a slice of state. The architecture was the
preparation; the feature should now be small.
