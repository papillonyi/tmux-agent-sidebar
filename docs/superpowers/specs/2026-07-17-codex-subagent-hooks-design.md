# Codex Subagent Hooks Design

## Goal

Display currently running Codex subagents in the existing sidebar subagent
tree by consuming Codex `SubagentStart` and `SubagentStop` lifecycle hooks.
The display must reuse the current `agent_type #<id-prefix>` format and remove
each subagent as soon as its matching stop event arrives.

## Scope

- Register Codex `SubagentStart` and `SubagentStop` hooks in the adapter's
  existing `HOOK_REGISTRATIONS` table.
- Parse both events into the existing `AgentEvent::SubagentStart` and
  `AgentEvent::SubagentStop` variants.
- Reuse the existing pane option, lifecycle handlers, tmux query parsing, and
  tree-row rendering without introducing Codex-specific UI state.
- Generate the two new hook entries through the existing `setup codex` output.
- Cover registration, payload parsing, setup generation, and malformed-payload
  behavior with focused tests.

## Out of scope

- Persisting or displaying completed subagents.
- Displaying `last_assistant_message` in the sidebar.
- Reading or displaying `agent_transcript_path` contents.
- Showing subagent progress, prompts, token usage, or tool activity per child.
- Adding a Codex-specific pane option, popup, history view, or UI layout.
- Changing Claude Code subagent presentation or lifecycle behavior.

## Chosen approach

Extend only the Codex adapter and reuse the complete subagent pipeline already
used by Claude Code. This keeps the upstream event normalization
agent-specific while preserving one internal representation and one UI path.

The rejected alternatives are:

1. A Codex-specific state and renderer. This would duplicate the existing
   lifecycle, tmux option, and tree-row logic without adding user-visible value.
2. Process or transcript polling. This would introduce latency and stale-state
   failure modes, and transcript files are not a stable hook interface.

## Data flow

```text
Codex SubagentStart JSON
  -> CodexAdapter::parse("subagent-start", payload)
  -> AgentEvent::SubagentStart { agent_type, agent_id }
  -> on_subagent_start(...)
  -> @pane_subagents += "agent_type:agent_id"
  -> tmux refresh parses it as "agent_type #<first-four-id-characters>"
  -> existing tree row renders the live subagent

Codex SubagentStop JSON
  -> CodexAdapter::parse("subagent-stop", payload)
  -> AgentEvent::SubagentStop { agent_type, agent_id, ... }
  -> on_subagent_stop(...)
  -> remove the entry matching the full agent_id
  -> unset @pane_subagents when the final entry is removed
```

The full `agent_id` remains in the tmux option so parallel subagents of the same
type can be removed precisely. Only the first four characters are displayed;
the shortened value is a visual discriminator, not a semantic task identifier.

## Hook registration

`CodexAdapter::HOOK_REGISTRATIONS` gains two entries with no matcher:

- `SubagentStart` -> `AgentEventKind::SubagentStart`
- `SubagentStop` -> `AgentEventKind::SubagentStop`

The existing setup generator reads this table directly, so `setup codex` will
emit commands for `codex subagent-start` and `codex subagent-stop` without a
second source of hook names.

## Payload mapping and fallback behavior

Codex defines `agent_type` and `agent_id` for both subagent lifecycle events.
The adapter maps them into the existing event variants.

Defensive behavior is intentionally asymmetric:

- If `agent_type` is absent or empty but `agent_id` is present, use the display
  type `subagent`. The UI will render `subagent #<id-prefix>` and the stop event
  can still remove the correct instance.
- If `agent_id` is absent or empty, do not add an active entry. An entry without
  a stable id could not be matched reliably by `SubagentStop`, especially when
  multiple subagents share the same type, and could remain falsely visible.
- A stop event without `agent_id` is a no-op for the same reason.

`SubagentStop` also carries `last_assistant_message` and
`agent_transcript_path`. The adapter normalizes these fields into the
existing transient `AgentEvent` shape, but the current handler discards them;
they are neither written to tmux state nor rendered.

## State and rendering

No new state or rendering code is required. The existing pipeline already:

- stores entries as comma-separated `agent_type:agent_id` values in
  `@pane_subagents`;
- distinguishes parallel instances using the full id;
- converts each entry to `agent_type #<id-prefix>` during tmux parsing;
- renders active entries with the existing `├` / `└` tree rows; and
- removes the pane option after the final subagent stops.

The parent pane remains the owner of cwd, permission mode, session id, and
worktree metadata while any subagents are active, preserving the current
parent-protection behavior.

## Tests

Implementation follows a red-green-refactor sequence.

Focused Codex adapter tests will cover:

- a realistic `SubagentStart` payload producing the correct type and id;
- missing `agent_type` falling back to `subagent` when an id exists;
- a realistic `SubagentStop` payload producing the matching id;
- missing ids remaining `None` in the normalized event; and
- the registration table remaining synchronized with accepted parse arms.

Setup tests will verify that the generated Codex hook configuration contains
both triggers and maps them to the expected `subagent-start` and
`subagent-stop` commands. The existing handler test verifies that a missing id
produces no pane-state change. Existing tmux parsing, parallel-instance, and UI
snapshot tests continue to cover the remaining shared downstream pipeline.

## Rollout

After installing a release containing this change, the user's Codex hook
configuration must include the two generated entries. Running `setup codex`
provides the updated snippet. Because Codex trust is tied to hook definitions,
new or changed hooks must be reviewed and trusted through `/hooks` before they
become active.

## Success criteria

- `/hooks` shows active `SubagentStart` and `SubagentStop` registrations for
  the sidebar.
- Starting a Codex subagent adds one tree row beneath the parent pane.
- Parallel Codex subagents, including identical types, display as distinct
  `agent_type #<id-prefix>` rows.
- A matching stop event removes only the completed subagent.
- The final stop event removes the subagent tree entirely.
- Missing `agent_type` still produces a generic `subagent` row when an id is
  available.
- Missing `agent_id` never creates an entry that can become permanently stale.
- Claude Code behavior and the existing sidebar layout remain unchanged.
