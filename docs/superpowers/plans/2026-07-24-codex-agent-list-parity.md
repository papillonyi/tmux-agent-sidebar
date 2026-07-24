# Codex Agent List Parity Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Show `Main` plus every retained Codex child agent in the sidebar with `/agent`-compatible names, ordering, ids, and working/done/interrupted lifecycle state.

**Architecture:** Keep `@pane_subagents` as the existing active-only compatibility set, append Codex hook transitions to a flock-protected pane journal, and incrementally parse the parent rollout for agent paths, order, interruption, and close events. Merge those sources into a Codex-only runtime list and pass it to a dedicated renderer while leaving Claude Code and OpenCode rows unchanged.

**Tech Stack:** Rust 2024, serde_json, indexmap, libc `flock`, Ratatui, Crossterm, inline insta snapshots, tmux pane options, JSONL rollout and lifecycle files.

## Global Constraints

- Work directly in `/home/ronin/workspace/tmux-agent-sidebar`; do not create a worktree.
- Keep the change Codex-only. Claude Code and OpenCode retain active-only `SubagentInfo` rendering.
- Keep `@pane_subagents` as the active-only parent-protection contract; do not migrate or repurpose its serialized format.
- Do not add dependencies. Use the existing exact `libc = "=0.2.186"`, `serde_json = "=1.0.150"`, and `indexmap = "=2.14.0"` dependencies.
- Treat the parent Codex transcript as an unstable best-effort source. Unknown or malformed lines must be ignored without breaking pane refresh.
- Never use a child `agent_transcript_path` as `@pane_transcript_path`.
- Keep full agent paths and ids in state. Render an eight-character id prefix and truncate only at the row boundary.
- A `SubagentStart` hook means `working`; `SubagentStop` means `done`; only an explicit transcript interruption means `interrupted`; missing lifecycle data means `unknown`.
- Retain completed children until a confirmed successful close or parent-session replacement/teardown.
- Any test that renders a Ratatui frame or row must use an inline `insta::assert_snapshot!`.
- All new documentation and code comments are in English.
- Run `cargo fmt` before every commit.
- Stage only files owned by the current task; never stage the unrelated `.idea/` directory.

---

## File Structure

### New files

- `src/codex_agents/mod.rs`
  - Stable internal agent model.
  - Source merge and status precedence.
  - `CodexAgentTracker` orchestration used by `PaneRuntimeState`.
- `src/codex_agents/journal.rs`
  - Pane journal path.
  - Versioned JSONL lifecycle records.
  - Cross-process flock-protected writer and incremental reader.
- `src/codex_agents/transcript.rs`
  - Incremental parent-rollout reader.
  - `sub_agent_activity` catalog/interruption parsing.
  - Successful `close_agent` call/output correlation.

### Existing files

- `src/lib.rs`
  - Export the new `codex_agents` module.
- `src/event.rs`
  - Add `session_id` to shared subagent lifecycle variants.
- `src/adapter/codex.rs`
  - Preserve Codex subagent parent session ids.
- `src/adapter/claude/mod.rs`
  - Preserve Claude session ids so the shared event shape remains consistent.
- `src/adapter/claude/tests.rs`
  - Update shared variant expectations.
- `src/cli/hook.rs`
  - Skip transcript synchronization for child lifecycle events.
  - Append Codex-only lifecycle records before calling the shared active-set handlers.
- `src/cli/hook/handlers/run.rs`
  - Preserve the Codex active set across parent Stop while retaining existing Claude behavior.
- `src/cli/hook/handlers/subagent.rs`
  - Keep the shared active-set behavior and add Codex journal integration regression coverage through hook dispatch.
- `src/cli/hook/context/meta.rs`
  - Delete the lifecycle journal during normal metadata teardown.
- `src/tmux/query.rs`
  - Delete the journal when a Codex process falls back to a shell.
- `src/state/pane_runtime.rs`
  - Store the tracker and merged `Vec<CodexAgentInfo>` per pane.
- `src/state/refresh.rs`
  - Refresh Codex agent lists from the current session id, parent transcript path, and pane journal.
- `src/ui/panes/row.rs`
  - Route Codex panes to the rich agent renderer.
- `src/ui/panes/row/body.rs`
  - Render `Main`, full `/root/...` names, id prefixes, statuses, and durations.
- `src/ui/panes/row_collector.rs`
  - Pass the per-pane Codex agent slice to row rendering.
- `tests/ui_snapshot.rs`
  - Add full-frame wide/narrow/retained-history snapshots.
- `docs/state-management.md`
  - Document the lifecycle journal, transcript cache, merge rules, and cleanup.

---

### Task 1: Preserve parent session ids and protect the parent transcript path

**Files:**
- Modify: `src/event.rs:76-86`
- Modify: `src/adapter/codex.rs:91-115`
- Modify: `src/adapter/claude/mod.rs:220-241`
- Modify: `src/adapter/claude/tests.rs:250-300`
- Modify: `src/cli/hook.rs:28-49`
- Test: inline tests in `src/adapter/codex.rs`, `src/adapter/claude/tests.rs`, and `src/cli/hook.rs`

**Interfaces:**
- Produces: `AgentEvent::SubagentStart { agent_type, agent_id, session_id }`.
- Produces: `AgentEvent::SubagentStop { agent_type, agent_id, session_id, last_message, transcript_path }`.
- Produces: `sync_event_transcript_path(pane: &str, kind: AgentEventKind, transcript_path: Option<&str>)`.
- Consumes: existing `EventAdapter::transcript_path`, `sync_transcript_path`, and `AgentEvent::kind`.

- [ ] **Step 1: Write failing adapter tests for subagent session ids**

Update the existing Codex expected values so the realistic payloads assert:

```rust
Some(AgentEvent::SubagentStart {
    agent_type: "reviewer".into(),
    agent_id: Some("agent-a81f1234".into()),
    session_id: Some("session-1".into()),
})
```

and:

```rust
Some(AgentEvent::SubagentStop {
    agent_type: "reviewer".into(),
    agent_id: Some("agent-a81f1234".into()),
    session_id: Some("session-1".into()),
    last_message: "Review complete".into(),
    transcript_path: "/tmp/codex-subagent.jsonl".into(),
})
```

For every existing missing-field Codex case, add `session_id: None`. In
`src/adapter/claude/tests.rs`, add `"session_id": "claude-parent"` to the full
start/stop fixtures and expect `Some("claude-parent".into())`; add
`session_id: None` to fixtures without the field.

- [ ] **Step 2: Run the adapter tests and verify the new fields fail to compile**

Run:

```bash
cargo test adapter::codex::tests::subagent --lib
cargo test adapter::claude::tests::subagent --lib
```

Expected: compilation fails because the two `AgentEvent` variants do not yet
have `session_id`.

- [ ] **Step 3: Add the shared event fields and adapter mappings**

Change the variants to:

```rust
SubagentStart {
    agent_type: String,
    agent_id: Option<String>,
    session_id: Option<String>,
},
SubagentStop {
    agent_type: String,
    agent_id: Option<String>,
    session_id: Option<String>,
    last_message: String,
    transcript_path: String,
},
```

In both adapters, map:

```rust
session_id: optional_str(input, "session_id"),
```

Update every constructor/pattern in the repository to include `session_id` or
use `..` where the value is intentionally irrelevant.

- [ ] **Step 4: Write failing hook tests for event-based transcript ownership**

Extract this helper in `src/cli/hook.rs` and test it with `tmux::test_mock`:

```rust
fn sync_event_transcript_path(
    pane: &str,
    kind: crate::event::AgentEventKind,
    transcript_path: Option<&str>,
) {
    if matches!(
        kind,
        crate::event::AgentEventKind::SubagentStart
            | crate::event::AgentEventKind::SubagentStop
    ) {
        return;
    }
    sync_transcript_path(
        pane,
        transcript_path,
        kind == crate::event::AgentEventKind::SessionStart,
    );
}
```

Add one test that seeds `@pane_transcript_path=/tmp/parent.jsonl`, calls the
helper for `SubagentStart` with `/tmp/child.jsonl`, then calls it for
`SubagentStop` with the same child path. Assert the parent value remains after
both calls. Add a second test proving `SessionStart` still sets and clears the
parent value.

- [ ] **Step 5: Run the hook test and verify the old unconditional path fails**

Run:

```bash
cargo test cli::hook::tests::subagent_transcript --lib
```

Expected: the new test fails until `cmd_hook` uses the extracted event-based
helper.

- [ ] **Step 6: Route `cmd_hook` through the tested helper**

Replace the unconditional `sync_transcript_path(...)` call with:

```rust
sync_event_transcript_path(&pane, event.kind(), transcript_path.as_deref());
```

Do not change `sync_transcript_path`'s existing parent-protection behavior for
ordinary events.

- [ ] **Step 7: Run focused tests**

Run:

```bash
cargo test adapter::codex::tests::subagent --lib
cargo test adapter::claude::tests::subagent --lib
cargo test cli::hook::tests::subagent_transcript --lib
```

Expected: all focused tests pass.

- [ ] **Step 8: Format and commit**

```bash
cargo fmt
git add src/event.rs src/adapter/codex.rs src/adapter/claude/mod.rs src/adapter/claude/tests.rs src/cli/hook.rs
git commit -m "fix: preserve parent Codex transcript metadata"
```

---

### Task 2: Add the concurrent Codex lifecycle journal

**Files:**
- Create: `src/codex_agents/mod.rs`
- Create: `src/codex_agents/journal.rs`
- Modify: `src/lib.rs:1-20`
- Modify: `src/cli/hook.rs:130-150`
- Modify: `src/cli/hook/handlers/run.rs:43-65`
- Modify: `src/cli/hook/context/meta.rs:56-75`
- Modify: `src/state/refresh.rs:102-138`
- Modify: `src/tmux/query.rs:394-427`
- Test: inline tests in the created files and modified hook/cleanup modules

**Interfaces:**
- Consumes: Task 1 subagent variants with `session_id`.
- Produces:

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CodexAgentStatus {
    Working,
    Done,
    Interrupted,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CodexAgentInfo {
    pub id: String,
    pub path: String,
    pub fallback_agent_type: String,
    pub status: CodexAgentStatus,
    pub started_at: Option<u64>,
    pub finished_at: Option<u64>,
}
```

- Produces:

```rust
pub fn journal_file_path(pane_id: &str) -> PathBuf;

pub(crate) fn append_lifecycle_event(
    pane_id: &str,
    parent_session_id: &str,
    agent_id: &str,
    agent_type: &str,
    status: CodexAgentStatus,
    occurred_at_ms: u64,
) -> io::Result<()>;

pub(crate) fn remove_journal(pane_id: &str);
```

- Produces:

```rust
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct LifecycleSnapshot {
    pub agent_id: String,
    pub agent_type: String,
    pub status: CodexAgentStatus,
    pub started_at_ms: Option<u64>,
    pub finished_at_ms: Option<u64>,
    pub last_event_at_ms: u64,
}

#[derive(Debug, Clone, Default)]
pub(crate) struct JournalTracker;

impl JournalTracker {
    pub(crate) fn set_context(
        &mut self,
        pane_id: &str,
        parent_session_id: Option<&str>,
    ) -> bool;
    pub(crate) fn refresh(&mut self) -> io::Result<()>;
    pub(crate) fn snapshots(&self) -> &IndexMap<String, LifecycleSnapshot>;
}
```

- [ ] **Step 1: Create failing journal record and fold tests**

In `src/codex_agents/journal.rs`, define tests using a unique pane name derived
from the test name and process id. The first test appends these records:

```rust
append_lifecycle_event(
    pane,
    "parent-1",
    "agent-a",
    "worker",
    CodexAgentStatus::Working,
    10_000,
)?;
append_lifecycle_event(
    pane,
    "parent-1",
    "agent-a",
    "worker",
    CodexAgentStatus::Done,
    25_000,
)?;
```

Refresh a `JournalTracker` for `parent-1` and expect one snapshot with
`started_at_ms == Some(10_000)`, `finished_at_ms == Some(25_000)`, and
`status == Done`.

Add tests for:

- duplicate start preserves the first start;
- duplicate stop preserves the first finish;
- a later start after done changes to working and clears finish;
- stop-before-start creates done with no start;
- a `parent-2` record is ignored while reading `parent-1`;
- missing/malformed/unknown-version lines are ignored; and
- two writer threads appending different ids produce two parseable snapshots.

- [ ] **Step 2: Run the journal tests and verify the module is missing**

Run:

```bash
cargo test codex_agents::journal::tests --lib
```

Expected: compilation fails because `codex_agents` and the journal interfaces
do not exist.

- [ ] **Step 3: Implement the model and flock-protected writer**

Export `pub mod codex_agents;` from `src/lib.rs`. Put the two public types in
`src/codex_agents/mod.rs`.

Use this exact pane encoding:

```rust
pub fn journal_file_path(pane_id: &str) -> PathBuf {
    let encoded = pane_id.replace('%', "_");
    PathBuf::from(format!("/tmp/tmux-agent-agents{encoded}.jsonl"))
}
```

Serialize each line with `serde_json::json!` using:

```text
version
parent_session_id
agent_id
agent_type
state
occurred_at_ms
```

Normalize `agent_id` and `agent_type` with
`crate::cli::sanitize_tmux_value` before serialization so neither source can
introduce a multiline UI value. Use the same normalization in the transcript
parser before ids are used as merge keys.

Map only `Working -> "working"` and `Done -> "done"` in the hook journal.
Reject `Interrupted` and `Unknown` with `io::ErrorKind::InvalidInput`, because
those states are not hook-owned.

Implement an RAII lock guard around:

```rust
unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX) }
```

and unlock with `LOCK_UN` in `Drop`. The reader uses `LOCK_SH`. Propagate lock
errors with `io::Error::last_os_error()`.

- [ ] **Step 4: Implement incremental journal folding**

`JournalTracker` stores:

```rust
path: Option<PathBuf>,
parent_session_id: Option<String>,
offset: u64,
modified: Option<SystemTime>,
partial_line: Vec<u8>,
snapshots: IndexMap<String, LifecycleSnapshot>,
```

`set_context(pane_id, session_id)` resets offsets and snapshots when pane or
session changes. `refresh()` reads from the saved offset under a shared lock,
combines the partial line, parses complete newline-terminated records, and
advances the offset. If file length shrinks, reset to offset zero and rebuild.

Fold records with this deterministic rule:

```rust
match (current.status, incoming.status) {
    (Working, Working) => preserve current.started_at_ms,
    (Done, Done) => preserve current.finished_at_ms,
    (_, Working) => set started_at_ms=incoming time and finished_at_ms=None,
    (_, Done) => set status=Done and set the first finished_at_ms,
}
```

Preserve `IndexMap` insertion order for hook-only fallback rows.

- [ ] **Step 5: Run journal tests**

Run:

```bash
cargo test codex_agents::journal::tests --lib
```

Expected: all journal tests pass, including the concurrent writer test.

- [ ] **Step 6: Write failing hook-dispatch tests**

In `src/cli/hook.rs`, add tests that call `handle_event` directly:

```rust
AgentEvent::SubagentStart {
    agent_type: "worker".into(),
    agent_id: Some("agent-a".into()),
    session_id: Some("parent-1".into()),
}
```

with `agent_name == "codex"`, then refresh a `JournalTracker` and assert
`Working`. Dispatch the matching stop and assert `Done`.

Add three regressions:

- `agent_name == "claude"` does not create a Codex journal;
- missing event session id falls back to seeded `@pane_session_id`; and
- missing/empty agent id writes nothing.

Remove every test journal at test end with `remove_journal`.

- [ ] **Step 7: Integrate Codex-only journal writes**

Add a helper in `src/cli/hook.rs`:

```rust
fn resolve_parent_session_id(pane: &str, event_session_id: Option<&str>) -> Option<String> {
    event_session_id
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
        .or_else(|| {
            let stored = crate::tmux::get_pane_option_value(pane, crate::tmux::PANE_SESSION_ID);
            (!stored.is_empty()).then_some(stored)
        })
}
```

Before calling the shared `on_subagent_start` or `on_subagent_stop`, append a
journal record only when `agent_name == crate::tmux::CODEX_AGENT`, the child id
is non-empty, and a parent session id resolves. Use
`crate::time::now_epoch_millis()` and ignore the returned I/O error so hook
completion is never blocked by display persistence.

- [ ] **Step 8: Preserve the Codex active set across parent Stop**

In `on_stop`, replace the unconditional active-set clear with:

```rust
if ctx.agent != tmux::CODEX_AGENT {
    tmux::unset_pane_option(pane, tmux::PANE_SUBAGENTS);
}
```

Add one test showing Codex Stop preserves `@pane_subagents` and keep the
existing Claude test proving stale active entries are cleared.

- [ ] **Step 9: Add lifecycle-journal cleanup**

Call `crate::codex_agents::remove_journal(pane)` from `clear_all_meta`.
Call it from both dead-process cleanup functions:

```rust
crate::codex_agents::remove_journal(pane_id);
```

Extend the existing `clear_all_meta_drops_every_pane_option_we_own`,
`parse_pane_line_wipes_stale_state_for_codex_shell_pane`, and
add `clear_dead_agent_metadata_removes_codex_agent_journal`. Each test creates
a lifecycle journal first and asserts it is absent afterward.

- [ ] **Step 10: Run focused hook and cleanup tests**

Run:

```bash
cargo test cli::hook::tests::codex_subagent --lib
cargo test cli::hook::handlers::run::tests --lib
cargo test cli::hook::context::meta::tests::clear_all_meta --lib
cargo test tmux::query::tests::parse_pane_line_wipes_stale_state_for_codex_shell_pane --lib
cargo test state::refresh::tests::clear_dead_agent_metadata_removes_codex_agent_journal --lib
```

Expected: all selected tests pass.

- [ ] **Step 11: Format and commit**

```bash
cargo fmt
git add src/lib.rs src/codex_agents/mod.rs src/codex_agents/journal.rs src/cli/hook.rs src/cli/hook/handlers/run.rs src/cli/hook/context/meta.rs src/state/refresh.rs src/tmux/query.rs
git commit -m "feat: persist Codex agent lifecycle"
```

---

### Task 3: Parse the parent Codex agent catalog incrementally

**Files:**
- Create: `src/codex_agents/transcript.rs`
- Modify: `src/codex_agents/mod.rs`
- Test: inline tests in `src/codex_agents/transcript.rs`

**Interfaces:**
- Consumes: `CodexAgentStatus` from Task 2.
- Produces:

```rust
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CatalogAgent {
    pub id: String,
    pub path: String,
    pub interrupted_at_ms: Option<u64>,
}

#[derive(Debug, Clone, Default)]
pub(crate) struct TranscriptTracker;

impl TranscriptTracker {
    pub(crate) fn set_context(
        &mut self,
        path: Option<&str>,
        parent_session_id: Option<&str>,
    ) -> bool;
    pub(crate) fn refresh(&mut self) -> io::Result<()>;
    pub(crate) fn agents(&self) -> &IndexMap<String, CatalogAgent>;
    pub(crate) fn closed_ids(&self) -> &HashSet<String>;
}
```

- [ ] **Step 1: Write realistic transcript parser tests**

Build fixture lines with `serde_json::json!` for:

```json
{
  "type": "event_msg",
  "payload": {
    "type": "sub_agent_activity",
    "occurred_at_ms": 1784257433136,
    "agent_thread_id": "agent-a",
    "agent_path": "/root/task1_review",
    "kind": "started"
  }
}
```

and an `interrupted` line for the same id. Assert full id/path preservation,
first-seen order, duplicate-start idempotency, and
`interrupted_at_ms == Some(1784257531130)`.

Add a `world_state` line that omits `agent-a`; assert the agent remains.
Add malformed JSON, unknown activity kinds, and `interacted`; assert none
delete or reorder existing agents.

- [ ] **Step 2: Add failing successful-close tests**

Append this function call:

```json
{
  "type": "response_item",
  "payload": {
    "type": "function_call",
    "name": "close_agent",
    "arguments": "{\"target\":\"agent-a\"}",
    "call_id": "call-close-a"
  }
}
```

and its successful output:

```json
{
  "type": "response_item",
  "payload": {
    "type": "function_call_output",
    "call_id": "call-close-a",
    "output": "{\"previous_status\":\"completed\"}"
  }
}
```

Assert `closed_ids()` contains `agent-a`. Add cases for exact canonical path,
failed/non-JSON output, an unknown target, and an ambiguous path; only the
successful unambiguous cases close an id.

- [ ] **Step 3: Add failing incremental-file tests**

Use `tempfile` to verify:

- first refresh scans the whole existing rollout, not only the tail;
- a later append adds one agent without duplicating earlier rows;
- a partial final JSON line is held until its newline arrives;
- truncating/replacing the file rebuilds the catalog; and
- changing parent session id with the same path clears and rebuilds the cache.

- [ ] **Step 4: Run the transcript tests and verify failure**

Run:

```bash
cargo test codex_agents::transcript::tests --lib
```

Expected: compilation fails because `TranscriptTracker` does not exist.

- [ ] **Step 5: Implement the defensive incremental reader**

Use the same offset, mtime, and partial-line mechanics as
`CodexUsageTracker`, except the initial read starts at byte zero because the
catalog needs every retained child.

Store agents in `IndexMap<String, CatalogAgent>`. For `started`, insert only on
first sight; if the id already exists and its stored path is empty, fill the
path without moving it. For `interrupted`, update only a known id and keep the
latest `occurred_at_ms`.

Run `agent_thread_id`, `agent_path`, and close targets through
`crate::cli::sanitize_tmux_value` before storage or comparison. Empty normalized
ids are ignored. This keeps every rendered value single-line and gives the
journal and transcript the same merge key.

Store pending close targets in `HashMap<String, String>` keyed by `call_id`.
Parse the JSON string in `arguments`, requiring a non-empty `target`. A close
output succeeds only when its JSON `output` parses and contains a string
`previous_status`. Resolve targets by exact full id first, then exact full
catalog path; require exactly one path match.

Never derive removal from `world_state` or `interacted`.

- [ ] **Step 6: Run transcript tests**

Run:

```bash
cargo test codex_agents::transcript::tests --lib
```

Expected: all transcript tests pass.

- [ ] **Step 7: Format and commit**

```bash
cargo fmt
git add src/codex_agents/mod.rs src/codex_agents/transcript.rs
git commit -m "feat: read Codex agent catalog"
```

---

### Task 4: Merge catalog and lifecycle state into per-pane runtime data

**Files:**
- Modify: `src/codex_agents/mod.rs`
- Modify: `src/state/pane_runtime.rs:1-180`
- Modify: `src/state/refresh.rs:161-230`
- Test: inline tests in `src/codex_agents/mod.rs`, `src/state/pane_runtime.rs`, and `src/state/refresh.rs`

**Interfaces:**
- Consumes: `JournalTracker` from Task 2 and `TranscriptTracker` from Task 3.
- Produces:

```rust
#[derive(Debug, Clone, Default)]
pub(crate) struct CodexAgentTracker;

impl CodexAgentTracker {
    pub(crate) fn refresh(
        &mut self,
        pane_id: &str,
        parent_session_id: Option<&str>,
        transcript_path: Option<&str>,
    );
    pub(crate) fn agents(&self) -> &[CodexAgentInfo];
}
```

- Produces:

```rust
pub fn set_pane_codex_agents(&mut self, pane_id: &str, agents: Vec<CodexAgentInfo>);
pub fn pane_codex_agents(&self, pane_id: &str) -> Option<&[CodexAgentInfo]>;
```

- [ ] **Step 1: Write failing merge tests**

Construct an ordered transcript catalog:

```text
agent-a -> /root/task1_review
agent-b -> /root/task2_owner
```

and journal snapshots in opposite arrival order. Assert the merged list stays
`agent-a`, `agent-b`, then appends journal-only `agent-c`.

Assert:

- catalog path replaces the hook fallback label;
- working uses `started_at_ms / 1000`;
- done uses both start and finish seconds;
- catalog-only is unknown;
- interruption wins only when its event time is later than the lifecycle
  record;
- a later resumed start wins over an older interruption;
- every confirmed closed id is absent; and
- an empty transcript with journal data still yields fallback rows.

- [ ] **Step 2: Run merge tests and verify failure**

Run:

```bash
cargo test codex_agents::tests::merge --lib
```

Expected: failure because the orchestration and merge functions are missing.

- [ ] **Step 3: Implement `CodexAgentTracker` and merge precedence**

The tracker owns:

```rust
transcript: TranscriptTracker,
journal: JournalTracker,
agents: Vec<CodexAgentInfo>,
```

`refresh` sets both contexts. It attempts both reads independently, preserving
each source's last valid cached state on I/O error, then rebuilds `agents`.
When `parent_session_id` is missing, clear `agents` because journal records
cannot be associated safely.

Merge by full id. Use transcript insertion order first, then journal insertion
order for missing ids. Choose status with:

```rust
match (lifecycle, interrupted_at_ms) {
    (Some(snapshot), Some(interrupted))
        if interrupted > snapshot.last_event_at_ms =>
    {
        CodexAgentStatus::Interrupted
    }
    (Some(snapshot), _) => snapshot.status,
    (None, Some(_)) => CodexAgentStatus::Interrupted,
    (None, None) => CodexAgentStatus::Unknown,
}
```

Use the catalog path when non-empty. Otherwise use an empty path and preserve
`fallback_agent_type` for renderer fallback. Divide millisecond timestamps by
1000 with ordinary integer division.

- [ ] **Step 4: Add per-pane storage and accessors**

Extend `PaneRuntimeState` with:

```rust
pub codex_agents: Vec<CodexAgentInfo>,
pub(crate) codex_agent_tracker: CodexAgentTracker,
```

Add setter/accessor methods parallel to the existing Codex token-usage
methods. Extend `entry_mut_creates_default_on_miss` and the accessor round-trip
test to cover an empty list and one explicit agent value.

- [ ] **Step 5: Write failing refresh integration tests**

Create a temporary parent transcript containing one started event and append
working/done journal records for a unique pane/session. Build a Codex
`PaneInfo` with that session id, provide the transcript path map, call the new
refresh helper, and assert `pane_codex_agents` contains the enriched path and
done state.

Add a second test with an unreadable transcript path and a valid journal;
assert the fallback row remains available.

- [ ] **Step 6: Implement `refresh_codex_agents`**

Add:

```rust
fn refresh_codex_agents(&mut self, transcript_paths: &HashMap<String, String>)
```

Collect `(pane_id, session_id, transcript_path)` for Codex panes before taking
mutable runtime-state borrows. For each pane:

```rust
let state = self.pane_state_mut(&pane_id);
state.codex_agent_tracker.refresh(
    &pane_id,
    session_id.as_deref(),
    transcript_path.as_deref(),
);
state.codex_agents = state.codex_agent_tracker.agents().to_vec();
```

Call it immediately after `refresh_codex_token_usage` in the one-second
`refresh()` path.

- [ ] **Step 7: Run focused merge and refresh tests**

Run:

```bash
cargo test codex_agents::tests --lib
cargo test state::pane_runtime::tests --lib
cargo test state::refresh::tests::refresh_codex_agents --lib
```

Expected: all selected tests pass.

- [ ] **Step 8: Format and commit**

```bash
cargo fmt
git add src/codex_agents/mod.rs src/state/pane_runtime.rs src/state/refresh.rs
git commit -m "feat: merge Codex agent runtime state"
```

---

### Task 5: Render `/agent`-style Codex rows

**Files:**
- Modify: `src/ui/panes/row/body.rs:1-145`
- Modify: `src/ui/panes/row.rs:1-180`
- Modify: `src/ui/panes/row_collector.rs:80-135`
- Modify: `tests/ui_snapshot.rs`
- Test: inline row snapshots in `src/ui/panes/row.rs` and full-frame snapshots in `tests/ui_snapshot.rs`

**Interfaces:**
- Consumes: `&[CodexAgentInfo]`, `PaneInfo.session_id`, `RowCtx`, and the
  existing `elapsed_label`, `display_width`, and `truncate_to_width` helpers.
- Produces:

```rust
pub(super) fn codex_agent_rows(
    parent_session_id: Option<&str>,
    agents: &[CodexAgentInfo],
    ctx: &RowCtx<'_>,
    now: u64,
) -> Vec<Line<'static>>;
```

- Extends `render_pane_lines_with_runtime` with:

```rust
codex_agents: Option<&[CodexAgentInfo]>
```

- [ ] **Step 1: Write failing row snapshots for all states**

Create one `codex_agent_rows` snapshot with:

- Main session `019f920c-bee2-7980-9ab1-0476552b63c8`;
- done `/root/task1_owner_contract`, id `019f9260-...`, start 100,
  finish 265;
- working `/root/task2_owner_propagation`, id `019f9264-...`, start 200;
- interrupted `/root/task2_review`, id `019f9267-...`; and
- unknown `/root/task3_builder`, id `019f9268-...`.

Use `now=325` and an inner width that preserves every field. Snapshot the
joined row text, including `Main [default] (current)`, eight-character ids,
`✓ done 2m45s`, `● working 2m5s`, `○ interrupted`, and `? unknown`.

Add a narrow snapshot proving duration is removed before id/status, and the
path is truncated with `…`.

- [ ] **Step 2: Run the row tests and verify failure**

Run:

```bash
cargo test ui::panes::row::tests::codex_agent --lib
```

Expected: compilation fails because the rich renderer does not exist.

- [ ] **Step 3: Implement width-aware Codex rows**

Render no rows when `agents.is_empty()`. Otherwise render Main first and use
tree connectors across Main plus all children.

Use:

```rust
fn id_prefix(id: &str) -> String {
    id.chars().take(8).collect()
}
```

Child fallback label:

```rust
let label = if agent.path.is_empty() {
    agent.fallback_agent_type.clone()
} else {
    agent.path.clone()
};
```

Status text and colors:

```text
Working     "● working"      theme.status_running
Done        "✓ done"         theme.status_idle
Interrupted "○ interrupted"  theme.status_waiting
Unknown     "? unknown"      theme.status_unknown
```

For done duration, call `elapsed_label(started_at, finished_at.unwrap_or(0))`.
For working duration, call `elapsed_label(started_at, now)`. Append duration
only when the row can still preserve connector, one visible path cell, id, and
status. The mandatory right side is the eight-character id prefix followed by
the icon and status word. Truncate the left path to the remaining display-width
budget. A hook-only fallback therefore reads as the agent type on the left and
the same id/status columns as a catalog-enriched row.

- [ ] **Step 4: Route Codex panes without changing other agents**

Add `codex_agents` to `render_pane_lines_with_runtime`. Preserve the test
wrapper by passing `None`.

Replace the unconditional shared subagent rendering with:

```rust
if pane.agent == AgentType::Codex {
    if let Some(agents) = codex_agents.filter(|agents| !agents.is_empty()) {
        out.extend(codex_agent_rows(
            pane.session_id.as_deref(),
            agents,
            ctx,
            now,
        ));
    } else {
        out.extend(subagent_rows(&pane.subagents, ctx, now));
    }
} else {
    out.extend(subagent_rows(&pane.subagents, ctx, now));
}
```

In `row_collector`, obtain:

```rust
let codex_agents = pane_state
    .map(|state| state.codex_agents.as_slice())
    .filter(|agents| !agents.is_empty());
```

and pass it next to token usage.

- [ ] **Step 5: Convert affected row tests to inline snapshots**

Update any existing row-rendering test whose expected Codex output changes.
For the multiple-subagent frame, replace contains-based visual checks with one
inline snapshot of the rendered lines. Do not weaken non-visual state
assertions.

- [ ] **Step 6: Add full-frame wide and narrow snapshots**

In `tests/ui_snapshot.rs`, add:

```rust
state.set_pane_codex_agents("%1", vec![/* four explicit CodexAgentInfo values */]);
```

Create:

- `snapshot_codex_agent_history_matches_agent_panel_ui` at width 64, showing
  Main plus all four statuses in creation order;
- `snapshot_codex_agent_history_narrow_ui` at width 32, showing id/status
  preservation and path truncation; and
- `snapshot_claude_subagents_remain_active_only_ui`, retaining the existing
  `Explore`/`Plan` format without Main/history rows.

All three assertions must use inline `insta::assert_snapshot!`.

- [ ] **Step 7: Run row and frame snapshots**

Run:

```bash
cargo test ui::panes::row::tests::codex_agent --lib
cargo test --test ui_snapshot codex_agent_history
cargo test --test ui_snapshot claude_subagents_remain_active_only
```

Expected: all snapshots pass with no pending `.snap.new` files.

- [ ] **Step 8: Check pending snapshots and commit**

```bash
cargo insta pending-snapshots
cargo fmt
git add src/ui/panes/row/body.rs src/ui/panes/row.rs src/ui/panes/row_collector.rs tests/ui_snapshot.rs
git commit -m "feat: render retained Codex agents"
```

Expected: `cargo insta pending-snapshots` reports no pending snapshots before
the commit.

---

### Task 6: Document, validate, and live-check the complete feature

**Files:**
- Modify: `docs/state-management.md:35-70`
- Modify: `docs/state-management.md:120-145`
- Review only: `README.md`
- Review only: `docs/superpowers/specs/2026-07-24-codex-agent-list-parity-design.md`
- Review only: all implementation files from Tasks 1-5

**Interfaces:**
- Consumes: complete hook, journal, transcript, merge, and UI behavior.
- Produces: documented runtime ownership and final verification evidence.

- [ ] **Step 1: Update state-management documentation**

Add a third per-pane source row:

```text
Codex lifecycle journal | SubagentStart/Stop hooks | Versioned, flock-protected JSONL transitions keyed by pane, parent session id, and full child id
```

Change the `@pane_transcript_path` row to state that only parent events update
it and `SubagentStart`/`SubagentStop` are explicitly excluded.

Add `codex_agents` and `codex_agent_tracker` to the `PaneRuntimeState` table:

- `codex_agents`: merged `/agent`-style rows refreshed every second;
- `codex_agent_tracker`: incremental parent transcript and lifecycle-journal
  cursors plus cached catalog/state.

Document cleanup: normal metadata teardown, dead process cleanup, and shell
fallback remove the journal; session ids isolate stale records before removal.

- [ ] **Step 2: Run documentation and formatting checks**

```bash
cargo fmt
cargo fmt --check
git diff --check
rg -n "<<<<<<<|=======|>>>>>>>" docs src tests
```

Expected: all commands succeed and the conflict-marker search returns no
matches.

- [ ] **Step 3: Run the full test suite**

```bash
cargo test
```

Expected: exit code 0 with all unit and integration tests passing.

- [ ] **Step 4: Run Clippy at the CI strictness**

```bash
cargo clippy --all-targets --all-features -- -D warnings
```

Expected: exit code 0 with no warnings.

- [ ] **Step 5: Build the release binary**

```bash
cargo build --release
```

Expected: exit code 0 and
`target/release/tmux-agent-sidebar` exists.

- [ ] **Step 6: Restart and live-check the local plugin**

Because the plugin directory is normally a symlink to this checkout, toggle
the sidebar off and on after the release build. In a Codex pane:

1. start two named subagents in parallel;
2. open `/agent`;
3. compare Main and child membership/order with the sidebar;
4. allow one child to finish and verify its sidebar row freezes at `done`;
5. interrupt one child and verify `interrupted`;
6. close one retained child and verify only that row disappears; and
7. restart the sidebar and verify the remaining rows reconstruct.

- [ ] **Step 7: Capture runtime evidence**

Resolve the first sidebar pane and first Codex pane from tmux options, then
capture them:

```bash
sidebar_target=$(tmux list-panes -a -F '#{pane_id}|#{@sidebar_pid}' | awk -F'|' '$2 != "" {print $1; exit}')
codex_target=$(tmux list-panes -a -F '#{pane_id}|#{@pane_agent}' | awk -F'|' '$2 == "codex" {print $1; exit}')
test -n "$sidebar_target"
test -n "$codex_target"
tmux capture-pane -p -t "$sidebar_target"
tmux show-options -p -t "$codex_target" -v @pane_session_id
tmux show-options -p -t "$codex_target" -v @pane_transcript_path
```

Expected:

- capture shows Main plus retained child rows;
- pane session id equals the Main id;
- transcript path points to the parent rollout, not any child rollout; and
- sidebar membership matches `/agent`.

- [ ] **Step 8: Review the final diff**

```bash
git status --short
git diff --stat 3d04f72..HEAD
git diff 3d04f72..HEAD -- src docs/state-management.md tests/ui_snapshot.rs
```

Confirm:

- `.idea/` remains untracked and unstaged;
- no unrelated files changed;
- no transcript parser detail leaked outside `src/codex_agents/transcript.rs`;
- non-Codex render flow still uses `SubagentInfo`; and
- no child lifecycle event can write `@pane_transcript_path`.

- [ ] **Step 9: Commit documentation or final test adjustments**

If Step 1 or validation-required fixes changed tracked files:

```bash
cargo fmt
git add docs/state-management.md src/lib.rs src/event.rs src/adapter/codex.rs src/adapter/claude/mod.rs src/adapter/claude/tests.rs src/cli/hook.rs src/cli/hook/handlers/run.rs src/cli/hook/context/meta.rs src/tmux/query.rs src/state/pane_runtime.rs src/state/refresh.rs src/ui/panes/row.rs src/ui/panes/row/body.rs src/ui/panes/row_collector.rs src/codex_agents/mod.rs src/codex_agents/journal.rs src/codex_agents/transcript.rs tests/ui_snapshot.rs
git commit -m "docs: describe Codex agent history state"
```

If only `docs/state-management.md` changed, stage only that file. Do not create
an empty commit.

- [ ] **Step 10: Verify final branch state**

```bash
git status --short --branch
git log --oneline -8
```

Expected: `personal-main` is ahead only by the local feature commits and the
only unrelated worktree entry is `?? .idea/`. Do not push.
