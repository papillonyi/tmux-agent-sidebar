# Active Codex Agent Rows Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Render only currently working Codex subagents as two-line entries with canonical path, role, model, status, and elapsed time.

**Architecture:** Keep hooks, the plugin lifecycle journal, and Codex rollout JSONL as source adapters. Normalize them inside `codex_agents`, filter terminal records before they reach UI state, and render the resulting active-only records without exposing internal IDs.

**Tech Stack:** Rust 2024, serde_json, indexmap, Ratatui, inline insta snapshots, tmux pane options, JSONL lifecycle files.

## Global Constraints

- Work in the current checkout; do not create a worktree.
- Preserve the synthetic Main entry while at least one working child exists.
- Render exactly two lines for Main and for every visible child.
- Render only child records whose merged lifecycle state is `Working`.
- Do not render parent or child internal IDs.
- Keep all rollout schema knowledge in `src/codex_agents/transcript.rs`.
- Preserve `@pane_subagents` parsing and non-Codex compatibility, but never
  use it as a Codex UI fallback.
- Do not change Claude Code or OpenCode subagent rendering.
- Every rendered-frame assertion must use an inline `insta::assert_snapshot!`.
- Run `cargo fmt` before every commit.
- Do not stage the unrelated `.idea/` directory.

---

### Task 1: Validate and enrich nested child rollouts

**Files:**
- Modify: `src/codex_agents/transcript.rs`

**Interfaces:**
- Consumes: child `session_meta`, child `turn_context`, child lifecycle events, and parent `turn_context`.
- Produces: `ChildMetadata { display_name, role, model }`, child lifecycle snapshots, and `TranscriptTracker::main_model()`.

- [x] **Step 1: Write failing nested-validation and metadata tests**

Add tests proving that a depth-two child is valid when its exact child ID and
root `session_id` match even though its immediate `parent_thread_id` differs:

```rust
let payload = json!({
    "id": child_id,
    "session_id": root_session_id,
    "parent_thread_id": immediate_parent_id,
    "agent_path": "/root/task6_implement/lifecycle_probe_alpha",
    "source": {
        "subagent": {
            "thread_spawn": {
                "parent_thread_id": immediate_parent_id,
                "depth": 2,
                "agent_role": "worker"
            }
        }
    }
});
```

Then feed:

```rust
json!({
    "type": "turn_context",
    "payload": { "model": "gpt-5.6-terra" }
})
```

Assert that the child metadata contains the canonical path, role `worker`,
and model `gpt-5.6-terra`. Add a parent-rollout test asserting that the latest
valid `turn_context.model` becomes `main_model()`.

Retain negative cases for wrong root `session_id`, missing child ID,
non-subagent sources, and non-string relationship fields.

- [x] **Step 2: Run focused tests and verify RED**

Run:

```bash
cargo test codex_agents::transcript --lib
```

Expected: the depth-two validation or new metadata/model assertions fail
because current validation requires the immediate parent to equal the root and
does not retain role or model.

- [x] **Step 3: Implement child and parent metadata extraction**

Introduce:

```rust
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct ChildMetadata {
    pub display_name: Option<String>,
    pub role: Option<String>,
    pub model: Option<String>,
}
```

Have `ChildRolloutTracker` populate path/nickname and role after a valid
`session_meta`, then populate model from valid `turn_context` records. Validate
the exact child ID, root `session_id`, and subagent source structure, while
allowing a non-empty immediate parent ID that differs from the root.

Replace the separate child-name map with a child-metadata map. Parse the latest
sanitized parent `turn_context.model` into `TranscriptTracker.main_model`.

- [x] **Step 4: Run focused tests and verify GREEN**

Run:

```bash
cargo test codex_agents::transcript --lib
```

Expected: all transcript tests pass.

### Task 2: Preserve hook model metadata

**Files:**
- Modify: `src/event.rs`
- Modify: `src/adapter/codex.rs`
- Modify: `src/cli/hook.rs`
- Modify: `src/codex_agents/journal.rs`

**Interfaces:**
- Consumes: optional `model` from official Codex `SubagentStart` and `SubagentStop` hook payloads.
- Produces: optional model in the plugin-owned lifecycle snapshot, while remaining compatible with existing version-1 journal lines.

- [x] **Step 1: Write failing adapter and journal tests**

Extend Codex adapter expectations to require:

```rust
AgentEvent::SubagentStart {
    agent_type: "worker".into(),
    agent_id: Some("agent-a".into()),
    session_id: Some("root-session".into()),
    model: Some("gpt-5.6-terra".into()),
}
```

Add journal coverage for a version-1 line with `"model":"gpt-5.6-terra"` and
for an older version-1 line without `model`. The first must retain the model;
the second must parse successfully with `model == None`.

- [x] **Step 2: Run focused tests and verify RED**

Run:

```bash
cargo test adapter::codex --lib
cargo test codex_agents::journal --lib
cargo test cli::hook --lib
```

Expected: compilation or assertion failure because the event and journal
types do not contain model.

- [x] **Step 3: Implement optional model propagation**

Add `model: Option<String>` to Codex subagent lifecycle variants, read it with
`optional_str(input, "model")`, pass it to `append_lifecycle_event`, write it
as an optional journal field, and retain it in `LifecycleSnapshot`.

Keep `JOURNAL_VERSION` at `1`: absent model remains valid, and additional
optional JSON fields are backward compatible.

- [x] **Step 4: Run focused tests and verify GREEN**

Run:

```bash
cargo test adapter::codex --lib
cargo test codex_agents::journal --lib
cargo test cli::hook --lib
```

Expected: all selected tests pass.

### Task 3: Publish active-only normalized records

**Files:**
- Modify: `src/codex_agents/mod.rs`
- Modify: `src/state/pane_runtime.rs`
- Modify: `src/state/refresh.rs`

**Interfaces:**
- Consumes: merged catalog, journal, child lifecycle, child metadata, and parent model.
- Produces: `AgentRecord { internal_id, display_name, role, model, status, started_at, finished_at }` for working children only, plus pane-level `codex_main_model`.

- [x] **Step 1: Write failing normalization and state tests**

Extend expected records with:

```rust
role: "worker".into(),
model: "gpt-5.6-terra".into(),
```

Add one merge test with working, done, interrupted, and unknown children and
assert that `records()` contains only the working child. Add fallback
assertions:

```text
role: child role > hook agent_type > "default"
model: child model > hook model > parent model > "unknown"
```

Add a runtime refresh assertion that `codex_main_model` is populated from the
parent rollout.

- [x] **Step 2: Run focused tests and verify RED**

Run:

```bash
cargo test codex_agents --lib
cargo test state::pane_runtime --lib
cargo test state::refresh::tests::refresh_codex_agents --lib
```

Expected: compilation or assertion failure because records have no role/model,
terminal records remain published, and runtime state has no parent model.

- [x] **Step 3: Implement normalization and active-only filtering**

Add `role` and `model` strings to `AgentRecord`. Resolve metadata inside
`merge_record`, then append the record only when:

```rust
record.status == AgentStatus::Working
```

Expose `CodexAgentTracker::main_model() -> Option<&str>`, copy it into
`PaneRuntimeState.codex_main_model`, and clear it with other Codex runtime
state on pane/session changes.

- [x] **Step 4: Run focused tests and verify GREEN**

Run:

```bash
cargo test codex_agents --lib
cargo test state::pane_runtime --lib
cargo test state::refresh::tests::refresh_codex_agents --lib
```

Expected: all selected tests pass.

### Task 4: Render two lines per active agent

**Files:**
- Modify: `src/ui/panes/row.rs`
- Modify: `src/ui/panes/row/body.rs`
- Modify: `src/ui/panes/row_collector.rs`
- Modify: `tests/ui_snapshot.rs`

**Interfaces:**
- Consumes: working `AgentRecord` values, `codex_main_model`, pane start timestamp, and current time.
- Produces: two rows for Main and two rows for each active child.

- [x] **Step 1: Change inline snapshots first**

Replace historical Codex snapshots with active-only data and expect:

```text
├ Main (current)
│  ● working 2h27m · default · gpt-5.6-sol
└ /root/task6_implement/lifecycle_probe_alpha
   ● working 1m23s · default · gpt-5.6-terra
```

Add a narrow inline snapshot proving that status remains visible before
elapsed time, role, and model are omitted or truncated. Assert that every
rendered line maps to the owning pane.

- [x] **Step 2: Run UI tests and verify RED**

Run:

```bash
cargo test codex_agent_rows --lib
cargo test codex_agent_history --test ui_snapshot
```

Expected: snapshot mismatches show the old one-line historical layout.

- [x] **Step 3: Implement the two-line renderer**

Change `codex_agent_rows` to accept the Main model and pane start timestamp.
For each entry, render an identity line and a metadata line. Use tree
continuation prefixes so the two lines read as one entry:

```text
├ name
│  metadata
└ name
   metadata
```

Build metadata in priority order: `● working`, elapsed time, role, model.
Apply running color to the status, active-text color to elapsed time, and
muted/subagent colors to role/model. Truncate only within the available inner
width and never expose `internal_id`.

- [x] **Step 4: Run UI tests and verify GREEN**

Run:

```bash
cargo test codex_agent_rows --lib
cargo test codex_agent_history --test ui_snapshot
```

Expected: all selected snapshots pass.

### Task 5: Suppress stale legacy Codex fallback

**Files:**
- Modify: `src/ui/panes/row.rs`
- Modify: `tests/ui_snapshot.rs`

- [x] **Step 1: Add a failing live-state regression snapshot**

Create a Codex pane with a stale `@pane_subagents` entry but no normalized
working records. Assert that the legacy row is absent.

- [x] **Step 2: Remove the Codex-only fallback**

Render Codex child rows exclusively from normalized `AgentRecord` values.
Keep the legacy single-line renderer for Claude Code and OpenCode.

- [x] **Step 3: Run focused UI tests**

Run:

```bash
cargo test ui::panes::row --lib
cargo test --test ui_snapshot
```

Expected: all row and full-frame snapshots pass.

### Task 6: Document, verify, commit, and deploy locally

**Files:**
- Modify: `docs/state-management.md`
- Create: `docs/superpowers/specs/2026-07-24-active-codex-agent-rows-design.md`
- Create: `docs/superpowers/plans/2026-07-24-active-codex-agent-rows.md`

**Interfaces:**
- Consumes: final implementation and verification output.
- Produces: updated source contract, a scoped Git commit, and the release binary used by the symlinked tmux plugin.

- [x] **Step 1: Update state documentation**

Document:

```text
hook journal + parent rollout + validated child rollout
    -> CodexAgentTracker
    -> working AgentRecord values + parent model
    -> two-line Sidebar rows
```

State that terminal child records are retained only as merge inputs and never
published to the UI.

- [x] **Step 2: Run repository verification**

Run:

```bash
cargo fmt
cargo fmt --check
cargo test
cargo clippy --all-targets --all-features
cargo build --release
```

Expected: every command exits with status 0.

- [x] **Step 3: Review and commit the scoped diff**

Run:

```bash
git diff --check
git status --short
git diff --stat
```

Stage only files listed by this plan, confirm `.idea/` is absent from the
index, and create the implementation commit after the final `cargo fmt`.

- [x] **Step 4: Refresh the local tmux client**

Because the plugin directory is symlinked to this checkout, the release build
is already the deployed binary. Run:

```bash
tmux refresh-client -S
```

Restart the Sidebar pane if the running process does not reload the binary
automatically, then capture the live pane to confirm terminal rows are absent
and active rows use the two-line layout.

## Self-review

- Spec coverage: nested child identity, role/model sources, active-only
  lifecycle, two-line layout, narrow behavior, ID hiding, Main metadata, and
  local deployment all have explicit tasks.
- Placeholder scan: no deferred implementation or unspecified validation
  steps remain.
- Type consistency: transcript metadata feeds `AgentRecord`; runtime state
  carries parent model; the renderer consumes only normalized state.
