# Unseen Agent Attention Highlighting Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Persist unseen action-required and completed events per tmux pane, render the corresponding agent card with a full semantic background, and clear only the event that the user actually acknowledges by focusing the pane.

**Architecture:** Extend the existing pane-scoped `@pane_attention` value into a typed, versioned event consumed by `PaneInfo`. Hook handlers remain the writers, the refresh/focus path performs compare-and-clear acknowledgement, and the existing row contexts apply the semantic background without changing ordinary selection rendering.

**Tech Stack:** Rust 2024, tmux pane options and formats, Ratatui, Crossterm, inline `insta` snapshots.

## Global Constraints

- Work in the current checkout; do not create a worktree.
- Apply the behavior to Claude Code, Codex, and OpenCode panes.
- Only actual tmux pane focus acknowledges attention; sidebar selection and cached focus do not.
- Use a static dark-amber `!` treatment for action required and a static dark-green `✓` treatment for completed.
- Cover every rendered row and padding cell while attention is present.
- Preserve ordinary selection styling, pane ordering, filtering, navigation, card height, desktop notifications, and failure semantics.
- A stop with a live background shell must remain `background` and must not produce completed attention.
- Keep legacy `notification` and unknown non-empty values visible as action required.
- Do not modify or stage `.idea/`.
- Before every commit, run `cargo fmt`.
- Do not push.

---

## File Structure

- `src/tmux/types.rs`: Own typed pane-attention parsing, encoding, icon selection, and `PaneInfo.attention`.
- `src/tmux/options.rs`: Own the pane-option compare-and-clear primitive and its tmux test mock.
- `src/tmux/query.rs`: Parse raw `@pane_attention` values into `PaneInfo`.
- `src/tmux.rs`: Re-export the new attention types, color option constants, and compare-and-clear helper.
- `src/cli/mod.rs`: Generate event IDs and write or clear typed pane attention.
- `src/cli/hook.rs`: Keep `TaskCompleted` desktop notification dispatch separate from pane completion attention.
- `src/cli/hook/handlers/{attention,run,session}.rs`, `src/cli/hook/context/pending.rs`, `src/cli/hook/activity.rs`: Apply the approved attention transitions.
- `src/state/focus.rs`: Distinguish newly observed focus from sticky display focus and acknowledge the exact observed event.
- `src/state/refresh.rs`: Invoke focus acknowledgement after rebuilding pane state and before rendering.
- `src/ui/colors.rs`: Own semantic attention foreground/background colors and tmux overrides.
- `src/ui/panes/row.rs`, `src/ui/panes/row/status.rs`: Apply attention backgrounds to all rows and substitute semantic icons.
- `tests/{test_helpers,color_tests,styled_tests,ui_snapshot}.rs` and inline module tests: Lock typed state, transitions, focus behavior, style coverage, and unchanged layout.
- `docs/state-management.md`: Document the typed pane option and acknowledgement data flow.

---

### Task 1: Typed Attention Model and Hook Transitions

**Files:**
- Modify: `src/tmux/types.rs`
- Modify: `src/tmux/query.rs`
- Modify: `src/tmux.rs`
- Modify: `src/cli/mod.rs`
- Modify: `src/cli/hook.rs`
- Modify: `src/cli/hook/handlers/attention.rs`
- Modify: `src/cli/hook/handlers/run.rs`
- Modify: `src/cli/hook/handlers/session.rs`
- Modify: `src/cli/hook/context/pending.rs`
- Modify: `src/cli/hook/activity.rs`
- Modify: pane constructors in `src/` and `tests/`

**Interfaces:**
- Produces: `PaneAttentionKind::{ActionRequired, Completed}`.
- Produces: `PaneAttention::parse(raw: &str) -> Option<PaneAttention>`.
- Produces: `PaneAttention::encode(kind: PaneAttentionKind, event_id: &str) -> String`.
- Produces: `PaneInfo.attention: Option<PaneAttention>`.
- Produces: `set_attention(pane: &str, kind: Option<PaneAttentionKind>)`.

- [ ] **Step 1: Add failing parser and query tests**

Add focused tests in `src/tmux/types.rs`:

```rust
#[test]
fn pane_attention_parses_typed_and_legacy_values() {
    let action = PaneAttention::parse("action_required:1700000000000-7").unwrap();
    assert_eq!(action.kind, PaneAttentionKind::ActionRequired);
    assert_eq!(action.event_id, "1700000000000-7");
    assert_eq!(action.raw_value, "action_required:1700000000000-7");

    let completed = PaneAttention::parse("completed:1700000000001-8").unwrap();
    assert_eq!(completed.kind, PaneAttentionKind::Completed);
    assert_eq!(completed.event_id, "1700000000001-8");

    assert_eq!(
        PaneAttention::parse("notification").unwrap().kind,
        PaneAttentionKind::ActionRequired
    );
    assert_eq!(
        PaneAttention::parse("future-value").unwrap().kind,
        PaneAttentionKind::ActionRequired
    );
    assert!(PaneAttention::parse("").is_none());
}

#[test]
fn pane_attention_encode_round_trips() {
    let raw = PaneAttention::encode(PaneAttentionKind::Completed, "123-9");
    assert_eq!(raw, "completed:123-9");
    assert_eq!(
        PaneAttention::parse(&raw).unwrap().kind,
        PaneAttentionKind::Completed
    );
}
```

Update `src/tmux/query.rs::parse_pane_line_full_fields` and add a typed-attention
case that sets field `PANE_ATTENTION` to `completed:123-9` and expects
`PaneAttentionKind::Completed`.

Run:

```bash
cargo test pane_attention -- --nocapture
```

Expected: compilation fails because the attention types and `Option` field do
not exist yet.

- [ ] **Step 2: Implement the attention types and update pane parsing**

Add to `src/tmux/types.rs`:

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PaneAttentionKind {
    ActionRequired,
    Completed,
}

impl PaneAttentionKind {
    pub fn prefix(self) -> &'static str {
        match self {
            Self::ActionRequired => "action_required",
            Self::Completed => "completed",
        }
    }

    pub fn icon(self) -> &'static str {
        match self {
            Self::ActionRequired => "!",
            Self::Completed => "✓",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PaneAttention {
    pub kind: PaneAttentionKind,
    pub event_id: String,
    pub raw_value: String,
}

impl PaneAttention {
    pub fn parse(raw: &str) -> Option<Self> {
        if raw.is_empty() {
            return None;
        }
        let (kind, event_id) = match raw.split_once(':') {
            Some(("action_required", id)) => (PaneAttentionKind::ActionRequired, id),
            Some(("completed", id)) => (PaneAttentionKind::Completed, id),
            _ => (PaneAttentionKind::ActionRequired, raw),
        };
        Some(Self {
            kind,
            event_id: event_id.to_string(),
            raw_value: raw.to_string(),
        })
    }

    pub fn encode(kind: PaneAttentionKind, event_id: &str) -> String {
        format!("{}:{event_id}", kind.prefix())
    }
}
```

Change `PaneInfo.attention` to `Option<PaneAttention>`, parse it with
`PaneAttention::parse`, re-export both types from `src/tmux.rs`, and change
every `attention: false` constructor to `attention: None`.

Run:

```bash
cargo test pane_attention -- --nocapture
cargo test parse_pane_line_full_fields -- --nocapture
```

Expected: PASS.

- [ ] **Step 3: Add failing hook transition tests**

Extend the handler tests to assert parsed kinds rather than literal generated
event IDs:

```rust
fn stored_attention(pane: &str) -> Option<PaneAttention> {
    PaneAttention::parse(&tmux::get_pane_option_value(
        pane,
        tmux::PANE_ATTENTION,
    ))
}

#[test]
fn on_permission_denied_sets_action_required_attention() {
    let _guard = tmux::test_mock::install();
    let pane = "%PERMISSION_ATTENTION";
    let ctx = AgentContext {
        agent: "claude",
        cwd: "/repo",
        permission_mode: "default",
        worktree: &None,
        session_id: &None,
    };
    let notifications = desktop_notification::DesktopNotificationSettings {
        enabled: false,
        events: Default::default(),
    };
    on_permission_denied(pane, &ctx, &notifications);
    assert_eq!(
        stored_attention(pane).unwrap().kind,
        PaneAttentionKind::ActionRequired
    );
}

#[test]
fn on_stop_without_background_sets_completed_attention() {
    let _guard = tmux::test_mock::install();
    let pane = "%STOP_COMPLETED_ATTENTION";
    let ctx = AgentContext {
        agent: "claude",
        cwd: "/repo",
        permission_mode: "default",
        worktree: &None,
        session_id: &None,
    };
    on_stop(
        pane,
        &ctx,
        "done",
        None,
        &desktop_notification::DesktopNotificationSettings {
            enabled: false,
            events: Default::default(),
        },
    );
    assert_eq!(
        stored_attention(pane).unwrap().kind,
        PaneAttentionKind::Completed
    );
}

#[test]
fn on_stop_with_background_clears_attention() {
    let _guard = tmux::test_mock::install();
    let pane = "%STOP_BACKGROUND_ATTENTION";
    tmux::test_mock::set(pane, tmux::PANE_BG_CMD, "cargo test");
    tmux::test_mock::set(
        pane,
        tmux::PANE_ATTENTION,
        "action_required:old-event",
    );
    let ctx = AgentContext {
        agent: "claude",
        cwd: "/repo",
        permission_mode: "default",
        worktree: &None,
        session_id: &None,
    };
    on_stop(
        pane,
        &ctx,
        "done",
        None,
        &desktop_notification::DesktopNotificationSettings {
            enabled: false,
            events: Default::default(),
        },
    );
    assert!(stored_attention(pane).is_none());
}
```

Add this dispatch test to `src/cli/hook.rs` to prove `TaskCompleted` alone does
not write `PANE_ATTENTION`; it still calls the existing desktop notification
handler:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn task_completed_does_not_write_pane_attention() {
        let _guard = tmux::test_mock::install();
        let pane = "%TASK_COMPLETED_ONLY";
        assert_eq!(
            handle_event(
                pane,
                "claude",
                AgentEvent::TaskCompleted {
                    task_id: "task-1".into(),
                    task_subject: "finish tests".into(),
                },
            ),
            0,
        );
        assert!(!tmux::test_mock::contains(pane, tmux::PANE_ATTENTION));
    }
}
```

Run:

```bash
cargo test on_permission_denied_sets_action_required_attention -- --nocapture
cargo test on_stop_without_background_sets_completed_attention -- --nocapture
cargo test task_completed -- --nocapture
```

Expected: FAIL because handlers still write `notification` or clear attention.

- [ ] **Step 4: Implement typed hook writes**

Change the helper in `src/cli/mod.rs`:

```rust
fn set_attention(pane: &str, kind: Option<tmux::PaneAttentionKind>) {
    match kind {
        None => tmux::unset_pane_option(pane, tmux::PANE_ATTENTION),
        Some(kind) => {
            let event_id = format!(
                "{}-{}",
                crate::time::now_epoch_millis(),
                std::process::id()
            );
            let value = tmux::PaneAttention::encode(kind, &event_id);
            tmux::set_pane_option(pane, tmux::PANE_ATTENTION, &value);
        }
    }
}
```

Use `Some(PaneAttentionKind::ActionRequired)` in existing attention-producing
handlers. Use `None` in clear paths. In `on_stop`, resolve and write the base
status first, then write `Completed` only when `bg_shell_live` is false. Remove
the direct attention write from the `AgentEvent::TaskCompleted` dispatch arm.

Keep `set_status`'s existing running/idle cleanup as a defensive clear; the
completion write must occur after `set_status("idle")`.

Run:

```bash
cargo test pane_attention -- --nocapture
cargo test on_notification -- --nocapture
cargo test on_permission_denied -- --nocapture
cargo test on_stop -- --nocapture
cargo test task_completed -- --nocapture
```

Expected: PASS.

- [ ] **Step 5: Format, verify, and commit Task 1**

```bash
cargo fmt
cargo test pane_attention -- --nocapture
cargo test on_stop -- --nocapture
git diff --check
git add src/cli/mod.rs src/cli/hook.rs src/cli/hook/activity.rs src/cli/hook/context/pending.rs src/cli/hook/handlers/attention.rs src/cli/hook/handlers/run.rs src/cli/hook/handlers/session.rs src/tmux.rs src/tmux/types.rs src/tmux/query.rs src/group.rs src/state.rs src/state/focus.rs src/state/pet.rs src/state/refresh.rs src/state/filter.rs src/state/popup.rs src/state/tab.rs src/ui/panes/row.rs src/ui/panes/row_collector.rs src/ui/panes/filter_bar.rs tests/test_helpers.rs tests/state_tests.rs tests/ui_snapshot.rs
git commit -m "feat: track typed unseen agent attention"
```

Stage only files changed for the typed model and transition work; do not stage
`.idea/`, the plan, or unrelated files.

---

### Task 2: Focus Acknowledgement and Compare-and-Clear

**Files:**
- Modify: `src/tmux/options.rs`
- Modify: `src/tmux.rs`
- Modify: `src/state/focus.rs`
- Modify: `src/state/refresh.rs`

**Interfaces:**
- Consumes: `PaneAttention.raw_value`.
- Produces: `clear_pane_option_if_value(pane: &str, key: &str, expected: &str) -> bool`.
- Produces: `find_focused_pane(&mut self) -> Option<String>`, returning only a pane observed during this call while preserving sticky display focus.
- Produces: `record_observed_focus(&mut self, observed: Option<String>) -> Option<String>` for pure focus-state testing.
- Produces: `acknowledge_pane_attention_with<F>(&mut self, pane_id: &str, clear: F) -> bool`, where `F: FnMut(&str, &str) -> bool`.

- [ ] **Step 1: Add failing compare-and-clear tests**

Add to `src/tmux/options.rs` tests:

```rust
#[test]
fn clear_pane_option_if_value_clears_matching_mock_value() {
    let _guard = test_mock::install();
    test_mock::set("%1", PANE_ATTENTION, "completed:1-1");
    assert!(clear_pane_option_if_value(
        "%1",
        PANE_ATTENTION,
        "completed:1-1"
    ));
    assert!(!test_mock::contains("%1", PANE_ATTENTION));
}

#[test]
fn clear_pane_option_if_value_preserves_newer_mock_value() {
    let _guard = test_mock::install();
    test_mock::set("%1", PANE_ATTENTION, "action_required:2-2");
    assert!(!clear_pane_option_if_value(
        "%1",
        PANE_ATTENTION,
        "completed:1-1"
    ));
    assert_eq!(
        test_mock::get("%1", PANE_ATTENTION).as_deref(),
        Some("action_required:2-2")
    );
}
```

Run:

```bash
cargo test clear_pane_option_if_value -- --nocapture
```

Expected: FAIL because the helper does not exist.

- [ ] **Step 2: Implement compare-and-clear**

Extend the test mock with an intercept that compares and removes in one mutable
borrow. For production, use one tmux `set-option -p -F` command whose value is a
server-side conditional format: it expands to empty only when the current raw
value equals the expected safe typed or legacy value, otherwise it expands to
the current value. Read the option once afterward: empty means acknowledged;
a different non-empty value means a newer event won and must remain.

Expose:

```rust
pub(super) fn intercept_clear_if_value(
    pane: &str,
    key: &str,
    expected: &str,
) -> Option<bool> {
    MOCK.with(|mock| {
        let mut guard = mock.borrow_mut();
        let store = guard.as_mut()?;
        let slot = (pane.to_string(), key.to_string());
        match store.get(&slot).cloned() {
            None => Some(true),
            Some(current) if current == expected => {
                store.remove(&slot);
                Some(true)
            }
            Some(_) => Some(false),
        }
    })
}

fn safe_tmux_attention_value(value: &str) -> bool {
    value
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '_' | ':' | '-'))
}

pub fn clear_pane_option_if_value(
    pane: &str,
    key: &str,
    expected: &str,
) -> bool {
    #[cfg(test)]
    if let Some(cleared) = test_mock::intercept_clear_if_value(pane, key, expected) {
        return cleared;
    }

    if safe_tmux_attention_value(expected) {
        let keep_newer = format!(
            "#{{?#{{==:#{{{key}}},{expected}}},,#{{{key}}}}}"
        );
        if run_tmux(&[
            "set-option",
            "-p",
            "-F",
            "-t",
            pane,
            key,
            &keep_newer,
        ])
        .is_none()
        {
            return false;
        }
        return get_pane_option_value(pane, key).is_empty();
    }

    if get_pane_option_value(pane, key) != expected {
        return false;
    }
    unset_pane_option(pane, key);
    get_pane_option_value(pane, key).is_empty()
}
```

Generated typed values and the legacy `notification` value use the atomic tmux
format path. For an unknown value containing tmux format delimiters, fall back
to a guarded read/compare/unset so legacy compatibility remains usable without
format injection.

Run:

```bash
cargo test clear_pane_option_if_value -- --nocapture
```

Expected: PASS.

- [ ] **Step 3: Add failing focus acknowledgement tests**

Add tests in `src/state/focus.rs`:

```rust
#[test]
fn acknowledgement_clears_matching_observed_attention() {
    let mut state = state_with_attention("%1", "completed:1-1");
    let cleared = state.acknowledge_pane_attention_with("%1", |pane, expected| {
        assert_eq!(pane, "%1");
        assert_eq!(expected, "completed:1-1");
        true
    });
    assert!(cleared);
    assert!(state.pane_by_id("%1").unwrap().attention.is_none());
}

#[test]
fn acknowledgement_keeps_attention_when_value_changed() {
    let mut state = state_with_attention("%1", "completed:1-1");
    let cleared = state.acknowledge_pane_attention_with("%1", |_, _| false);
    assert!(!cleared);
    assert!(state.pane_by_id("%1").unwrap().attention.is_some());
}

#[test]
fn cached_focus_without_new_observation_does_not_acknowledge() {
    let mut state = state_with_attention("%1", "completed:1-1");
    state.focus_state.focused_pane_id = Some("%1".into());
    assert!(state.record_observed_focus(None).is_none());
    assert_eq!(state.focus_state.focused_pane_id.as_deref(), Some("%1"));
    assert!(state.pane_by_id("%1").unwrap().attention.is_some());
}
```

Add a pure observation test:

```rust
#[test]
fn newly_observed_focus_updates_sticky_focus_and_is_returned() {
    let mut state = AppState::new("%99".into());
    assert_eq!(
        state.record_observed_focus(Some("%2".into())).as_deref(),
        Some("%2")
    );
    assert_eq!(state.focus_state.focused_pane_id.as_deref(), Some("%2"));
}
```

Run:

```bash
cargo test acknowledgement -- --nocapture
cargo test find_focused_pane -- --nocapture
```

Expected: FAIL because the acknowledgement API and observed-focus return value
do not exist.

- [ ] **Step 4: Implement observed-focus acknowledgement**

Add a mutable pane lookup and acknowledgement method:

```rust
pub fn pane_by_id_mut(&mut self, pane_id: &str) -> Option<&mut tmux::PaneInfo> {
    self.repo_groups
        .iter_mut()
        .flat_map(|group| group.panes.iter_mut())
        .find_map(|(pane, _)| (pane.pane_id == pane_id).then_some(pane))
}

pub(crate) fn acknowledge_pane_attention_with<F>(
    &mut self,
    pane_id: &str,
    mut clear: F,
) -> bool
where
    F: FnMut(&str, &str) -> bool,
{
    let Some(expected) = self
        .pane_by_id(pane_id)
        .and_then(|pane| pane.attention.as_ref())
        .map(|attention| attention.raw_value.clone())
    else {
        return false;
    };
    if !clear(pane_id, &expected) {
        return false;
    }
    if let Some(pane) = self.pane_by_id_mut(pane_id) {
        pane.attention = None;
    }
    true
}
```

Make `find_focused_pane` return the newly observed pane ID while retaining its
existing sticky update through a pure helper:

```rust
pub(crate) fn record_observed_focus(
    &mut self,
    observed: Option<String>,
) -> Option<String> {
    if let Some(ref id) = observed {
        self.focus_state.focused_pane_id = Some(id.clone());
    }
    observed
}

pub fn find_focused_pane(&mut self) -> Option<String> {
    let observed = tmux::find_active_pane(&self.tmux_pane).map(|(id, _)| id);
    self.record_observed_focus(observed)
}
```

In `apply_session_snapshot`, call acknowledgement only for that return value
and use `tmux::clear_pane_option_if_value` with `PANE_ATTENTION`.

Run:

```bash
cargo test acknowledgement -- --nocapture
cargo test focus -- --nocapture
cargo test apply_session_snapshot -- --nocapture
```

Expected: PASS.

- [ ] **Step 5: Format, verify, and commit Task 2**

```bash
cargo fmt
cargo test clear_pane_option_if_value -- --nocapture
cargo test acknowledgement -- --nocapture
cargo test focus -- --nocapture
git diff --check
git add src/tmux/options.rs src/tmux.rs src/state/focus.rs src/state/refresh.rs
git commit -m "feat: acknowledge attention on pane focus"
```

---

### Task 3: Full-card Attention Rendering

**Files:**
- Modify: `src/tmux/options.rs`
- Modify: `src/tmux.rs`
- Modify: `src/ui/colors.rs`
- Modify: `src/ui/panes/row.rs`
- Modify: `src/ui/panes/row/status.rs`
- Modify: `tests/test_helpers.rs`
- Modify: `tests/color_tests.rs`
- Modify: `tests/styled_tests.rs`
- Modify: `tests/ui_snapshot.rs`

**Interfaces:**
- Consumes: `PaneInfo.attention: Option<PaneAttention>`.
- Produces: `ColorTheme.attention_action_bg` and `ColorTheme.attention_completed_bg`.
- Produces: `ColorTheme::attention_bg(kind: PaneAttentionKind) -> Color`.
- Produces: `ColorTheme::attention_fg(kind: PaneAttentionKind) -> Color`.
- Produces: tmux options `@sidebar_color_attention_action_bg` and `@sidebar_color_attention_completed_bg`.

- [ ] **Step 1: Add failing theme and styled snapshots**

Add theme assertions:

```rust
#[test]
fn attention_background_defaults() {
    let theme = ColorTheme::default();
    assert_eq!(theme.attention_action_bg, Color::Indexed(58));
    assert_eq!(theme.attention_completed_bg, Color::Indexed(22));
}
```

Add inline styled snapshots in `tests/styled_tests.rs` for:

- a selected, multi-line Claude card with `ActionRequired`,
- a non-selected, multi-line Codex card with `Completed`, and
- an OpenCode card proving the shared path.

Start with this deliberate red inline snapshot for the selected action-required
case:

```rust
#[test]
fn snapshot_attention_action_selected_full_card_styled() {
    let mut pane = make_pane(AgentType::Claude, PaneStatus::Waiting);
    pane.attention = PaneAttention::parse("action_required:1-1");
    pane.wait_reason = "permission_denied".into();
    pane.prompt = "Allow cargo test?".into();

    let mut state = make_state(vec![]);
    state.repo_groups = vec![make_repo_group("project", vec![pane])];
    state.rebuild_row_targets();
    state.focus_state.sidebar_focused = true;
    state.focus_state.focus = Focus::Panes;
    state.focus_state.focused_pane_id = None;
    state.global.selected_pane_row = 0;
    state.bottom_panel_height = 0;

    insta::assert_snapshot!(
        render_to_styled_string(&mut state, 32, 12),
        @""
    );
}
```

Add the remaining deliberate red inline snapshots:

```rust
#[test]
fn snapshot_attention_completed_codex_full_card_styled() {
    let mut pane = make_pane(AgentType::Codex, PaneStatus::Idle);
    pane.attention = PaneAttention::parse("completed:2-2");
    pane.prompt = "All tests pass.".into();
    pane.prompt_is_response = true;

    let mut state = make_state(vec![]);
    state.repo_groups = vec![make_repo_group("project", vec![pane])];
    state.rebuild_row_targets();
    state.focus_state.sidebar_focused = true;
    state.focus_state.focus = Focus::BottomPanel;
    state.focus_state.focused_pane_id = None;
    state.bottom_panel_height = 0;

    insta::assert_snapshot!(
        render_to_styled_string(&mut state, 32, 12),
        @""
    );
}

#[test]
fn snapshot_attention_action_opencode_full_card_styled() {
    let mut pane = make_pane(AgentType::OpenCode, PaneStatus::Waiting);
    pane.attention = PaneAttention::parse("action_required:3-3");
    pane.wait_reason = "permission".into();

    let mut state = make_state(vec![]);
    state.repo_groups = vec![make_repo_group("project", vec![pane])];
    state.rebuild_row_targets();
    state.focus_state.sidebar_focused = true;
    state.focus_state.focus = Focus::BottomPanel;
    state.focus_state.focused_pane_id = None;
    state.bottom_panel_height = 0;

    insta::assert_snapshot!(
        render_to_styled_string(&mut state, 32, 12),
        @""
    );
}
```

These empty inline snapshots are intentional only for the red step; Step 4
replaces them with the reviewed generated styled output.

The reviewed snapshots must show `bg:58` or `bg:22` on the left marker, icon,
title, content, padding, and every prompt/wait-reason row. The action snapshots
must contain `!`; the completed snapshot must contain `✓`.

Run:

```bash
cargo test attention_background -- --nocapture
cargo test snapshot_attention -- --nocapture
```

Expected: FAIL because the new theme fields and full-card rendering are absent.

- [ ] **Step 2: Implement theme options**

Add and re-export:

```rust
pub const SIDEBAR_COLOR_ATTENTION_ACTION_BG: &str =
    "@sidebar_color_attention_action_bg";
pub const SIDEBAR_COLOR_ATTENTION_COMPLETED_BG: &str =
    "@sidebar_color_attention_completed_bg";
```

Add `ColorTheme` fields with defaults 58 and 22, load both through
`ColorTheme::from_options`, and add:

```rust
pub fn attention_bg(&self, kind: PaneAttentionKind) -> Color {
    match kind {
        PaneAttentionKind::ActionRequired => self.attention_action_bg,
        PaneAttentionKind::Completed => self.attention_completed_bg,
    }
}

pub fn attention_fg(&self, kind: PaneAttentionKind) -> Color {
    match kind {
        PaneAttentionKind::ActionRequired => self.status_waiting,
        PaneAttentionKind::Completed => self.status_idle,
    }
}
```

Keep `status_color` responsible only for ordinary `PaneStatus`; update its
signature, callers, and existing tests accordingly:

```rust
pub fn status_color(&self, status: &PaneStatus) -> Color {
    match status {
        PaneStatus::Running | PaneStatus::Background => self.status_running,
        PaneStatus::Waiting => self.status_waiting,
        PaneStatus::Idle => self.status_idle,
        PaneStatus::Error => self.status_error,
        PaneStatus::Unknown => self.status_unknown,
    }
}
```

- [ ] **Step 3: Implement attention icon and background precedence**

In `src/ui/panes/row.rs`, derive:

```rust
let attention_bg = pane
    .attention
    .as_ref()
    .map(|attention| theme.attention_bg(attention.kind));
let marker_bg = attention_bg.or_else(|| selected.then_some(theme.selection_bg));
let body_bg = attention_bg;
```

Use `marker_bg` for the status/branch contexts and `body_bg` for body contexts.
This makes attention cover the whole card while preserving the existing
ordinary selection background behavior when attention is absent.

In `status_row`, use `attention.kind.icon()` and `theme.attention_fg` when
attention exists; otherwise retain `running_icon_for` and ordinary status
colors. Keep the icon width at one display cell.

Run:

```bash
cargo test attention_background -- --nocapture
cargo test snapshot_attention -- --nocapture
```

Expected: snapshot failures containing only the newly intended styles.

- [ ] **Step 4: Accept only reviewed inline snapshots and run UI regressions**

Review:

```bash
cargo insta pending-snapshots
```

Confirm that only the new attention snapshots and directly affected existing
styled snapshots are pending. Then run:

```bash
cargo insta accept
cargo test --test styled_tests
cargo test --test color_tests
cargo test --test ui_snapshot
```

Expected: PASS. Ordinary selection snapshots retain their existing body-row
styling, card widths are unchanged, and focus-enclosure snapshots remain
structurally unchanged.

- [ ] **Step 5: Format, verify, and commit Task 3**

```bash
cargo fmt
cargo test --test styled_tests
cargo test --test color_tests
cargo test --test ui_snapshot
git diff --check
git add src/tmux/options.rs src/tmux.rs src/ui/colors.rs src/ui/panes/row.rs src/ui/panes/row/status.rs tests/test_helpers.rs tests/color_tests.rs tests/styled_tests.rs tests/ui_snapshot.rs
git commit -m "feat: highlight unseen agent cards"
```

---

### Task 4: Documentation, Full Verification, and Final Review

**Files:**
- Modify: `docs/state-management.md`
- Modify: `docs/superpowers/specs/2026-07-23-unseen-agent-attention-highlighting-design.md`
- Add: `docs/superpowers/plans/2026-07-23-unseen-agent-attention-highlighting.md`

**Interfaces:**
- Documents: typed `@pane_attention` values, event transitions, focus
  acknowledgement, theme overrides, and update frequency.

- [ ] **Step 1: Update state-management documentation**

Change the pane-option table entry to describe:

```text
@pane_attention
  action_required:<event-id> or completed:<event-id>
  Written by attention/completion hooks; conditionally cleared when the pane
  is actually observed as focused.
```

Update `PaneInfo` in the key-types section from `attention: bool` to
`attention: Option<PaneAttention>`. Add both theme option names and explain
that focus acknowledgement uses newly observed focus, not sticky
`focused_pane_id`.

- [ ] **Step 2: Run focused and full verification**

```bash
cargo fmt
cargo fmt --check
cargo test
cargo clippy
cargo build --release
git diff --check
```

Expected: every command exits 0.

- [ ] **Step 3: Review the final diff against acceptance criteria**

Run:

```bash
git status --short
git diff --stat 20ad508
git diff 20ad508 -- src docs/state-management.md tests
```

Confirm:

- permission/action cards are full dark amber with `!`,
- completed idle cards are full dark green with `✓`,
- actual focus clears only the observed event,
- cursor selection does not clear or hide attention,
- background stops never show completed attention,
- a changed event survives compare-and-clear,
- ordinary cards, ordering, dimensions, and notifications are unchanged, and
- `.idea/` is still untracked and unstaged.

- [ ] **Step 4: Commit final documentation**

```bash
cargo fmt
git add docs/state-management.md docs/superpowers/specs/2026-07-23-unseen-agent-attention-highlighting-design.md docs/superpowers/plans/2026-07-23-unseen-agent-attention-highlighting.md
git commit -m "docs: describe unseen agent attention"
```

- [ ] **Step 5: Verify final repository state**

```bash
git status --short --branch
git log -5 --oneline --decorate
```

Expected: `personal-main` is ahead of `origin/personal-main` by local commits;
only the pre-existing `.idea/` directory remains untracked. Do not push.
