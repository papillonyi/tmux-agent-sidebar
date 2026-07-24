# Codex Agent List Parity Design

> Superseded for implementation boundaries and rendering by
> `2026-07-24-normalized-codex-agent-state-design.md`. This document remains
> as the detailed history of the original `/agent` parity exploration.

## Goal

Make a Codex pane's sidebar agent tree match the useful semantics of Codex's
interactive `/agent` (alias `/subagents`) view:

- show the parent `Main` thread;
- show every retained child agent in creation order, including completed
  children;
- use the Codex agent path, such as `/root/task1_review`, as the primary child
  label;
- identify each row with the agent thread id;
- update `working` and `done` promptly from lifecycle hooks;
- show an explicit `interrupted` state when Codex records one; and
- remove a child only after a successful close operation or when the parent
  session is replaced.

This design supersedes the completed-agent and transcript exclusions in
`2026-07-17-codex-subagent-hooks-design.md` for Codex panes only.

## User-visible behavior

A Codex pane with retained children renders an agent section beneath its
existing parent details:

```text
    ├ Main [default] (current)         019f920c
    ├ /root/task1_owner_contract       019f9260  ✓ done 2m45s
    ├ /root/task1_review               019f9263  ✓ done 1m12s
    ├ /root/task2_owner_propagation    019f9264  ● working 2m05s
    └ /root/task2_review               019f9267  ○ interrupted
```

The exact spacing is width-dependent:

- the full agent path and full UUID remain in the data model;
- the UUID is rendered as an eight-character prefix;
- the path is truncated only when the available row width cannot fit the
  status and id columns; and
- status and id remain visible before optional duration text is sacrificed.

The existing sidebar scroll behavior handles cards made taller by retained
agents. This change does not add agent selection, thread switching, per-agent
scrolling, or mouse actions. It matches `/agent` membership, ordering, identity,
and lifecycle presentation, not its interactive navigation.

Claude Code and OpenCode keep their current active-only subagent rows and
labels.

## Status semantics

Each Codex child has one of four display states:

| State | Source | Display behavior |
| --- | --- | --- |
| `working` | Latest matching `SubagentStart` hook | Green active marker and a live elapsed duration |
| `done` | Latest matching `SubagentStop` hook | Success marker and a frozen duration |
| `interrupted` | Explicit `sub_agent_activity` transcript event with `kind: "interrupted"` | Neutral interrupted marker; no live timer |
| `unknown` | Catalog entry exists but no usable lifecycle signal is available | Neutral unknown marker; no inferred completion |

State changes are idempotent:

- a duplicate start keeps the original start time;
- a start after a terminal state begins a new working interval and clears the
  previous finish time;
- a stop without a preceding start still creates a retained `done` fallback
  record;
- a duplicate stop keeps the first finish time; and
- an explicit interruption overrides a stale `working` state unless a later
  start resumes the same thread.

No state is inferred from absence in a `world_state` snapshot. Those snapshots
can contain only a rolling subset of the current agent tree.

## Source-of-truth split

No single currently available interface provides both the `/agent` identity
list and prompt lifecycle updates for the running interactive Codex process.
The sidebar therefore merges two narrow sources:

1. **Parent transcript catalog**
   - supplies `agent_thread_id`, full `agent_path`, creation order,
     interruption events, and successful close operations;
   - is read only from the parent pane's recorded transcript path; and
   - is isolated behind a defensive parser because Codex documents transcript
     contents as unstable.
2. **Hook lifecycle journal**
   - supplies prompt `working` and `done` transitions immediately from
     `SubagentStart` and `SubagentStop`;
   - persists across sidebar restarts; and
   - is keyed by parent pane, parent session id, and full child agent id.

The parent transcript catalog wins for names and ordering. The hook journal
wins for working/done state and timestamps. An explicit transcript
interruption or successful close is applied after the lifecycle overlay.

If the transcript is absent, unreadable, or temporarily malformed, hook
records still render fallback children as
`<agent_type> #<agent-id-prefix>`. Once the parent transcript becomes readable,
the same full id enriches the existing row without reordering it.

## Why the app-server is not the live source

The Codex app-server is the official deep-integration interface and its thread
schema exposes parent ids, names, roles, and statuses. A separately launched
app-server process does not own or attach to the already-running interactive
CLI's in-memory agent-control tree, however. A read-only probe against the
current CLI session did not return its `/agent` children.

Launching the interactive CLI through a new app-server owner would be an
invasive architectural change outside this sidebar feature. The design keeps
the app-server as a future migration path, not as the current runtime source.

## Parent transcript ownership

`@pane_transcript_path` must always point at the parent Codex rollout.

Today the generic hook entry path synchronizes the transcript before the
`SubagentStart` handler adds the child to `@pane_subagents`. The first child
hook can therefore overwrite the parent path even though later child hooks are
guarded.

The fix is event-based rather than timing-based:

- `SubagentStart` and `SubagentStop` never call
  `sync_transcript_path`;
- parent lifecycle and ordinary parent events continue to synchronize it;
- child `SessionStart` and `SessionEnd` keep the existing parent-protection
  checks; and
- a new parent session id resets the transcript cache and selects only
  lifecycle records for that new session.

The child `agent_transcript_path` carried by `SubagentStop` is not used as the
catalog source.

## Hook lifecycle journal

### Record format

The hook path writes a dedicated, append-only JSONL journal per tmux pane. Each
line has a versioned shape:

```json
{
  "version": 1,
  "parent_session_id": "019f920c-bee2-7980-9ab1-0476552b63c8",
  "agent_id": "019f9264-e63b-7a80-96bb-0b0391cb9cc6",
  "agent_type": "worker",
  "state": "working",
  "occurred_at_ms": 1784862050123
}
```

The normalized `AgentEvent::SubagentStart` and
`AgentEvent::SubagentStop` variants gain the parent session id required to
write this record. If the payload omits the id, the hook reads the current
parent `@pane_session_id`. Records with neither value are ignored rather than
being attached to the wrong session.

The journal uses a pane-safe sibling of the existing activity-log path, for
example `/tmp/tmux-agent-agents_5.jsonl` for pane `%5`.

### Concurrency and durability

Multiple child hooks may execute in separate processes at nearly the same
time. Updating one serialized tmux option would require a racy read-modify-write
cycle, so the durable display state is not stored in `@pane_subagents`.

Journal writers:

- open the file in append mode;
- take an exclusive cross-process `flock` through the existing `libc`
  dependency;
- write one complete newline-terminated JSON record;
- release the lock; and
- tolerate write failures without blocking the Codex hook.

Readers take a shared lock while reading newly appended bytes. This prevents a
partial final line from moving the cache offset past data that has not finished
writing.

`@pane_subagents` remains the active-only compatibility set used by the
existing parent-protection logic and by non-Codex rendering. Codex display
status comes from the journal, so a legacy read-modify-write collision in that
option cannot erase a Codex child from the visible agent list.

### Session isolation and cleanup

Every record carries the parent session id. The reader folds only records
matching the pane's current `@pane_session_id`, so stale data cannot leak into
a later Codex run in the same pane.

The journal is removed by the same dead-pane and agent-process cleanup paths
that remove the activity log. A normal new session does not need to rewrite
history synchronously; session filtering makes old records inert, and cleanup
may compact them opportunistically.

## Parent transcript catalog

### Accepted events

The parser recognizes only the minimal observed shapes needed by this feature:

- `event_msg.payload.type == "sub_agent_activity"` with
  `kind == "started"`:
  upsert `agent_thread_id` and `agent_path` in first-seen order;
- the same payload with `kind == "interrupted"`:
  mark that known id interrupted;
- a successful `close_agent` function call/output pair:
  remove the unambiguously resolved child from the visible catalog; and
- the parent rollout metadata:
  provide the main thread id shown on the `Main` row.

A close target may be a full id or a canonical agent path. It is applied only
after the matching function-call output reports success and only when the
target resolves to exactly one known child. Ambiguous, failed, malformed, and
unknown close events are ignored.

`interacted` activity and `world_state` snapshots do not add, remove, or finish
agents.

### Defensive parsing

Transcript parsing lives in a Codex-specific module with serde structs local to
that module. It must:

- ignore unknown top-level and payload fields;
- ignore malformed JSON lines without discarding already parsed agents;
- never panic on an unknown activity kind or tool output shape;
- treat ids and paths as untrusted display text and pass them through the
  existing single-line sanitization/truncation path; and
- expose fallback behavior independently of the UI renderer.

The rest of the application consumes a stable internal `CodexAgentCatalog`
instead of depending on transcript JSON shapes.

### Incremental refresh

Re-reading a potentially large rollout on every UI frame is not acceptable.
`AppState` keeps a cache per parent transcript containing:

- the current path and parent session id;
- the last fully parsed byte offset;
- the ordered catalog;
- pending close calls waiting for outputs; and
- the last observed file length.

The first observation scans the existing file once. Later refreshes parse only
new complete lines. A changed path, changed parent session id, or file length
smaller than the cached offset resets and rebuilds that cache.

The lifecycle journal is folded with the same incremental pattern. Cache
failure affects only agent enrichment; it must not break pane discovery or
rendering.

## Merge model

The UI-facing Codex agent list is built as follows:

1. Add a synthetic `Main` row from the pane's parent session id.
2. Add transcript catalog entries in first-seen order.
3. Append hook-only ids in their journal first-seen order.
4. Join catalog and lifecycle records by the full child id.
5. Apply the latest explicit interruption by event time.
6. Remove children with a confirmed successful close.
7. Render the remaining rows with width-aware labels and durations.

The merge model stores full values:

```text
CodexAgentInfo {
    id,
    path,
    fallback_agent_type,
    state,
    started_at,
    finished_at,
    first_seen_order,
}
```

The display duration is:

- `now - started_at` for `working`;
- `finished_at - started_at` for `done` when both exist; and
- omitted when the required timestamps are unavailable.

Clock values are clamped so out-of-order or future timestamps never produce a
negative duration.

## Rendering and layout

Codex rows reuse the existing tree connector, color, duration-formatting, and
width-allocation helpers where possible. The old `SubagentInfo` active-only
shape remains available to Claude Code and OpenCode; Codex uses a richer
`CodexAgentInfo` list rather than forcing terminal history into the shared
active-only type.

The `Main` row is rendered only for Codex panes that have at least one child
catalog or lifecycle record. This avoids adding an otherwise redundant line to
every Codex card before any child has been used.

Priority under narrow widths is:

1. connector and status marker;
2. status word;
3. eight-character id prefix;
4. as much of the agent path as fits;
5. duration.

The full `/root/...` path is preserved when it fits. The renderer does not
replace it with the generic hook `agent_type`.

## Error handling

- Missing or unreadable parent transcript: render hook-only fallback rows.
- Transcript line or schema drift: ignore that line and retain prior state.
- Missing lifecycle journal: catalog rows render as `unknown`.
- Journal lock/read/write error: preserve the last cached state and continue
  the normal hook or render path.
- Stop before start: retain a `done` fallback row rather than losing the child.
- Missing child id: do not create a record that cannot be merged safely.
- Parent session replacement: reset the cache and ignore old-session records.
- Dead pane or absent agent process: remove both activity and lifecycle logs.

These failures may reduce detail but must never change the parent pane status
or prevent the sidebar from drawing.

## Testing strategy

Implementation follows red-green-refactor.

### Transcript parser tests

- started events preserve full ids, full paths, and first-seen order;
- duplicate starts are idempotent;
- explicit interruption changes the matching state;
- successful close removes an id;
- failed, ambiguous, and unmatched close calls do not remove an id;
- rolling `world_state` omission does not remove an id;
- malformed and unknown lines are ignored;
- a truncated or replaced transcript rebuilds the cache; and
- incremental parsing reads appended events without duplicating old entries.

### Lifecycle journal tests

- start writes `working` with session, type, id, and timestamp;
- stop writes `done`;
- missing child id writes nothing;
- missing payload session falls back to the pane's parent session;
- a different-session record is ignored;
- stop-before-start yields a retained done row;
- duplicate and resumed transitions obey the status rules;
- concurrent append records remain individually parseable; and
- dead-pane cleanup removes the lifecycle journal.

### Hook ownership tests

- the first `SubagentStart` cannot replace the parent transcript path;
- `SubagentStop` cannot replace it with the child transcript path;
- parent hooks can still set and clear the parent transcript path; and
- existing parent metadata protections remain intact.

### Merge and UI snapshot tests

- catalog names replace generic hook labels by full id;
- hook-only children remain visible;
- Main plus working, done, interrupted, and unknown rows render in order;
- done durations freeze while working durations advance;
- narrow and wide sidebar snapshots preserve id and status priority;
- enough retained agents increase card height and remain reachable through
  existing scrolling; and
- Claude Code and OpenCode snapshots remain unchanged.

Every frame-rendering assertion uses inline `insta::assert_snapshot!`.

## Validation

The completed implementation must pass:

```bash
cargo fmt
cargo fmt --check
cargo test
cargo clippy --all-targets --all-features -- -D warnings
cargo build --release
```

Live validation uses a Codex pane that starts at least two parallel children:

1. compare sidebar membership and order with `/agent`;
2. observe both children as working;
3. let one finish and confirm it remains visible as done;
4. interrupt one and confirm the interrupted state;
5. close a retained child and confirm only that row disappears;
6. restart the sidebar and confirm retained rows and statuses survive; and
7. capture the sidebar with `tmux capture-pane` as runtime evidence.

## Rollout and compatibility

Existing Codex `SubagentStart` and `SubagentStop` hook registrations remain the
installation contract. Users do not need a new hook type, but they must have
the existing generated hooks enabled and trusted.

The change adds a private temporary journal format with an explicit version.
Unknown versions are ignored. No migration of the existing
`@pane_subagents` value is required.

Because transcript parsing is a compatibility bridge over an unstable format,
all schema knowledge stays in one module with fixtures. If Codex later exposes
the running CLI's agent tree through a stable attachable API, that module can be
replaced without changing the hook journal, merge model, or renderer.

## Out of scope

- switching the active Codex thread from the sidebar;
- sending messages to, interrupting, closing, or spawning agents from the
  sidebar;
- rendering child prompts, final messages, token usage, or tool activity;
- parsing child transcript contents;
- changing Codex hook installation or trust UX;
- changing Claude Code or OpenCode subagent history behavior; and
- launching or owning the interactive Codex CLI through app-server.

## Success criteria

- A Codex pane's sidebar list contains `Main` plus every open child shown by
  `/agent`, in the same creation order.
- Completed children remain visible as `done`.
- Hook events update working/done state without waiting for transcript polling.
- Explicit Codex interruption is visible as `interrupted`.
- A successful close removes only the matching child.
- The first child hook never overwrites the parent transcript path.
- Parallel hook delivery cannot lose visible lifecycle records.
- Sidebar restart reconstructs the same retained list from the parent
  transcript and lifecycle journal.
- Transcript drift degrades to hook-only fallback labels instead of breaking
  the sidebar.
- Non-Codex agent rendering remains unchanged.
