# Normalized Codex Agent State Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [x]`) syntax for tracking.

**Goal:** Make the Sidebar consume normalized Codex agent records, show agent paths or names, and hide internal IDs.

**Architecture:** Keep the existing hook journal and rollout parser as independent input adapters. Merge them inside `codex_agents` into `AgentRecord`; make `PaneRuntimeState` and the renderer consume only that type so rollout and hook field choices do not leak into UI code.

**Tech Stack:** Rust 2024, serde_json, indexmap, Ratatui, inline insta snapshots, tmux pane options, JSONL lifecycle files.

## Global Constraints

- Work in the current checkout; do not create a worktree.
- Preserve the existing `@pane_subagents` compatibility behavior.
- Keep all rollout schema knowledge in `src/codex_agents/transcript.rs`.
- Do not render parent or child agent IDs.
- Do not change Claude Code or OpenCode subagent behavior.
- Every rendered-frame assertion must use an inline `insta::assert_snapshot!`.
- Run `cargo fmt` before the final commit.
- Do not stage the unrelated `.idea/` directory.

---

### Task 1: Normalize source output

**Files:**
- Modify: `src/codex_agents/mod.rs`
- Modify: `src/codex_agents/transcript.rs`
- Modify: `src/codex_agents/journal.rs`
- Modify: `src/cli/hook.rs`
- Modify: `src/state/pane_runtime.rs`
- Modify: `src/state/refresh.rs`

**Interfaces:**
- Consumes: parent catalog entries, validated child metadata, child lifecycle snapshots, and hook lifecycle snapshots.
- Produces: `AgentStatus`, `AgentRecord`, and `CodexAgentTracker::records() -> &[AgentRecord]`.

- [x] **Step 1: Write failing normalization tests**

Add tests that expect the tracker to return:

```rust
AgentRecord {
    internal_id: child_id.into(),
    display_name: "/root/task6_implement".into(),
    status: AgentStatus::Working,
    started_at: Some(20),
    finished_at: None,
}
```

Add independent fixtures proving the name priority:

```text
parent path > child path > child nickname > hook type > "subagent"
```

- [x] **Step 2: Run the focused tests and verify RED**

Run:

```bash
cargo test codex_agents --lib
```

Expected: compilation or assertion failure because `AgentRecord`,
`display_name`, child metadata enrichment, or the new fallback behavior is
not implemented.

- [x] **Step 3: Implement the normalized model**

Replace the source-shaped public fields with:

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AgentStatus {
    Working,
    Done,
    Interrupted,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentRecord {
    pub internal_id: String,
    pub display_name: String,
    pub status: AgentStatus,
    pub started_at: Option<u64>,
    pub finished_at: Option<u64>,
}
```

Have the tracker resolve `display_name` before constructing the record. Extend
validated child rollout metadata with a sanitized optional name and expose it
to the merge layer. Keep IDs inside the record for correlation only.

- [x] **Step 4: Run the focused tests and verify GREEN**

Run:

```bash
cargo test codex_agents --lib
cargo test state::refresh::tests::refresh_codex_agents --lib
```

Expected: all selected tests pass.

### Task 2: Render only normalized names and states

**Files:**
- Modify: `src/ui/panes/row.rs`
- Modify: `src/ui/panes/row/body.rs`
- Modify: `src/ui/panes/row_collector.rs`
- Modify: `tests/ui_snapshot.rs`

**Interfaces:**
- Consumes: `Option<&[AgentRecord]>`.
- Produces: a Main row and retained child rows containing display names,
  states, and optional durations, with no internal IDs.

- [x] **Step 1: Change inline snapshots first**

Define the wide expected behavior literally:

```text
├ Main [default] (current)
├ /root/task6_implement                     ● working 2m5s
└ /root/task6_review                         ✓ done 1m12s
```

Add or update the narrow snapshot so status remains visible and the available
name budget grows after IDs are removed.

- [x] **Step 2: Run UI tests and verify RED**

Run:

```bash
cargo test codex_agent_rows --lib
cargo test codex_agent_history --test ui_snapshot
```

Expected: snapshot mismatches show the currently rendered eight-character
parent and child IDs.

- [x] **Step 3: Remove source and ID logic from the renderer**

Make `codex_agent_rows` consume `AgentRecord.display_name` directly. Remove
the ID prefix renderer and its width reservation. Preserve existing status
colors, duration semantics, tree connectors, and row-to-pane click mapping.

- [x] **Step 4: Run UI tests and verify GREEN**

Run:

```bash
cargo test codex_agent_rows --lib
cargo test codex_agent_history --test ui_snapshot
```

Expected: all selected snapshots pass.

### Task 3: Document and verify the stable boundary

**Files:**
- Modify: `docs/state-management.md`
- Modify: `docs/superpowers/specs/2026-07-24-codex-agent-list-parity-design.md`
- Create: `docs/superpowers/specs/2026-07-24-normalized-codex-agent-state-design.md`
- Create: `docs/superpowers/plans/2026-07-24-normalized-codex-agent-state.md`

**Interfaces:**
- Consumes: the final implementation and verification output.
- Produces: an explicit source/adapter/state/UI contract for future changes.

- [x] **Step 1: Update state documentation**

Document three distinct inputs:

```text
hook journal + parent rollout + validated child rollout
    -> CodexAgentTracker
    -> Vec<AgentRecord>
    -> Sidebar rows
```

State that rollout schemas are best-effort adapters and that `internal_id` is
not rendered.

- [x] **Step 2: Run repository verification**

Run:

```bash
cargo fmt
cargo fmt --check
cargo test
cargo clippy --all-targets --all-features
cargo build --release
```

Expected: every command exits with status 0. If Clippy reports pre-existing
warnings, report them exactly rather than claiming a clean `-D warnings`
result.

- [x] **Step 3: Review and commit the scoped diff**

Run:

```bash
git diff --check
git status --short
git diff --stat
```

Stage only files listed in this plan, confirm `.idea/` is absent from the
index, and create one local commit after `cargo fmt`.

## Self-review

- Spec coverage: the plan covers source normalization, name priority, hidden
  IDs, fallback behavior, UI snapshots, documentation, and repository gates.
- Placeholder scan: no deferred implementation steps or unspecified error
  handling remain.
- Type consistency: every downstream consumer uses `AgentRecord` and
  `AgentStatus`; source-specific metadata stays inside `codex_agents`.
