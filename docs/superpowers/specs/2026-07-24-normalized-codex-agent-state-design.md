# Normalized Codex Agent State Design

## Goal

Show retained Codex child agents with names such as
`/root/task6_implement` and lifecycle states (`working`, `done`,
`interrupted`, `unknown`) without making hooks or Codex rollout JSONL the
Sidebar UI contract.

This design replaces the source-specific UI boundary from the earlier Codex
agent-list design. It retains the already validated hook journal and rollout
readers, but makes their output the only input accepted by the renderer.

## Stable internal contract

The `codex_agents` module publishes one normalized record:

```rust
pub struct AgentRecord {
    pub internal_id: String,
    pub display_name: String,
    pub status: AgentStatus,
    pub started_at: Option<u64>,
    pub finished_at: Option<u64>,
}
```

`internal_id` is retained only for source correlation, deduplication, close
handling, and tests. The Sidebar does not render it.

`display_name` is resolved before the record leaves the state layer:

1. parent rollout `sub_agent_activity.agent_path`;
2. validated child rollout `agent_path`;
3. validated child rollout `agent_nickname`;
4. hook `agent_type`;
5. literal `subagent`.

All values pass through the existing single-line sanitizer. Empty values fall
through to the next source.

## Input adapters

### Hook lifecycle journal

Codex `SubagentStart` and `SubagentStop` hooks append versioned lifecycle
events to the plugin-owned pane journal. This remains the prompt source for
real-time `working` and `done` updates and the restart-safe fallback when
rollout files are unavailable.

The existing `@pane_subagents` option remains the shared active-only
compatibility state used by parent-protection logic and non-Codex agents. It
is not the Codex history UI source.

### Rollout adapter

The parent rollout supplies child IDs, canonical paths, creation order,
explicit interruption, and confirmed close operations. Validated child
rollouts fill lifecycle gaps and may supply a path or nickname for a
hook-only child.

Rollout JSONL is an internal compatibility input, not a stable application
API. All field knowledge remains inside `codex_agents::transcript`. Unknown,
missing, malformed, or changed fields are ignored without clearing the last
valid source state.

## Merge and failure rules

The state layer joins inputs by full internal ID and emits `Vec<AgentRecord>`
in first-seen parent order followed by hook-only order.

- The newest timestamped lifecycle event wins.
- A later start resumes an interrupted or completed child.
- A confirmed close removes that child.
- A parent session change clears the previous normalized records.
- A rollout read failure preserves valid journal-derived records.
- If name enrichment fails, the record remains visible with the hook type or
  `subagent`.
- Failure to enrich Codex agents never changes the parent pane status and
  never prevents Sidebar rendering.

The Sidebar reads only `AgentRecord` values from `PaneRuntimeState`; it does
not parse source fields or choose between rollout and hook labels.

## Rendering

When at least one retained Codex child exists, render a synthetic
`Main [default] (current)` row followed by child records.

Example:

```text
  ├ Main [default] (current)
  ├ /root/task6_implement                ● working 2m5s
  ├ /root/task6_review                   ✓ done 1m12s
  └ /root/interrupted                    ○ interrupted
```

The main and child IDs are intentionally hidden. Width priority is:

1. tree connector;
2. status marker and word;
3. as much of `display_name` as fits;
4. optional duration.

Working duration advances from `started_at`. Done duration is frozen between
`started_at` and `finished_at`. Interrupted and unknown records omit duration.
Claude Code and OpenCode retain their existing active-only subagent display.

## Validation

Tests must cover:

- canonical parent path beats all fallbacks;
- a validated child path enriches a hook-only child;
- child nickname is used when no path exists;
- hook type and `subagent` remain safe fallbacks;
- the UI renders names and statuses without any internal ID;
- narrow rendering preserves a useful portion of the name before duration;
- parent-session reset and malformed rollout behavior stay unchanged; and
- non-Codex snapshots remain unchanged.

Required repository verification:

```bash
cargo fmt
cargo fmt --check
cargo test
cargo clippy --all-targets --all-features
cargo build --release
```

## Out of scope

- treating rollout JSONL as a public stable Codex API;
- changing hook registration or installation;
- adding Sidebar actions for spawn, switch, interrupt, or close;
- changing Claude Code or OpenCode history behavior; and
- exposing internal agent IDs in the UI.
