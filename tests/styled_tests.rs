#[allow(dead_code, unused_imports)]
mod test_helpers;

use test_helpers::*;
use tmux_agent_sidebar::activity::ActivityEntry;
use tmux_agent_sidebar::state::{BottomPanel, Focus};
use tmux_agent_sidebar::tmux::{AgentType, PaneAttention, PaneStatus, SessionInfo, WindowInfo};

// ─── Styled Snapshot Tests for Selection and Focus ─────────────────

#[test]
fn snapshot_selected_focused_styled() {
    let pane = make_pane(AgentType::Claude, PaneStatus::Idle);
    let mut state = make_state(vec![SessionInfo {
        session_name: "main".into(),
        windows: vec![WindowInfo {
            window_id: "@1".into(),
            window_name: "project".into(),
            window_active: true,
            auto_rename: false,
            panes: vec![pane.clone()],
        }],
    }]);
    state.repo_groups = vec![make_repo_group("project", vec![pane])];
    state.rebuild_row_targets();
    state.focus_state.sidebar_focused = true;
    state.global.selected_pane_row = 0;
    state.bottom_panel_height = 0;

    // Styled snapshot locks in the selected row's ┃[fg:153,bg:239] marker
    // and the selection background spanning its content cells.
    insta::assert_snapshot!(render_to_styled_string(&mut state, 28, 10), @r"
     ≡[fg:111]1[fg:255]  ●[fg:245]0[fg:245]  ◎[fg:245]0[fg:245]  ◐[fg:245]0[fg:245]  ○[fg:245]1[fg:255]  ✕[fg:245]0[fg:245]
    ⓘ[fg:221]                        —[fg:252] ▾[fg:252]
    p[fg:153]r[fg:153]o[fg:153]j[fg:153]e[fg:153]c[fg:153]t[fg:153]
    ┃[fg:153,bg:239] [bg:239]○[fg:110,bg:239] [fg:174,bg:239]c[fg:174,bg:239]l[fg:174,bg:239]a[fg:174,bg:239]u[fg:174,bg:239]d[fg:174,bg:239]e[fg:174,bg:239] [bg:239] [bg:239] [bg:239] [bg:239] [bg:239] [bg:239] [bg:239] [bg:239] [bg:239] [bg:239] [bg:239] [bg:239] [bg:239] [bg:239] [bg:239] [bg:239] [bg:239] [bg:239]
       [fg:255] [fg:255]W[fg:255]a[fg:255]i[fg:255]t[fg:255]i[fg:255]n[fg:255]g[fg:255] [fg:255]f[fg:255]o[fg:255]r[fg:255] [fg:255]p[fg:255]r[fg:255]o[fg:255]m[fg:255]p[fg:255]t[fg:255]…[fg:255]
    ");
}

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

    insta::assert_snapshot!(render_to_styled_string(&mut state, 32, 12), @"
     ≡[fg:111]1[fg:255]  ●[fg:245]0[fg:245]  ◎[fg:245]0[fg:245]  ◐[fg:245]1[fg:255]  ○[fg:245]0[fg:245]  ✕[fg:245]0[fg:245]
    ⓘ[fg:221]                            —[fg:252] ▾[fg:252]
    p[fg:255]r[fg:255]o[fg:255]j[fg:255]e[fg:255]c[fg:255]t[fg:255]
    ┃[fg:153,bg:58] [bg:58]![fg:221,bg:58] [fg:174,bg:58]c[fg:174,bg:58]l[fg:174,bg:58]a[fg:174,bg:58]u[fg:174,bg:58]d[fg:174,bg:58]e[fg:174,bg:58] [bg:58] [bg:58] [bg:58] [bg:58] [bg:58] [bg:58] [bg:58] [bg:58] [bg:58] [bg:58] [bg:58] [bg:58] [bg:58] [bg:58] [bg:58] [bg:58] [bg:58] [bg:58] [bg:58] [bg:58] [bg:58] [bg:58]
     [bg:58] [bg:58] [fg:221,bg:58] [fg:221,bg:58]p[fg:221,bg:58]e[fg:221,bg:58]r[fg:221,bg:58]m[fg:221,bg:58]i[fg:221,bg:58]s[fg:221,bg:58]s[fg:221,bg:58]i[fg:221,bg:58]o[fg:221,bg:58]n[fg:221,bg:58] [fg:221,bg:58]d[fg:221,bg:58]e[fg:221,bg:58]n[fg:221,bg:58]i[fg:221,bg:58]e[fg:221,bg:58]d[fg:221,bg:58] [bg:58] [bg:58] [bg:58] [bg:58] [bg:58] [bg:58] [bg:58] [bg:58] [bg:58] [bg:58] [bg:58]
     [bg:58] [bg:58] [fg:244,bg:58] [fg:244,bg:58]A[fg:244,bg:58]l[fg:244,bg:58]l[fg:244,bg:58]o[fg:244,bg:58]w[fg:244,bg:58] [fg:244,bg:58]c[fg:244,bg:58]a[fg:244,bg:58]r[fg:244,bg:58]g[fg:244,bg:58]o[fg:244,bg:58] [fg:244,bg:58]t[fg:244,bg:58]e[fg:244,bg:58]s[fg:244,bg:58]t[fg:244,bg:58]?[fg:244,bg:58] [bg:58] [bg:58] [bg:58] [bg:58] [bg:58] [bg:58] [bg:58] [bg:58] [bg:58] [bg:58] [bg:58]
    ");
}

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

    insta::assert_snapshot!(render_to_styled_string(&mut state, 32, 12), @"
     ≡[fg:111]1[fg:255]  ●[fg:245]0[fg:245]  ◎[fg:245]0[fg:245]  ◐[fg:245]0[fg:245]  ○[fg:245]1[fg:255]  ✕[fg:245]0[fg:245]
    ⓘ[fg:221]                            —[fg:252] ▾[fg:252]
    p[fg:255]r[fg:255]o[fg:255]j[fg:255]e[fg:255]c[fg:255]t[fg:255]
     [bg:22] [bg:22]✓[fg:110,bg:22] [fg:141,bg:22]c[fg:141,bg:22]o[fg:141,bg:22]d[fg:141,bg:22]e[fg:141,bg:22]x[fg:141,bg:22] [bg:22] [bg:22] [bg:22] [bg:22] [bg:22] [bg:22] [bg:22] [bg:22] [bg:22] [bg:22] [bg:22] [bg:22] [bg:22] [bg:22] [bg:22] [bg:22] [bg:22] [bg:22] [bg:22] [bg:22] [bg:22] [bg:22] [bg:22]
     [bg:22] [bg:22]▷[fg:81,bg:22,bold] [fg:81,bg:22,bold]A[fg:244,bg:22]l[fg:244,bg:22]l[fg:244,bg:22] [fg:244,bg:22]t[fg:244,bg:22]e[fg:244,bg:22]s[fg:244,bg:22]t[fg:244,bg:22]s[fg:244,bg:22] [fg:244,bg:22]p[fg:244,bg:22]a[fg:244,bg:22]s[fg:244,bg:22]s[fg:244,bg:22].[fg:244,bg:22] [bg:22] [bg:22] [bg:22] [bg:22] [bg:22] [bg:22] [bg:22] [bg:22] [bg:22] [bg:22] [bg:22] [bg:22] [bg:22]
    ");
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

    insta::assert_snapshot!(render_to_styled_string(&mut state, 32, 12), @"
     ≡[fg:111]1[fg:255]  ●[fg:245]0[fg:245]  ◎[fg:245]0[fg:245]  ◐[fg:245]1[fg:255]  ○[fg:245]0[fg:245]  ✕[fg:245]0[fg:245]
    ⓘ[fg:221]                            —[fg:252] ▾[fg:252]
    p[fg:255]r[fg:255]o[fg:255]j[fg:255]e[fg:255]c[fg:255]t[fg:255]
     [bg:58] [bg:58]![fg:221,bg:58] [fg:117,bg:58]o[fg:117,bg:58]p[fg:117,bg:58]e[fg:117,bg:58]n[fg:117,bg:58]c[fg:117,bg:58]o[fg:117,bg:58]d[fg:117,bg:58]e[fg:117,bg:58] [bg:58] [bg:58] [bg:58] [bg:58] [bg:58] [bg:58] [bg:58] [bg:58] [bg:58] [bg:58] [bg:58] [bg:58] [bg:58] [bg:58] [bg:58] [bg:58] [bg:58] [bg:58] [bg:58] [bg:58]
     [bg:58] [bg:58] [fg:221,bg:58] [fg:221,bg:58]p[fg:221,bg:58]e[fg:221,bg:58]r[fg:221,bg:58]m[fg:221,bg:58]i[fg:221,bg:58]s[fg:221,bg:58]s[fg:221,bg:58]i[fg:221,bg:58]o[fg:221,bg:58]n[fg:221,bg:58] [bg:58] [bg:58] [bg:58] [bg:58] [bg:58] [bg:58] [bg:58] [bg:58] [bg:58] [bg:58] [bg:58] [bg:58] [bg:58] [bg:58] [bg:58] [bg:58] [bg:58] [bg:58]
    ");
}

#[test]
fn snapshot_activity_focused_styled() {
    let pane = make_pane(AgentType::Claude, PaneStatus::Running);
    let mut state = make_state(vec![SessionInfo {
        session_name: "main".into(),
        windows: vec![WindowInfo {
            window_id: "@1".into(),
            window_name: "project".into(),
            window_active: true,
            auto_rename: false,
            panes: vec![pane.clone()],
        }],
    }]);
    state.repo_groups = vec![make_repo_group("project", vec![pane])];
    state.rebuild_row_targets();
    state.focus_state.focus = Focus::BottomPanel;
    state.focus_state.sidebar_focused = true;
    state.activity.entries = vec![ActivityEntry {
        timestamp: "10:32".into(),
        tool: "Edit".into(),
        label: "src/main.rs".into(),
    }];

    // Styled snapshot locks in the focused group header accent (fg:153) and
    // the active-panel border color.
    insta::assert_snapshot!(render_to_styled_string(&mut state, 28, 14), @"
     ≡[fg:111]1[fg:255]  ●[fg:245]1[fg:255]  ◎[fg:245]0[fg:245]  ◐[fg:245]0[fg:245]  ○[fg:245]0[fg:245]  ✕[fg:245]0[fg:245]

    ╭[fg:240] [fg:240]G[fg:252]i[fg:252]t[fg:252] [fg:240]─[fg:240]─[fg:240]─[fg:240]─[fg:240]─[fg:240]─[fg:240]─[fg:240]─[fg:240]─[fg:240]─[fg:240]─[fg:240]─[fg:240]─[fg:240]─[fg:240]─[fg:240]─[fg:240]─[fg:240]─[fg:240]─[fg:240]─[fg:240]─[fg:240]╮[fg:240]
    │[fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240]│[fg:240]
    │[fg:240] [fg:240] [fg:240] [fg:240] [fg:240]W[fg:252]o[fg:252]r[fg:252]k[fg:252]i[fg:252]n[fg:252]g[fg:252] [fg:252]t[fg:252]r[fg:252]e[fg:252]e[fg:252] [fg:252]c[fg:252]l[fg:252]e[fg:252]a[fg:252]n[fg:252] [fg:240] [fg:240] [fg:240] [fg:240]│[fg:240]
    │[fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240]│[fg:240]
    │[fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240]│[fg:240]
    ╰[fg:240]─[fg:240]─[fg:240]─[fg:240]─[fg:240]─[fg:240]─[fg:240]─[fg:240]─[fg:240]─[fg:240]─[fg:240]─[fg:240]─[fg:240]─[fg:240]─[fg:240]─[fg:240]─[fg:240]─[fg:240]─[fg:240]─[fg:240]─[fg:240]─[fg:240]─[fg:240]─[fg:240]─[fg:240]─[fg:240]─[fg:240]╯[fg:240]
    ╭[fg:153] [fg:153]A[fg:153]c[fg:153]t[fg:153]i[fg:153]v[fg:153]i[fg:153]t[fg:153]y[fg:153] [fg:153]─[fg:153]─[fg:153]─[fg:153]─[fg:153]─[fg:153]─[fg:153]─[fg:153]─[fg:153]─[fg:153]─[fg:153]─[fg:153]─[fg:153]─[fg:153]─[fg:153]─[fg:153]─[fg:153]╮[fg:153]
    │[fg:153] [fg:153] [fg:153] [fg:153] [fg:153] [fg:153] [fg:153] [fg:153] [fg:153] [fg:153] [fg:153] [fg:153] [fg:153] [fg:153] [fg:153] [fg:153] [fg:153] [fg:153] [fg:153] [fg:153] [fg:153] [fg:153] [fg:153] [fg:153] [fg:153] [fg:153] [fg:153]│[fg:153]
    │[fg:153]1[fg:109]0[fg:109]:[fg:109]3[fg:109]2[fg:109] [fg:153] [fg:153] [fg:153] [fg:153] [fg:153] [fg:153] [fg:153] [fg:153] [fg:153] [fg:153] [fg:153] [fg:153] [fg:153] [fg:153] [fg:153] [fg:153] [fg:153]E[fg:180]d[fg:180]i[fg:180]t[fg:180]│[fg:153]
    │[fg:153] [fg:252] [fg:252]s[fg:252]r[fg:252]c[fg:252]/[fg:252]m[fg:252]a[fg:252]i[fg:252]n[fg:252].[fg:252]r[fg:252]s[fg:252] [fg:153] [fg:153] [fg:153] [fg:153] [fg:153] [fg:153] [fg:153] [fg:153] [fg:153] [fg:153] [fg:153] [fg:153] [fg:153]│[fg:153]
    │[fg:153] [fg:153] [fg:153] [fg:153] [fg:153] [fg:153] [fg:153] [fg:153] [fg:153] [fg:153] [fg:153] [fg:153] [fg:153] [fg:153] [fg:153] [fg:153] [fg:153] [fg:153] [fg:153] [fg:153] [fg:153] [fg:153] [fg:153] [fg:153] [fg:153] [fg:153] [fg:153]│[fg:153]
    ╰[fg:153]─[fg:153]─[fg:153]─[fg:153]─[fg:153]─[fg:153]─[fg:153]─[fg:153]─[fg:153]─[fg:153]─[fg:153]─[fg:153]─[fg:153]─[fg:153]─[fg:153]─[fg:153]─[fg:153]─[fg:153]─[fg:153]─[fg:153]─[fg:153]─[fg:153]─[fg:153]─[fg:153]─[fg:153]─[fg:153]─[fg:153]╯[fg:153]
    ");
}

#[test]
fn snapshot_activity_unfocused_styled() {
    let pane = make_pane(AgentType::Claude, PaneStatus::Running);
    let mut state = make_state(vec![SessionInfo {
        session_name: "main".into(),
        windows: vec![WindowInfo {
            window_id: "@1".into(),
            window_name: "project".into(),
            window_active: true,
            auto_rename: false,
            panes: vec![pane.clone()],
        }],
    }]);
    state.repo_groups = vec![make_repo_group("project", vec![pane])];
    state.rebuild_row_targets();
    state.focus_state.focus = Focus::Panes; // not activity
    state.focus_state.sidebar_focused = true;
    state.activity.entries = vec![ActivityEntry {
        timestamp: "10:32".into(),
        tool: "Edit".into(),
        label: "src/main.rs".into(),
    }];

    // Styled snapshot locks in the unfocused bottom-panel border
    // (border_inactive fg:240).
    insta::assert_snapshot!(render_to_styled_string(&mut state, 28, 14), @"
     ≡[fg:111]1[fg:255]  ●[fg:245]1[fg:255]  ◎[fg:245]0[fg:245]  ◐[fg:245]0[fg:245]  ○[fg:245]0[fg:245]  ✕[fg:245]0[fg:245]

    ╭[fg:240] [fg:240]G[fg:252]i[fg:252]t[fg:252] [fg:240]─[fg:240]─[fg:240]─[fg:240]─[fg:240]─[fg:240]─[fg:240]─[fg:240]─[fg:240]─[fg:240]─[fg:240]─[fg:240]─[fg:240]─[fg:240]─[fg:240]─[fg:240]─[fg:240]─[fg:240]─[fg:240]─[fg:240]─[fg:240]─[fg:240]╮[fg:240]
    │[fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240]│[fg:240]
    │[fg:240] [fg:240] [fg:240] [fg:240] [fg:240]W[fg:252]o[fg:252]r[fg:252]k[fg:252]i[fg:252]n[fg:252]g[fg:252] [fg:252]t[fg:252]r[fg:252]e[fg:252]e[fg:252] [fg:252]c[fg:252]l[fg:252]e[fg:252]a[fg:252]n[fg:252] [fg:240] [fg:240] [fg:240] [fg:240]│[fg:240]
    │[fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240]│[fg:240]
    │[fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240]│[fg:240]
    ╰[fg:240]─[fg:240]─[fg:240]─[fg:240]─[fg:240]─[fg:240]─[fg:240]─[fg:240]─[fg:240]─[fg:240]─[fg:240]─[fg:240]─[fg:240]─[fg:240]─[fg:240]─[fg:240]─[fg:240]─[fg:240]─[fg:240]─[fg:240]─[fg:240]─[fg:240]─[fg:240]─[fg:240]─[fg:240]─[fg:240]─[fg:240]╯[fg:240]
    ╭[fg:240] [fg:240]A[fg:153]c[fg:153]t[fg:153]i[fg:153]v[fg:153]i[fg:153]t[fg:153]y[fg:153] [fg:240]─[fg:240]─[fg:240]─[fg:240]─[fg:240]─[fg:240]─[fg:240]─[fg:240]─[fg:240]─[fg:240]─[fg:240]─[fg:240]─[fg:240]─[fg:240]─[fg:240]─[fg:240]─[fg:240]╮[fg:240]
    │[fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240]│[fg:240]
    │[fg:240]1[fg:109]0[fg:109]:[fg:109]3[fg:109]2[fg:109] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240]E[fg:180]d[fg:180]i[fg:180]t[fg:180]│[fg:240]
    │[fg:240] [fg:252] [fg:252]s[fg:252]r[fg:252]c[fg:252]/[fg:252]m[fg:252]a[fg:252]i[fg:252]n[fg:252].[fg:252]r[fg:252]s[fg:252] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240]│[fg:240]
    │[fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240]│[fg:240]
    ╰[fg:240]─[fg:240]─[fg:240]─[fg:240]─[fg:240]─[fg:240]─[fg:240]─[fg:240]─[fg:240]─[fg:240]─[fg:240]─[fg:240]─[fg:240]─[fg:240]─[fg:240]─[fg:240]─[fg:240]─[fg:240]─[fg:240]─[fg:240]─[fg:240]─[fg:240]─[fg:240]─[fg:240]─[fg:240]─[fg:240]─[fg:240]╯[fg:240]
    ");
}

#[test]
fn active_bottom_panel_activity_uses_accent_when_selected() {
    let pane = make_pane(AgentType::Claude, PaneStatus::Running);
    let mut state = make_state(vec![SessionInfo {
        session_name: "main".into(),
        windows: vec![WindowInfo {
            window_id: "@1".into(),
            window_name: "project".into(),
            window_active: true,
            auto_rename: false,
            panes: vec![pane.clone()],
        }],
    }]);
    state.repo_groups = vec![make_repo_group("project", vec![pane])];
    state.rebuild_row_targets();
    state.focus_state.focus = Focus::BottomPanel;
    state.focus_state.sidebar_focused = true;
    state.active_bottom_panel = BottomPanel::Activity;

    // Styled snapshot locks in `A` using accent (fg:153) and `G` remaining
    // muted (fg:252) on the inactive bottom-panel title row.
    insta::assert_snapshot!(render_to_styled_string(&mut state, 28, 14), @"
     ≡[fg:111]1[fg:255]  ●[fg:245]1[fg:255]  ◎[fg:245]0[fg:245]  ◐[fg:245]0[fg:245]  ○[fg:245]0[fg:245]  ✕[fg:245]0[fg:245]

    ╭[fg:240] [fg:240]G[fg:252]i[fg:252]t[fg:252] [fg:240]─[fg:240]─[fg:240]─[fg:240]─[fg:240]─[fg:240]─[fg:240]─[fg:240]─[fg:240]─[fg:240]─[fg:240]─[fg:240]─[fg:240]─[fg:240]─[fg:240]─[fg:240]─[fg:240]─[fg:240]─[fg:240]─[fg:240]─[fg:240]─[fg:240]╮[fg:240]
    │[fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240]│[fg:240]
    │[fg:240] [fg:240] [fg:240] [fg:240] [fg:240]W[fg:252]o[fg:252]r[fg:252]k[fg:252]i[fg:252]n[fg:252]g[fg:252] [fg:252]t[fg:252]r[fg:252]e[fg:252]e[fg:252] [fg:252]c[fg:252]l[fg:252]e[fg:252]a[fg:252]n[fg:252] [fg:240] [fg:240] [fg:240] [fg:240]│[fg:240]
    │[fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240]│[fg:240]
    │[fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240]│[fg:240]
    ╰[fg:240]─[fg:240]─[fg:240]─[fg:240]─[fg:240]─[fg:240]─[fg:240]─[fg:240]─[fg:240]─[fg:240]─[fg:240]─[fg:240]─[fg:240]─[fg:240]─[fg:240]─[fg:240]─[fg:240]─[fg:240]─[fg:240]─[fg:240]─[fg:240]─[fg:240]─[fg:240]─[fg:240]─[fg:240]─[fg:240]─[fg:240]╯[fg:240]
    ╭[fg:153] [fg:153]A[fg:153]c[fg:153]t[fg:153]i[fg:153]v[fg:153]i[fg:153]t[fg:153]y[fg:153] [fg:153]─[fg:153]─[fg:153]─[fg:153]─[fg:153]─[fg:153]─[fg:153]─[fg:153]─[fg:153]─[fg:153]─[fg:153]─[fg:153]─[fg:153]─[fg:153]─[fg:153]─[fg:153]─[fg:153]╮[fg:153]
    │[fg:153] [fg:153] [fg:153] [fg:153] [fg:153] [fg:153] [fg:153] [fg:153] [fg:153] [fg:153] [fg:153] [fg:153] [fg:153] [fg:153] [fg:153] [fg:153] [fg:153] [fg:153] [fg:153] [fg:153] [fg:153] [fg:153] [fg:153] [fg:153] [fg:153] [fg:153] [fg:153]│[fg:153]
    │[fg:153] [fg:153] [fg:153] [fg:153] [fg:153] [fg:153] [fg:153]N[fg:252]o[fg:252] [fg:252]a[fg:252]c[fg:252]t[fg:252]i[fg:252]v[fg:252]i[fg:252]t[fg:252]y[fg:252] [fg:252]y[fg:252]e[fg:252]t[fg:252] [fg:153] [fg:153] [fg:153] [fg:153] [fg:153]│[fg:153]
    │[fg:153] [fg:153] [fg:153] [fg:153] [fg:153] [fg:153] [fg:153] [fg:153] [fg:153] [fg:153] [fg:153] [fg:153] [fg:153] [fg:153] [fg:153] [fg:153] [fg:153] [fg:153] [fg:153] [fg:153] [fg:153] [fg:153] [fg:153] [fg:153] [fg:153] [fg:153] [fg:153]│[fg:153]
    │[fg:153] [fg:153] [fg:153] [fg:153] [fg:153] [fg:153] [fg:153] [fg:153] [fg:153] [fg:153] [fg:153] [fg:153] [fg:153] [fg:153] [fg:153] [fg:153] [fg:153] [fg:153] [fg:153] [fg:153] [fg:153] [fg:153] [fg:153] [fg:153] [fg:153] [fg:153] [fg:153]│[fg:153]
    ╰[fg:153]─[fg:153]─[fg:153]─[fg:153]─[fg:153]─[fg:153]─[fg:153]─[fg:153]─[fg:153]─[fg:153]─[fg:153]─[fg:153]─[fg:153]─[fg:153]─[fg:153]─[fg:153]─[fg:153]─[fg:153]─[fg:153]─[fg:153]─[fg:153]─[fg:153]─[fg:153]─[fg:153]─[fg:153]─[fg:153]─[fg:153]╯[fg:153]
    ");
}

#[test]
fn active_bottom_panel_git_uses_accent_when_selected() {
    let pane = make_pane(AgentType::Claude, PaneStatus::Running);
    let mut state = make_state(vec![SessionInfo {
        session_name: "main".into(),
        windows: vec![WindowInfo {
            window_id: "@1".into(),
            window_name: "project".into(),
            window_active: true,
            auto_rename: false,
            panes: vec![pane.clone()],
        }],
    }]);
    state.repo_groups = vec![make_repo_group("project", vec![pane])];
    state.rebuild_row_targets();
    state.focus_state.focus = Focus::BottomPanel;
    state.focus_state.sidebar_focused = true;
    state.active_bottom_panel = BottomPanel::Git;

    // Styled snapshot locks in `G` using accent (fg:153) and `A` remaining
    // muted (fg:252) on the inactive bottom-panel title row.
    insta::assert_snapshot!(render_to_styled_string(&mut state, 28, 14), @"
     ≡[fg:111]1[fg:255]  ●[fg:245]1[fg:255]  ◎[fg:245]0[fg:245]  ◐[fg:245]0[fg:245]  ○[fg:245]0[fg:245]  ✕[fg:245]0[fg:245]

    ╭[fg:153] [fg:153]G[fg:153]i[fg:153]t[fg:153] [fg:153]─[fg:153]─[fg:153]─[fg:153]─[fg:153]─[fg:153]─[fg:153]─[fg:153]─[fg:153]─[fg:153]─[fg:153]─[fg:153]─[fg:153]─[fg:153]─[fg:153]─[fg:153]─[fg:153]─[fg:153]─[fg:153]─[fg:153]─[fg:153]─[fg:153]╮[fg:153]
    │[fg:153] [fg:153] [fg:153] [fg:153] [fg:153] [fg:153] [fg:153] [fg:153] [fg:153] [fg:153] [fg:153] [fg:153] [fg:153] [fg:153] [fg:153] [fg:153] [fg:153] [fg:153] [fg:153] [fg:153] [fg:153] [fg:153] [fg:153] [fg:153] [fg:153] [fg:153] [fg:153]│[fg:153]
    │[fg:153] [fg:153] [fg:153] [fg:153] [fg:153]W[fg:252]o[fg:252]r[fg:252]k[fg:252]i[fg:252]n[fg:252]g[fg:252] [fg:252]t[fg:252]r[fg:252]e[fg:252]e[fg:252] [fg:252]c[fg:252]l[fg:252]e[fg:252]a[fg:252]n[fg:252] [fg:153] [fg:153] [fg:153] [fg:153]│[fg:153]
    │[fg:153] [fg:153] [fg:153] [fg:153] [fg:153] [fg:153] [fg:153] [fg:153] [fg:153] [fg:153] [fg:153] [fg:153] [fg:153] [fg:153] [fg:153] [fg:153] [fg:153] [fg:153] [fg:153] [fg:153] [fg:153] [fg:153] [fg:153] [fg:153] [fg:153] [fg:153] [fg:153]│[fg:153]
    │[fg:153] [fg:153] [fg:153] [fg:153] [fg:153] [fg:153] [fg:153] [fg:153] [fg:153] [fg:153] [fg:153] [fg:153] [fg:153] [fg:153] [fg:153] [fg:153] [fg:153] [fg:153] [fg:153] [fg:153] [fg:153] [fg:153] [fg:153] [fg:153] [fg:153] [fg:153] [fg:153]│[fg:153]
    ╰[fg:153]─[fg:153]─[fg:153]─[fg:153]─[fg:153]─[fg:153]─[fg:153]─[fg:153]─[fg:153]─[fg:153]─[fg:153]─[fg:153]─[fg:153]─[fg:153]─[fg:153]─[fg:153]─[fg:153]─[fg:153]─[fg:153]─[fg:153]─[fg:153]─[fg:153]─[fg:153]─[fg:153]─[fg:153]─[fg:153]─[fg:153]╯[fg:153]
    ╭[fg:240] [fg:240]A[fg:252]c[fg:252]t[fg:252]i[fg:252]v[fg:252]i[fg:252]t[fg:252]y[fg:252] [fg:240]─[fg:240]─[fg:240]─[fg:240]─[fg:240]─[fg:240]─[fg:240]─[fg:240]─[fg:240]─[fg:240]─[fg:240]─[fg:240]─[fg:240]─[fg:240]─[fg:240]─[fg:240]─[fg:240]╮[fg:240]
    │[fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240]│[fg:240]
    │[fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240]N[fg:252]o[fg:252] [fg:252]a[fg:252]c[fg:252]t[fg:252]i[fg:252]v[fg:252]i[fg:252]t[fg:252]y[fg:252] [fg:252]y[fg:252]e[fg:252]t[fg:252] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240]│[fg:240]
    │[fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240]│[fg:240]
    │[fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240]│[fg:240]
    ╰[fg:240]─[fg:240]─[fg:240]─[fg:240]─[fg:240]─[fg:240]─[fg:240]─[fg:240]─[fg:240]─[fg:240]─[fg:240]─[fg:240]─[fg:240]─[fg:240]─[fg:240]─[fg:240]─[fg:240]─[fg:240]─[fg:240]─[fg:240]─[fg:240]─[fg:240]─[fg:240]─[fg:240]─[fg:240]─[fg:240]─[fg:240]╯[fg:240]
    ");
}

// ─── Selection Background Border Tests ───────────────────────────────

#[test]
fn selection_marker_uses_accent_color_with_selection_bg() {
    let pane = make_pane(AgentType::Claude, PaneStatus::Running);
    let mut state = make_state(vec![SessionInfo {
        session_name: "main".into(),
        windows: vec![WindowInfo {
            window_id: "@1".into(),
            window_name: "project".into(),
            window_active: true,
            auto_rename: false,
            panes: vec![pane.clone()],
        }],
    }]);
    state.repo_groups = vec![make_repo_group("project", vec![pane])];
    state.rebuild_row_targets();
    state.focus_state.sidebar_focused = true;
    state.focus_state.focus = Focus::Panes;
    state.global.selected_pane_row = 0;

    // Styled snapshot locks in:
    //   1. the selected row begins with `┃[fg:153,bg:239]` (accent + selection bg)
    //   2. the selected row never contains the old frame `│`
    insta::assert_snapshot!(render_to_styled_string(&mut state, 28, 24), @"
     ≡[fg:111]1[fg:255]  ●[fg:245]1[fg:255]  ◎[fg:245]0[fg:245]  ◐[fg:245]0[fg:245]  ○[fg:245]0[fg:245]  ✕[fg:245]0[fg:245]
    ⓘ[fg:221]                        —[fg:252] ▾[fg:252]
    ┃[fg:153,bg:239] [bg:239]●[fg:82,bg:239] [fg:174,bg:239]c[fg:174,bg:239]l[fg:174,bg:239]a[fg:174,bg:239]u[fg:174,bg:239]d[fg:174,bg:239]e[fg:174,bg:239] [bg:239] [bg:239] [bg:239] [bg:239] [bg:239] [bg:239] [bg:239] [bg:239] [bg:239] [bg:239] [bg:239] [bg:239] [bg:239] [bg:239] [bg:239] [bg:239] [bg:239] [bg:239]

    ╭[fg:240] [fg:240]G[fg:252]i[fg:252]t[fg:252] [fg:240]─[fg:240]─[fg:240]─[fg:240]─[fg:240]─[fg:240]─[fg:240]─[fg:240]─[fg:240]─[fg:240]─[fg:240]─[fg:240]─[fg:240]─[fg:240]─[fg:240]─[fg:240]─[fg:240]─[fg:240]─[fg:240]─[fg:240]─[fg:240]─[fg:240]╮[fg:240]
    │[fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240]│[fg:240]
    │[fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240]│[fg:240]
    │[fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240]│[fg:240]
    │[fg:240] [fg:240] [fg:240] [fg:240] [fg:240]W[fg:252]o[fg:252]r[fg:252]k[fg:252]i[fg:252]n[fg:252]g[fg:252] [fg:252]t[fg:252]r[fg:252]e[fg:252]e[fg:252] [fg:252]c[fg:252]l[fg:252]e[fg:252]a[fg:252]n[fg:252] [fg:240] [fg:240] [fg:240] [fg:240]│[fg:240]
    │[fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240]│[fg:240]
    │[fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240]│[fg:240]
    │[fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240]│[fg:240]
    │[fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240]│[fg:240]
    ╰[fg:240]─[fg:240]─[fg:240]─[fg:240]─[fg:240]─[fg:240]─[fg:240]─[fg:240]─[fg:240]─[fg:240]─[fg:240]─[fg:240]─[fg:240]─[fg:240]─[fg:240]─[fg:240]─[fg:240]─[fg:240]─[fg:240]─[fg:240]─[fg:240]─[fg:240]─[fg:240]─[fg:240]─[fg:240]─[fg:240]─[fg:240]╯[fg:240]
    ╭[fg:240] [fg:240]A[fg:153]c[fg:153]t[fg:153]i[fg:153]v[fg:153]i[fg:153]t[fg:153]y[fg:153] [fg:240]─[fg:240]─[fg:240]─[fg:240]─[fg:240]─[fg:240]─[fg:240]─[fg:240]─[fg:240]─[fg:240]─[fg:240]─[fg:240]─[fg:240]─[fg:240]─[fg:240]─[fg:240]─[fg:240]╮[fg:240]
    │[fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240]│[fg:240]
    │[fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240]│[fg:240]
    │[fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240]│[fg:240]
    │[fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240]N[fg:252]o[fg:252] [fg:252]a[fg:252]c[fg:252]t[fg:252]i[fg:252]v[fg:252]i[fg:252]t[fg:252]y[fg:252] [fg:252]y[fg:252]e[fg:252]t[fg:252] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240]│[fg:240]
    │[fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240]│[fg:240]
    │[fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240]│[fg:240]
    │[fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240]│[fg:240]
    │[fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240]│[fg:240]
    ╰[fg:240]─[fg:240]─[fg:240]─[fg:240]─[fg:240]─[fg:240]─[fg:240]─[fg:240]─[fg:240]─[fg:240]─[fg:240]─[fg:240]─[fg:240]─[fg:240]─[fg:240]─[fg:240]─[fg:240]─[fg:240]─[fg:240]─[fg:240]─[fg:240]─[fg:240]─[fg:240]─[fg:240]─[fg:240]─[fg:240]─[fg:240]╯[fg:240]
    ");
}

#[test]
fn selection_bg_covers_inner_padding() {
    let pane = make_pane(AgentType::Claude, PaneStatus::Idle);
    let mut state = make_state(vec![SessionInfo {
        session_name: "main".into(),
        windows: vec![WindowInfo {
            window_id: "@1".into(),
            window_name: "project".into(),
            window_active: true,
            auto_rename: false,
            panes: vec![pane.clone()],
        }],
    }]);
    state.repo_groups = vec![make_repo_group("project", vec![pane])];
    state.rebuild_row_targets();
    state.focus_state.sidebar_focused = true;
    state.focus_state.focus = Focus::Panes;
    state.global.selected_pane_row = 0;

    // Styled snapshot locks in the selection background extending across the
    // inner padding immediately after the `┃` marker (` [bg:239]`).
    insta::assert_snapshot!(render_to_styled_string(&mut state, 28, 24), @"
     ≡[fg:111]1[fg:255]  ●[fg:245]0[fg:245]  ◎[fg:245]0[fg:245]  ◐[fg:245]0[fg:245]  ○[fg:245]1[fg:255]  ✕[fg:245]0[fg:245]
    ⓘ[fg:221]                        —[fg:252] ▾[fg:252]
       [fg:255] [fg:255]W[fg:255]a[fg:255]i[fg:255]t[fg:255]i[fg:255]n[fg:255]g[fg:255] [fg:255]f[fg:255]o[fg:255]r[fg:255] [fg:255]p[fg:255]r[fg:255]o[fg:255]m[fg:255]p[fg:255]t[fg:255]…[fg:255]

    ╭[fg:240] [fg:240]G[fg:252]i[fg:252]t[fg:252] [fg:240]─[fg:240]─[fg:240]─[fg:240]─[fg:240]─[fg:240]─[fg:240]─[fg:240]─[fg:240]─[fg:240]─[fg:240]─[fg:240]─[fg:240]─[fg:240]─[fg:240]─[fg:240]─[fg:240]─[fg:240]─[fg:240]─[fg:240]─[fg:240]─[fg:240]╮[fg:240]
    │[fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240]│[fg:240]
    │[fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240]│[fg:240]
    │[fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240]│[fg:240]
    │[fg:240] [fg:240] [fg:240] [fg:240] [fg:240]W[fg:252]o[fg:252]r[fg:252]k[fg:252]i[fg:252]n[fg:252]g[fg:252] [fg:252]t[fg:252]r[fg:252]e[fg:252]e[fg:252] [fg:252]c[fg:252]l[fg:252]e[fg:252]a[fg:252]n[fg:252] [fg:240] [fg:240] [fg:240] [fg:240]│[fg:240]
    │[fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240]│[fg:240]
    │[fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240]│[fg:240]
    │[fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240]│[fg:240]
    │[fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240]│[fg:240]
    ╰[fg:240]─[fg:240]─[fg:240]─[fg:240]─[fg:240]─[fg:240]─[fg:240]─[fg:240]─[fg:240]─[fg:240]─[fg:240]─[fg:240]─[fg:240]─[fg:240]─[fg:240]─[fg:240]─[fg:240]─[fg:240]─[fg:240]─[fg:240]─[fg:240]─[fg:240]─[fg:240]─[fg:240]─[fg:240]─[fg:240]─[fg:240]╯[fg:240]
    ╭[fg:240] [fg:240]A[fg:153]c[fg:153]t[fg:153]i[fg:153]v[fg:153]i[fg:153]t[fg:153]y[fg:153] [fg:240]─[fg:240]─[fg:240]─[fg:240]─[fg:240]─[fg:240]─[fg:240]─[fg:240]─[fg:240]─[fg:240]─[fg:240]─[fg:240]─[fg:240]─[fg:240]─[fg:240]─[fg:240]─[fg:240]╮[fg:240]
    │[fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240]│[fg:240]
    │[fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240]│[fg:240]
    │[fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240]│[fg:240]
    │[fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240]N[fg:252]o[fg:252] [fg:252]a[fg:252]c[fg:252]t[fg:252]i[fg:252]v[fg:252]i[fg:252]t[fg:252]y[fg:252] [fg:252]y[fg:252]e[fg:252]t[fg:252] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240]│[fg:240]
    │[fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240]│[fg:240]
    │[fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240]│[fg:240]
    │[fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240]│[fg:240]
    │[fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240]│[fg:240]
    ╰[fg:240]─[fg:240]─[fg:240]─[fg:240]─[fg:240]─[fg:240]─[fg:240]─[fg:240]─[fg:240]─[fg:240]─[fg:240]─[fg:240]─[fg:240]─[fg:240]─[fg:240]─[fg:240]─[fg:240]─[fg:240]─[fg:240]─[fg:240]─[fg:240]─[fg:240]─[fg:240]─[fg:240]─[fg:240]─[fg:240]─[fg:240]╯[fg:240]
    ");
}

#[test]
fn no_selection_bg_when_not_selected() {
    let pane = make_pane(AgentType::Claude, PaneStatus::Running);
    let mut state = make_state(vec![SessionInfo {
        session_name: "main".into(),
        windows: vec![WindowInfo {
            window_id: "@1".into(),
            window_name: "project".into(),
            window_active: true,
            auto_rename: false,
            panes: vec![pane.clone()],
        }],
    }]);
    state.repo_groups = vec![make_repo_group("project", vec![pane])];
    state.rebuild_row_targets();
    state.focus_state.sidebar_focused = false; // not focused → no selection

    // Styled snapshot locks in the absence of any selection background
    // (bg:239) while the sidebar is not focused.
    insta::assert_snapshot!(render_to_styled_string(&mut state, 28, 24), @"
     ≡[fg:111]1[fg:255]  ●[fg:245]1[fg:255]  ◎[fg:245]0[fg:245]  ◐[fg:245]0[fg:245]  ○[fg:245]0[fg:245]  ✕[fg:245]0[fg:245]
    ⓘ[fg:221]                        —[fg:252] ▾[fg:252]
    p[fg:153]r[fg:153]o[fg:153]j[fg:153]e[fg:153]c[fg:153]t[fg:153]

    ╭[fg:240] [fg:240]G[fg:252]i[fg:252]t[fg:252] [fg:240]─[fg:240]─[fg:240]─[fg:240]─[fg:240]─[fg:240]─[fg:240]─[fg:240]─[fg:240]─[fg:240]─[fg:240]─[fg:240]─[fg:240]─[fg:240]─[fg:240]─[fg:240]─[fg:240]─[fg:240]─[fg:240]─[fg:240]─[fg:240]─[fg:240]╮[fg:240]
    │[fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240]│[fg:240]
    │[fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240]│[fg:240]
    │[fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240]│[fg:240]
    │[fg:240] [fg:240] [fg:240] [fg:240] [fg:240]W[fg:252]o[fg:252]r[fg:252]k[fg:252]i[fg:252]n[fg:252]g[fg:252] [fg:252]t[fg:252]r[fg:252]e[fg:252]e[fg:252] [fg:252]c[fg:252]l[fg:252]e[fg:252]a[fg:252]n[fg:252] [fg:240] [fg:240] [fg:240] [fg:240]│[fg:240]
    │[fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240]│[fg:240]
    │[fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240]│[fg:240]
    │[fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240]│[fg:240]
    │[fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240]│[fg:240]
    ╰[fg:240]─[fg:240]─[fg:240]─[fg:240]─[fg:240]─[fg:240]─[fg:240]─[fg:240]─[fg:240]─[fg:240]─[fg:240]─[fg:240]─[fg:240]─[fg:240]─[fg:240]─[fg:240]─[fg:240]─[fg:240]─[fg:240]─[fg:240]─[fg:240]─[fg:240]─[fg:240]─[fg:240]─[fg:240]─[fg:240]─[fg:240]╯[fg:240]
    ╭[fg:240] [fg:240]A[fg:153]c[fg:153]t[fg:153]i[fg:153]v[fg:153]i[fg:153]t[fg:153]y[fg:153] [fg:240]─[fg:240]─[fg:240]─[fg:240]─[fg:240]─[fg:240]─[fg:240]─[fg:240]─[fg:240]─[fg:240]─[fg:240]─[fg:240]─[fg:240]─[fg:240]─[fg:240]─[fg:240]─[fg:240]╮[fg:240]
    │[fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240]│[fg:240]
    │[fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240]│[fg:240]
    │[fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240]│[fg:240]
    │[fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240]N[fg:252]o[fg:252] [fg:252]a[fg:252]c[fg:252]t[fg:252]i[fg:252]v[fg:252]i[fg:252]t[fg:252]y[fg:252] [fg:252]y[fg:252]e[fg:252]t[fg:252] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240]│[fg:240]
    │[fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240]│[fg:240]
    │[fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240]│[fg:240]
    │[fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240]│[fg:240]
    │[fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240] [fg:240]│[fg:240]
    ╰[fg:240]─[fg:240]─[fg:240]─[fg:240]─[fg:240]─[fg:240]─[fg:240]─[fg:240]─[fg:240]─[fg:240]─[fg:240]─[fg:240]─[fg:240]─[fg:240]─[fg:240]─[fg:240]─[fg:240]─[fg:240]─[fg:240]─[fg:240]─[fg:240]─[fg:240]─[fg:240]─[fg:240]─[fg:240]─[fg:240]─[fg:240]╯[fg:240]
    ");
}

// ─── Custom Theme Tests ─────────────────────────────────────────────

#[test]
fn snapshot_custom_theme_colors() {
    use ratatui::style::Color;
    use tmux_agent_sidebar::ui::colors::ColorTheme;

    let pane = make_pane(AgentType::Claude, PaneStatus::Idle);
    let mut state = make_state(vec![SessionInfo {
        session_name: "main".into(),
        windows: vec![WindowInfo {
            window_id: "@1".into(),
            window_name: "project".into(),
            window_active: true,
            auto_rename: false,
            panes: vec![pane.clone()],
        }],
    }]);
    state.repo_groups = vec![make_repo_group("project", vec![pane])];
    state.rebuild_row_targets();

    // Override theme with custom colors
    state.theme = ColorTheme {
        accent: Color::Indexed(196),       // red accent
        agent_claude: Color::Indexed(226), // yellow agent
        status_idle: Color::Indexed(46),   // green idle
        port: Color::Indexed(39),          // cyan port
        ..ColorTheme::default()
    };
    // Unfocus sidebar so selected row doesn't use REVERSED (which hides colors)
    state.focus_state.sidebar_focused = false;
    state.bottom_panel_height = 0;

    // Styled snapshot locks in the custom theme colors (accent fg:196,
    // agent_claude fg:226, status_idle fg:46).
    insta::assert_snapshot!(render_to_styled_string(&mut state, 28, 10), @r"
     ≡[fg:111]1[fg:255]  ●[fg:245]0[fg:245]  ◎[fg:245]0[fg:245]  ◐[fg:245]0[fg:245]  ○[fg:245]1[fg:255]  ✕[fg:245]0[fg:245]
    ⓘ[fg:221]                        —[fg:252] ▾[fg:252]
    p[fg:196]r[fg:196]o[fg:196]j[fg:196]e[fg:196]c[fg:196]t[fg:196]
    ┃[fg:196] ○[fg:46] [fg:226]c[fg:226]l[fg:226]a[fg:226]u[fg:226]d[fg:226]e[fg:226]
       [fg:255] [fg:255]W[fg:255]a[fg:255]i[fg:255]t[fg:255]i[fg:255]n[fg:255]g[fg:255] [fg:255]f[fg:255]o[fg:255]r[fg:255] [fg:255]p[fg:255]r[fg:255]o[fg:255]m[fg:255]p[fg:255]t[fg:255]…[fg:255]
    ");
}

#[test]
fn test_theme_default_matches_shell_colors() {
    use ratatui::style::Color;
    use tmux_agent_sidebar::ui::colors::ColorTheme;

    let theme = ColorTheme::default();

    // Verify defaults match shell version's agent-sidebar.conf
    assert_eq!(theme.accent, Color::Indexed(153));
    assert_eq!(theme.border_inactive, Color::Indexed(240));
    assert_eq!(theme.status_running, Color::Indexed(114));
    assert_eq!(theme.status_waiting, Color::Indexed(221));
    assert_eq!(theme.status_idle, Color::Indexed(110));
    assert_eq!(theme.status_error, Color::Indexed(167));
    assert_eq!(theme.agent_claude, Color::Indexed(174));
    assert_eq!(theme.agent_codex, Color::Indexed(141));
    assert_eq!(theme.text_active, Color::Indexed(255));
    assert_eq!(theme.text_muted, Color::Indexed(252));
    assert_eq!(theme.session_header, Color::Indexed(39));
    assert_eq!(theme.wait_reason, Color::Indexed(221));
}
