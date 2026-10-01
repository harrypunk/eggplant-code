# AGENTS.md

Rules for anyone (human or AI agent) working in this repository, **in priority order**.

## Rule 1 — Code quality first

Quality beats speed and cleverness, always.

- `cargo build --workspace` and `cargo clippy --workspace` must stay **warning-free**.
- `cargo fmt` before committing.
- Small, focused commits with clear messages; one logical change per commit.
- No dead code, no commented-out code, no `TODO` without a tracking note in PLAN.md.
- Prefer explicit, readable code over compact or clever code.
- If you touch it, leave it cleaner than you found it.

## Rule 2 — SOLID principles

Apply SOLID insofar as it serves Rule 1 (don't cargo-cult it):

- **Single responsibility** — one reason to change per module/struct. Keep crates focused
  (`eggplant-ui` renders, `eggplant-core` edits, `eggplant-agent` thinks).
- **Open/closed** — extend via traits (e.g. `Layer`, the backend facade), not by editing
  working code.
- **Liskov substitution** — trait implementations must honor the trait's documented contract
  (e.g. `KeyResult` semantics in the compositor).
- **Interface segregation** — small, purpose-built traits over fat ones.
- **Dependency inversion** — the UI depends on the `eggplant-core` facade, never on helix
  crates directly; the agent depends on provider traits, not concrete LLM clients.

## Rule 3 — Prefer declarative code

Write declarative code as much as possible — it reads better and is easier to maintain.

- Describe **what**, not **how**: builder APIs, combinators (`map`/`filter`/`collect`),
  `match` over `if`-chains, iterator pipelines over manual loops, table-driven data over
  procedural branching.
- This is why we chose ratatui: UIs are declared as layout + widget trees, not drawn
  imperatively.
- Imperative code is not banned — use it where it's genuinely clearer (state machines,
  algorithms, I/O glue). Just default to declarative first.

## Rule 4 — Always work on a new branch

- **Never commit directly to `main`.**
- Checkout a branch before any change: `git checkout -b <type>/<short-description>`
  (e.g. `feat/helix-document`, `fix/dialog-keys`, `chore/workspace-skeleton`).
- Merge back to `main` only when the work builds clean and the user confirms.

## Project references

- `PLAN.md` — roadmap, decisions, milestones. Keep it updated as decisions are made and
  milestones completed.
- Workspace layout: `crates/eggplant` (binary), `crates/eggplant-ui`, `crates/eggplant-core`,
  `crates/eggplant-agent`.
