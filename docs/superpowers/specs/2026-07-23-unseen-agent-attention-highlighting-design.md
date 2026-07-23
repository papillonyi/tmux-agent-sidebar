# Unseen Agent Attention Highlighting

## Goal

Make agent panes that need the user's attention immediately visible in the
sidebar. A pane should receive a persistent, full-card highlight when either:

- the agent requires user action, such as permission confirmation, or
- the agent has completed its current turn without a live background task.

The highlight represents an unseen event, not the pane's long-lived execution
status. It remains until the user actually focuses the corresponding tmux pane.

## User-visible behavior

The sidebar renders the entire agent card with a semantic background while its
latest attention event is unseen:

| Attention kind | Background | Status icon | Base pane status |
|---|---|---|---|
| Action required | Dark amber | `!` | Usually `waiting` |
| Completed | Dark green | `✓` | `idle` |

"Entire card" includes every visible row owned by the pane: status, branch and
ports, token usage, task progress, subagents, wait reason, background command,
and prompt or response text. Padding cells use the same background so the card
appears as one continuous highlighted block.

The treatment is static. It does not blink, pulse, play a sound, or move focus.
The icon distinction ensures the meaning is not conveyed by color alone.

### Acknowledgement

Only actual tmux pane focus acknowledges an event:

- Selecting the card with the sidebar cursor does not acknowledge it.
- Focusing the sidebar itself does not acknowledge the previously focused
  agent pane.
- Focusing the target agent pane clears the highlight while leaving its base
  `waiting` or `idle` status unchanged.
- If the event occurs while the target pane is already focused, the next
  refresh acknowledges it immediately, so no persistent highlight remains.
- A later attention event re-arms the highlight even if an earlier event was
  acknowledged.

## Scope

The behavior applies consistently to Claude Code, Codex, and OpenCode panes,
including panes displayed in the sidebar's other-window or other-session
section.

This change does not:

- alter desktop notification delivery or deduplication,
- change agent ordering, filtering, card height, or navigation,
- add animation, sound, or automatic pane selection,
- treat `StopFailure` as successful completion,
- redefine background work as completed, or
- refactor unrelated hook, state, or rendering behavior.

## State model

Replace `PaneInfo.attention: bool` with a typed pane attention value:

```rust
enum PaneAttentionKind {
    ActionRequired,
    Completed,
}

struct PaneAttention {
    kind: PaneAttentionKind,
    event_id: String,
    raw_value: String,
}
```

`None` represents a pane without an unseen attention event. `raw_value` is
retained so acknowledgement can conditionally clear the exact event that the
sidebar observed.

The existing pane-scoped `@pane_attention` option remains the shared source of
truth. This preserves attention across sidebar restarts and lets every sidebar
instance see the same pane state.

### Encoding

New values use:

```text
action_required:<event-id>
completed:<event-id>
```

The event ID is opaque to readers. Writers generate it from the current epoch
milliseconds plus the hook process ID, which distinguishes adjacent hook
processes without adding a dependency or requiring shared in-memory state.

Backward compatibility is intentionally conservative:

- the legacy value `notification` parses as `ActionRequired`,
- any other unknown non-empty value also parses as `ActionRequired`, and
- an empty or unset value parses as no attention.

This preserves the current behavior of treating any non-empty attention marker
as something the user should see.

## Event transitions

Hook handlers use one helper to write a fresh typed attention event and the
existing clear path to remove it.

| Event or transition | Attention result | Status result |
|---|---|---|
| Existing attention-producing notification | Fresh `ActionRequired` | Existing status resolution |
| `PermissionDenied` | Fresh `ActionRequired` | `waiting` |
| `TeammateIdle` | Fresh `ActionRequired` | Existing status behavior |
| `Stop`, no live background shell | Fresh `Completed` | `idle` |
| `Stop`, live background shell | Cleared | `background` |
| `UserPromptSubmit` or resumed activity | Cleared | `running` |
| `StopFailure` | Cleared | `error` |
| Agent or pane teardown | Removed with existing pane metadata | Removed |

The completion marker is written only after the stop handler has resolved that
no background shell remains. A generic `TaskCompleted` notification does not
write a separate attention event; the parent `Stop` transition remains the
single completion source of truth.

## Focus acknowledgement

The current `focused_pane_id` is deliberately sticky while the sidebar itself
has focus so the Git and Activity panels remain stable. That cached value must
not drive attention acknowledgement.

The focus query therefore exposes the pane ID actually observed as active
during the current refresh. The refresh path:

1. reads pane attention values with the normal `list-panes` snapshot,
2. queries the currently active non-sidebar pane,
3. updates the existing sticky `focused_pane_id` when an active pane is
   observed,
4. acknowledges attention only for that newly observed active pane, and
5. renders after acknowledgement has succeeded.

Acknowledgement uses a target-pane helper that clears `@pane_attention` only
when its current raw value still equals the value from the snapshot. This
compare-and-clear operation prevents focus acknowledgement of an older event
from deleting a newer event written during the same refresh.

If compare-and-clear succeeds, the matching in-memory `PaneInfo` is updated to
`None` before rendering, preventing a one-frame stale highlight. If tmux
returns an error or the value has changed, the in-memory attention stays
visible and the next refresh retries or reads the new event.

The existing `after-select-pane` and `after-select-window` SIGUSR1 hooks already
provide prompt refreshes. No new worker, polling interval, or focus hook is
needed.

## Rendering

The row renderer derives one card background before creating any `RowCtx`:

1. unseen `ActionRequired` background,
2. unseen `Completed` background,
3. existing sidebar selection background, or
4. no background.

Attention backgrounds take precedence over the sidebar cursor background so
merely selecting an unseen card cannot hide its semantic state. The existing
left `┃` marker continues to show sidebar selection on top of the attention
background.

Actual pane focus is acknowledged before rendering. Once acknowledged, the
existing active-pane marker and focused Codex enclosure render normally; no
attention/focus style overlap needs a separate visual state.

Every row context, including body rows that currently omit selection
background, receives the derived card background. Background application must
cover content spans, inter-column gaps, left markers, right borders, and
trailing padding without adding columns or changing click-target calculations.

The status row substitutes `!` or `✓` for the normal status icon only while an
unseen attention value is present. Both replacements occupy one display cell,
so title, badge, elapsed-time, wrapping, and narrow-width budgets remain
unchanged.

## Theme

Add two optional tmux color overrides using the existing indexed-color or RGB
parsing path:

```text
@sidebar_color_attention_action_bg
@sidebar_color_attention_completed_bg
```

Defaults:

- action required: `Color::Indexed(58)` (dark amber),
- completed: `Color::Indexed(22)` (dark green).

The existing foreground colors remain in use. Styled snapshots must verify
that active, muted, agent, wait-reason, and response text remain readable on
both default backgrounds.

## Error handling and compatibility

- Empty attention values remain equivalent to no attention.
- Legacy and unknown non-empty values remain visible as action-required
  attention.
- A malformed event ID is still treated as an opaque ID and remains visible.
- A failed or mismatched compare-and-clear keeps the card highlighted.
- Missing focus-query output never acknowledges cached focus.
- Pane teardown continues to clear `@pane_attention` through the existing
  metadata cleanup lists.

These rules favor a stale visible reminder over silently losing a new
attention event.

## Testing

### Hook and transition tests

- permission and notification handlers write fresh `ActionRequired` values,
- a normal stop without a background shell writes `Completed`,
- a stop with a live background shell does not write completion attention,
- user prompt and resumed activity clear stale attention,
- stop failure remains `error` without a completion marker, and
- teardown paths include the typed attention option.

### Parser tests

- parse both new attention kinds,
- preserve opaque event IDs and raw values,
- map legacy `notification` to `ActionRequired`,
- map unknown non-empty values to `ActionRequired`, and
- map empty values to no attention.

### Focus tests

- an actually observed active pane acknowledges its matching event,
- sidebar focus and sidebar cursor selection do not acknowledge,
- cached `focused_pane_id` alone does not acknowledge,
- a changed raw value is not cleared by an older acknowledgement,
- a failed clear leaves attention visible, and
- an event emitted for an already focused pane does not persist after refresh.

### UI tests

Any test that renders a frame uses inline `insta` snapshots.

- styled inline snapshots verify that each row and its padding receive the
  action-required or completed background,
- snapshots cover Claude Code, Codex, and OpenCode,
- snapshots cover selected unseen cards, ordinary cards, acknowledged focused
  cards, multi-line cards, and narrow widths,
- text snapshots lock the `!` and `✓` icons and unchanged layout,
- existing focused-Codex enclosure snapshots remain valid after
  acknowledgement, and
- row widths and click targets remain unchanged.

### Repository verification

Run:

```bash
cargo fmt
cargo test
cargo clippy
cargo build --release
```

## Acceptance criteria

1. A permission or other existing attention-producing event gives the
   corresponding agent card a persistent dark-amber background and `!`.
2. A normal completed turn without live background work gives the card a
   persistent dark-green background and `✓`.
3. The highlight covers every visible row and remains while the user only
   navigates inside the sidebar.
4. Actually focusing the target tmux pane clears the highlight without
   changing the pane's base status.
5. An event that occurs while the pane is already focused produces no
   persistent highlight.
6. A later event re-arms the highlight, and acknowledgement of an older event
   cannot delete it.
7. Attention survives sidebar restart because it remains pane-scoped tmux
   state.
8. Existing notification behavior, pane ordering, layout, and non-attention
   rendering remain unchanged.
