# Tmux Pane Attention Background

## Goal

Extend the existing unseen-attention behavior from the Sidebar card to the
corresponding tmux pane. A pane with an unseen `ActionRequired` or `Completed`
event receives a persistent dark-green background until the same attention
event is cleared.

The actual pane uses the Sidebar completed-card green:

```text
colour22
```

The Sidebar keeps its existing semantic distinction: action-required cards
remain dark amber with `!`, and completed cards remain dark green with `✓`.
The actual pane background is a single generic "this pane needs attention"
signal, so both attention kinds use green.

## User-Visible Behavior

- An unseen action-required or completed event colors the corresponding tmux
  pane background green.
- Focusing that actual pane acknowledges the event through the existing
  compare-and-clear path and restores the pane's previous styles.
- User activity, resumed work, failure handling, and agent teardown restore the
  pane when they clear `@pane_attention`.
- Sidebar cursor movement does not clear the pane background.
- No pane border is changed.
- Other panes and the Sidebar pane are not restyled.

Tmux applies `window-style` and `window-active-style` to cells that use the
terminal's default background. A full-screen terminal application may paint an
explicit background over some or all cells; the plugin does not rewrite
application output to defeat that behavior.

## Architecture

`@pane_attention` remains the only attention source of truth. The Sidebar
refresh path already reads it for every agent pane and acknowledges the
currently focused pane before rendering.

After focus acknowledgement, each refresh reconciles the actual pane style
with the in-memory attention state:

1. A pane with attention is given the green background.
2. A pane without attention is restored if the plugin previously styled it.
3. A pane that leaves agent tracking is restored during metadata teardown.

This refresh-driven design covers Claude Code, Codex, and OpenCode without
duplicating style mutations across their hook handlers. It also repairs stale
style state after a Sidebar restart. The normal refresh and existing SIGUSR1
focus hooks provide the update cadence; no new worker or tmux hook is added.

## Style Preservation

Before styling a pane for the first time, the plugin saves its explicit
pane-level values for:

```text
window-style
window-active-style
```

Two internal pane options hold those backups. A sentinel represents an
originally unset pane option so restoration can remove the temporary override
and return to inherited tmux configuration.

While attention remains present, refreshes reuse the original backup rather
than capturing the plugin's green style as a new baseline. The highlighted
style retains the effective foreground and attributes and replaces only the
background.

When attention disappears, restoration:

1. reinstates the saved explicit value, or unsets the pane option when it was
   originally inherited;
2. removes the internal backup options; and
3. leaves unrelated pane options untouched.

If no backup marker exists, restoration does not alter the pane style. This
prevents teardown or upgrade paths from deleting a style the plugin did not
install.

## Color Source

The actual pane reuses `@sidebar_color_attention_completed_bg` when it contains
a valid Sidebar color value:

- an xterm-256 index such as `22`, converted to `colour22`; or
- a six-digit RGB value such as `#005f00`.

An unset or invalid override falls back to `colour22`. This keeps the default
identical to the completed Sidebar card while preserving the existing theme
override contract.

## Failure Handling

Tmux style writes remain best-effort, matching existing pane-option helpers. A
failed write does not change `@pane_attention`, so Sidebar attention rendering
and acknowledgement continue to work. A later refresh retries reconciliation.

Style restoration happens from both the normal refresh reconciliation and
agent metadata teardown. This prevents a stale green background when a
Codex/OpenCode process returns to the shell or a dead agent pane is pruned.

## Testing

Unit tests cover:

- default and overridden pane-attention color normalization;
- first-time styling with inherited pane styles;
- preservation and restoration of explicit pane styles;
- repeated reconciliation without overwriting backups;
- restoration when attention clears;
- no-op restoration when no plugin backup exists; and
- compare-and-clear acknowledgement followed by immediate style restoration.

Existing typed-attention, focus acknowledgement, hook transition, color, and
UI snapshot tests remain unchanged except where the new reconciliation call is
observed. The full Rust test, format, clippy, and release-build checks remain
the completion gate.

## Scope

This change does not:

- change Sidebar card colors or icons;
- add a second attention state source;
- change attention event generation or focus semantics;
- alter global tmux styles;
- use pane borders, blinking, animation, sound, or automatic focus; or
- attempt to recolor cells explicitly painted by terminal applications.
