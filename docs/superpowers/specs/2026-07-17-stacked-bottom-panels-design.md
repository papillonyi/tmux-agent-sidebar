# Stacked Bottom Panels Design

## Goal

Show Git status and agent activity at the same time in the sidebar's existing
bottom-panel allocation. Git appears above Activity, and the two cards split
the configured bottom-panel height equally without increasing the total
height.

## Scope

- Replace the mutually exclusive Activity/Git tab rendering with two stacked,
  independently bordered cards.
- Render Git in the upper card and Activity in the lower card on every frame.
- Split the existing `bottom_panel_height` allocation 50/50.
- Preserve one active-panel selection for keyboard and mouse scrolling.
- Preserve the existing per-pane default and remembered selection behavior.
- Poll Git while the bottom region is visible because Git is no longer hidden
  behind a tab.
- Update state documentation and inline UI snapshots for the new layout and
  terminology.

## Out of scope

- Increasing the default bottom-panel height or changing
  `@sidebar_bottom_height` semantics.
- Adding a configurable Git/Activity ratio or draggable divider.
- Adding a compatibility mode that restores the old tab-only view.
- Changing the Git header, file rows, Activity entry format, polling interval,
  or PR cache policy.
- Changing the pane list, pet scene, popup layouts, or agent-event pipeline.

## Chosen approach

Split the current bottom area into two independent bordered cards. The Git
card is first and the Activity card is second. At the default height of 20
rows, each card receives exactly 10 rows, including its border. For an odd
height, Git receives `height / 2` rows and Activity receives the remaining
row, so the difference is at most one row.

This approach was selected because it gives both sections an unambiguous
title, border, content viewport, and click target while satisfying the equal
split requirement. It also lets the current Git and Activity content renderers
continue to receive ordinary inner rectangles.

The rejected alternatives are:

1. One outer card with a titled middle divider. This saves one border row, but
   an even total height cannot produce two equal content viewports once the
   top border, shared divider, and bottom border are counted. It also requires
   custom border intersection and hit-testing logic.
2. A fixed-height Git card with Activity consuming the remainder. This can use
   space more efficiently for small Git states, but it violates the requested
   equal split and makes the layout move as content changes.

## Layout and rendering

`ui::bottom::draw_bottom` divides its existing `area` vertically:

```text
bottom area
  -> Git card      (upper half)
  -> Activity card (lower half)
```

The visual structure is:

```text
╭ Git ─────────────────╮
│ branch / diff / files │
│ ...                   │
╰───────────────────────╯
╭ Activity ─────────────╮
│ tool activity          │
│ ...                   │
╰───────────────────────╯
```

Both content renderers run on every frame that gives their card a nonempty
inner rectangle. Each receives the inner rectangle of its own card, so their
existing scroll calculations are based on the new, smaller viewport. Empty
states remain independent: Git can show `Working tree clean` while Activity
shows `No activity yet`, or vice versa.

The selected card title uses the accent color at all times so the current
scroll target remains visible. When keyboard focus is in the bottom region,
the selected card's border also uses the accent color; the other card keeps the
inactive border color. When focus is elsewhere, both borders use the inactive
color.

The configured bottom height remains authoritative:

- `0` still hides the complete bottom region.
- The default `20` produces two equal 10-row cards.
- Odd values give the extra row to Activity.
- Very small nonzero values are not silently enlarged. Rectangles with no
  drawable content are skipped, and Ratatui clipping provides a safe degraded
  display rather than a panic.

## Selection and input behavior

Although both cards are always visible, one remains selected as the active
scroll target:

- `Shift+Tab` toggles the active card without hiding either card.
- Clicking the `Git` or `Activity` title selects that card.
- Up/down keys and mouse-wheel events in the bottom region scroll the active
  card, preserving the current interaction model.
- Moving down from the last pane row still enters bottom-region focus.
- Moving up from a selected card at scroll offset zero still returns focus to
  the pane list; otherwise it scrolls that card upward.

Existing per-pane selection behavior is preserved. An agent pane without a
saved preference initially selects Activity, while a non-agent pane initially
selects Git. Returning to a pane restores its last selected card.

Because the UI no longer has bottom tabs, related state names become panel
names: `BottomTab` becomes `BottomPanel`, `bottom_tab` becomes
`active_bottom_panel`, and `Focus::ActivityLog` becomes `Focus::BottomPanel`.
Tab-switch and tab-preference helpers receive the corresponding panel-oriented
names. This is a scoped semantic rename, not a behavior redesign.

## Git refresh behavior

The Git card is always visible whenever the bottom region has a usable height,
so the background Git poller remains active instead of being gated by the
selected card. The existing two-second interval and PR cache remain unchanged.
When `bottom_panel_height` is `0`, the Git polling thread is not started.

The `git_tab_active` shared flag and input-path updates to that flag are
removed. Focus changes still trigger the existing immediate Git refresh for
the newly focused pane. This prevents stale visible Git data and removes state
that would otherwise describe a tab that no longer exists.

This creates a small increase in local Git polling compared with leaving the
old Activity tab selected indefinitely. The cost is bounded by the existing
two-second interval, and GitHub-backed PR lookup remains protected by the
existing cache.

## Data and error behavior

No Git or Activity data model changes are required. Both cards continue to
read the focused pane's current Git and Activity state. Scroll offsets remain
independent and are clamped by each renderer after its viewport size changes.

PR hyperlink overlays keep using the Git card's inner origin, so moving Git to
the upper half only changes the rectangle supplied to the existing renderer.
Zero-height rectangles must not register overlays or attempt content drawing.

## Tests

Implementation follows a red-green-refactor sequence.

UI tests use inline `insta::assert_snapshot!` snapshots and cover:

- populated Git and Activity content rendered together, in that order;
- two equal 10-row cards within the default 20-row allocation;
- independent empty states;
- selected Git and selected Activity title/border styling;
- an odd bottom height assigning only one extra row to Activity; and
- safe clipping at a very small configured height.

State and input tests cover:

- `Shift+Tab` changing only the active scroll target;
- title clicks selecting the matching card;
- keyboard and mouse scrolling dispatching to the selected card;
- upward navigation leaving bottom focus only when the selected card is at its
  top; and
- per-pane active-panel defaults and restoration remaining unchanged.

Worker tests cover Git polling without a visibility-selection gate. Existing
Git-content and Activity-content unit tests continue to verify the two
renderers independently.

## Documentation

`docs/state-management.md` is updated to describe two visible bottom panels,
the active scroll target, renamed focus/selection state, independent viewport
sizes, and always-on Git polling while the sidebar runs.

## Success criteria

- Git is visible above Activity whenever the configured bottom height is large
  enough to draw both cards.
- At the default height, the two bordered cards each occupy 10 rows.
- The total bottom-region height does not change.
- Git and Activity keep independent scroll offsets and viewport bounds.
- `Shift+Tab` and title clicks select the scroll target without changing
  visibility.
- The selected target is visually identifiable.
- Git status stays current even when Activity is selected.
- `@sidebar_bottom_height=0` still hides the entire region, and small nonzero
  heights do not panic.
- All UI rendering assertions use inline snapshots.
- `cargo fmt --check`, `cargo test`, `cargo clippy`, and
  `cargo build --release` pass.
