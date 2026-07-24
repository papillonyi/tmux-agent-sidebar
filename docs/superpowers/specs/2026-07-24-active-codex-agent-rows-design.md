# Active Codex Agent Rows Design

## Goal

Show only currently working Codex subagents in the Sidebar, using two lines per
agent so the canonical agent path, role, model, status, and elapsed time remain
readable without exposing internal agent IDs.

The synthetic Main entry remains visible while at least one working subagent
exists and uses the same two-line layout.

## Display contract

The rendered list is compact and active-only:

```text
  ├ Main (current)
  │  ● working 2h27m · default · gpt-5.6-sol
  └ /root/task6_implement/lifecycle_probe_alpha
     ● working 1m23s · default · gpt-5.6-terra
```

Each entry has:

1. an identity line containing `Main (current)` or the canonical agent path;
2. a metadata line containing `● working`, elapsed time when known, role, and
   model.

Internal agent IDs are correlation keys only and are never rendered. The
identity line truncates when the Sidebar is narrow. The metadata line retains
the status first, then fits elapsed time, role, and model in that priority
order. Missing role or model data uses `default` or `unknown` rather than
removing the field.

Main uses the pane start timestamp for elapsed time, the fixed role `default`,
and the latest model parsed from the parent Codex rollout.

## Active-only lifecycle

The tracker continues to merge hook-journal and rollout lifecycle events by
full internal ID and timestamp, but publishes only records whose final merged
status is `Working`.

- `Done`, `Interrupted`, and `Unknown` records are not rendered.
- `SubagentStop`, child `task_complete`, child `turn_aborted` with reason
  `interrupted`, or a confirmed close removes the agent from the published
  list.
- A later start may make the same internal ID visible again.
- A missing or malformed rollout preserves valid hook-derived working state.
- A parent session change clears all prior child state.

The synthetic Main entry is rendered only when the filtered child list is
non-empty.

## Metadata sources

The normalized `AgentRecord` gains `role` and `model` fields.

Name resolution remains:

1. parent rollout `sub_agent_activity.agent_path`;
2. validated child rollout `session_meta.agent_path`;
3. validated child rollout `source.subagent.thread_spawn.agent_path`;
4. validated child rollout `agent_nickname`;
5. hook `agent_type`;
6. literal `subagent`.

Role resolution is:

1. validated child rollout `source.subagent.thread_spawn.agent_role`;
2. hook-journal `agent_type`;
3. literal `default`.

Model resolution is:

1. validated child rollout `turn_context.model`;
2. optional model captured from the official `SubagentStart` hook payload;
3. parent rollout model;
4. literal `unknown`.

The parent rollout's latest `turn_context.model` supplies the Main model.
Rollout schema knowledge remains isolated in `codex_agents::transcript`.

## Nested child validation

A child rollout belongs to the current pane when:

- its exact `payload.id` matches the discovered child ID;
- its `payload.session_id` matches the current root session ID; and
- its source is a structurally valid subagent source.

The immediate `parent_thread_id` is not required to equal the root session ID.
Depth-two and deeper agents correctly point to their immediate parent, such as
`/root/task6_implement`, while retaining the root session in `session_id`.
Malformed or non-string relationship fields remain rejected.

This validation allows a nested child such as
`/root/task6_implement/lifecycle_probe_alpha` to contribute its real path,
model, and terminal lifecycle.

## Component changes

- `adapter::codex` preserves the optional hook model for subagent lifecycle
  events.
- `codex_agents::journal` stores the optional model in the plugin-owned
  lifecycle journal without invalidating existing version-1 records.
- `codex_agents::transcript` validates nested children and extracts child
  path, role, model, lifecycle, and the parent model.
- `CodexAgentTracker` normalizes source data, filters its public records to
  `Working`, and exposes the parent model.
- `PaneRuntimeState` stores the filtered child records and parent model.
- `ui::panes::row::body` renders two lines per Main/child entry and does not
  fall back to stale `@pane_subagents` data when no normalized Codex child is
  working.

Claude Code and OpenCode keep their existing active-only single-line
subagent rendering.

## Failure handling

Rollout JSONL remains a best-effort compatibility input rather than a public
API. Unknown fields, partial final lines, transient read errors, and malformed
records do not crash or clear the last valid state.

If child enrichment is temporarily unavailable, the hook role and model are
used. If all model sources are unavailable, the UI renders `unknown`.

## Validation

Tests must prove:

- only `Working` records leave `CodexAgentTracker`;
- completed, interrupted, unknown, and confirmed-closed records disappear;
- depth-two child metadata is accepted when `session_id` matches the root;
- malformed nested relationship fields remain rejected;
- the canonical nested path, role, and child model are normalized correctly;
- hook and parent-model fallbacks work;
- Main and every child render as two inline-snapshot rows;
- narrow rendering preserves status before optional metadata;
- internal IDs never appear;
- row-to-pane click mapping covers both lines;
- an empty normalized Codex list suppresses stale legacy subagent rows; and
- Claude Code and OpenCode snapshots remain unchanged.

Required repository verification:

```bash
cargo fmt
cargo fmt --check
cargo test
cargo clippy --all-targets --all-features
cargo build --release
```

## Out of scope

- retaining completed or interrupted child history;
- displaying nicknames when a canonical path is available;
- token usage, reasoning effort, permission mode, or current tool per child;
- actions for spawning, switching, interrupting, or closing agents; and
- treating Codex rollout JSONL as a stable public API.
