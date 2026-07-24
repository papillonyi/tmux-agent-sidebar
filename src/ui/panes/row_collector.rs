use ratatui::{
    style::Style,
    text::{Line, Span},
};

use super::SPAWN_BUTTON;
use super::row;
use crate::group::{PaneGitInfo, RepoGroup};
use crate::state::{AppState, Focus};
use crate::tmux::PaneInfo;
use crate::ui::text::{display_width, truncate_to_width};

#[derive(Debug, Default)]
pub(super) struct CollectedRows {
    pub lines: Vec<Line<'static>>,
    pub line_to_row: Vec<Option<usize>>,
    pub pending_spawn: Vec<(usize, String, String)>,
    pub pending_remove: Vec<(usize, u16, String)>,
}

impl CollectedRows {
    fn push_blank(&mut self) {
        self.lines.push(Line::from(""));
        self.line_to_row.push(None);
    }

    fn append(&mut self, mut other: Self) {
        let line_offset = self.lines.len();
        for (line, _, _) in &mut other.pending_spawn {
            *line += line_offset;
        }
        for (line, _, _) in &mut other.pending_remove {
            *line += line_offset;
        }
        self.lines.append(&mut other.lines);
        self.line_to_row.append(&mut other.line_to_row);
        self.pending_spawn.append(&mut other.pending_spawn);
        self.pending_remove.append(&mut other.pending_remove);
    }
}

fn push_repo_header(
    state: &AppState,
    width: usize,
    collected: &mut CollectedRows,
    group: &RepoGroup,
    panes: &[&(PaneInfo, PaneGitInfo)],
) {
    let theme = &state.theme;
    let group_has_focused_pane = state
        .focus_state
        .focused_pane_id
        .as_ref()
        .is_some_and(|fid| panes.iter().any(|(pane, _)| pane.pane_id == *fid));

    let title = &group.name;
    let title_color = if group_has_focused_pane {
        theme.accent
    } else {
        theme.text_active
    };
    let repo_root = panes.iter().find_map(|(_, git)| git.repo_root.clone());
    let spans: Vec<Span<'static>> = if let Some(ref root) = repo_root {
        let title_w = display_width(title);
        let pad_width = width
            .saturating_sub(title_w)
            .saturating_sub(SPAWN_BUTTON.len());
        collected
            .pending_spawn
            .push((collected.lines.len(), group.name.clone(), root.clone()));
        let button_color = if group_has_focused_pane {
            theme.accent
        } else {
            theme.text_active
        };
        vec![
            Span::styled(title.clone(), Style::default().fg(title_color)),
            Span::raw(" ".repeat(pad_width)),
            Span::styled(SPAWN_BUTTON, Style::default().fg(button_color)),
        ]
    } else {
        vec![Span::styled(
            title.clone(),
            Style::default().fg(title_color),
        )]
    };
    collected.lines.push(Line::from(spans));
    collected.line_to_row.push(None);
}

fn push_pane(
    state: &AppState,
    width: usize,
    collected: &mut CollectedRows,
    row_index: &mut usize,
    pane: &PaneInfo,
    git_info: &PaneGitInfo,
) {
    let is_selected = state.focus_state.sidebar_focused
        && state.focus_state.focus == Focus::Panes
        && *row_index == state.global.selected_pane_row;

    let is_active = state.focus_state.focused_pane_id.as_ref() == Some(&pane.pane_id);
    let has_focus_enclosure = row::has_focus_enclosure(pane, is_active, width);

    let pane_state = state.pane_state(&pane.pane_id);
    let ports = pane_state.map(|s| s.ports.as_slice());
    let task_progress = pane_state.and_then(|s| s.task_progress.as_ref());
    let token_usage = pane_state.and_then(|s| s.codex_token_usage.as_ref());
    let codex_agents = pane_state
        .map(|state| state.codex_agents.as_slice())
        .filter(|agents| !agents.is_empty());
    let codex_main_model = pane_state.and_then(|state| state.codex_main_model.as_deref());
    let status_line_idx = collected.lines.len();
    let pane_lines = row::render_pane_lines_with_runtime(
        pane,
        git_info,
        ports,
        task_progress,
        token_usage,
        codex_agents,
        codex_main_model,
        is_selected,
        is_active,
        width,
        &state.icons,
        &state.theme,
        state.spinner_frame,
        state.now,
    );
    let pane_line_count = pane_lines.len();
    collected.lines.extend(pane_lines);
    for _ in 0..pane_line_count {
        collected.line_to_row.push(Some(*row_index));
    }

    if pane.sidebar_spawned
        && git_info.is_worktree
        && pane_line_count >= 2
        && let Some(x) = row::sidebar_remove_marker_col(
            git_info,
            ports,
            true,
            width.saturating_sub(if has_focus_enclosure { 3 } else { 2 }),
        )
    {
        collected
            .pending_remove
            .push((status_line_idx + 1, x, pane.pane_id.clone()));
    }

    *row_index += 1;
}

fn collect_current_window(
    state: &AppState,
    width: usize,
    collected: &mut CollectedRows,
    row_index: &mut usize,
) {
    let ordered_panes: Vec<(usize, &(PaneInfo, PaneGitInfo))> = state
        .ordered_current_window_entries()
        .into_iter()
        .map(|(group_index, pane_index)| {
            (
                group_index,
                &state.repo_groups[group_index].panes[pane_index],
            )
        })
        .collect();

    let mut start = 0;
    while start < ordered_panes.len() {
        let group_index = ordered_panes[start].0;
        let mut end = start + 1;
        while end < ordered_panes.len() && ordered_panes[end].0 == group_index {
            end += 1;
        }
        if start > 0 {
            collected.push_blank();
        }
        let run: Vec<&(PaneInfo, PaneGitInfo)> = ordered_panes[start..end]
            .iter()
            .map(|(_, pane)| *pane)
            .collect();
        push_repo_header(
            state,
            width,
            collected,
            &state.repo_groups[group_index],
            &run,
        );
        for (pane, git_info) in run {
            push_pane(state, width, collected, row_index, pane, git_info);
        }
        start = end;
    }
}

fn collect_repo_groups(
    state: &AppState,
    width: usize,
    collected: &mut CollectedRows,
    row_index: &mut usize,
    pane_matches: impl Fn(&PaneInfo) -> bool,
) {
    let filter = state.global.status_filter;
    let mut first_group = true;

    for group in &state.repo_groups {
        if !state.global.repo_filter.matches_group(&group.name) {
            continue;
        }
        let filtered_panes: Vec<_> = group
            .panes
            .iter()
            .filter(|(pane, _)| filter.matches(&pane.status) && pane_matches(pane))
            .collect();
        if filtered_panes.is_empty() {
            continue;
        }

        if !first_group {
            // Separate repo groups, but do not add a leading blank before
            // the first repo so the list starts immediately below the header.
            collected.push_blank();
        }
        first_group = false;
        push_repo_header(state, width, collected, group, &filtered_panes);
        for (pane, git_info) in filtered_panes {
            push_pane(state, width, collected, row_index, pane, git_info);
        }
    }
}

pub(super) fn collect(state: &AppState, width: u16, visible_height: u16) -> CollectedRows {
    let width = width as usize;
    let mut row_index = 0;

    let mut current = CollectedRows::default();
    collect_current_window(state, width, &mut current, &mut row_index);

    let mut other = CollectedRows::default();
    for (location_index, location) in state
        .visible_other_window_locations()
        .into_iter()
        .enumerate()
    {
        if location_index > 0 {
            other.push_blank();
        }
        let label = truncate_to_width(&format!("↳ {}", location.label()), width);
        other.lines.push(Line::from(Span::styled(
            label,
            Style::default().fg(state.theme.session_header),
        )));
        other.line_to_row.push(None);

        collect_repo_groups(state, width, &mut other, &mut row_index, |pane| {
            state.pane_locations.get(&pane.pane_id) == Some(&location)
        });
    }

    if other.lines.is_empty() {
        return current;
    }

    // When everything fits, expand the gap so other-window agents sit at
    // the bottom of the pane list, immediately above the divider/Git area.
    // Under pressure the gap collapses to one separator row and the existing
    // list scrolling takes over.
    let minimum_gap = usize::from(!current.lines.is_empty());
    let flexible_gap = (visible_height as usize)
        .saturating_sub(current.lines.len() + other.lines.len())
        .max(minimum_gap);
    for _ in 0..flexible_gap {
        current.push_blank();
    }
    current.append(other);

    current
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::group::{PaneGitInfo, RepoGroup};
    use crate::state::{AppState, PaneLocation, StatusFilter};
    use crate::tmux::{AgentType, PaneInfo, PaneStatus, PermissionMode, WorktreeMetadata};

    fn make_pane(id: &str, status: PaneStatus) -> PaneInfo {
        PaneInfo {
            pane_id: id.into(),
            pane_active: false,
            status,
            attention: None,
            agent: AgentType::Claude,
            path: "/tmp/repo".into(),
            current_command: String::new(),
            prompt: String::new(),
            prompt_is_response: false,
            started_at: None,
            wait_reason: String::new(),
            permission_mode: PermissionMode::Default,
            subagents: vec![],
            pane_pid: None,
            worktree: WorktreeMetadata::default(),
            session_id: None,
            session_name: String::new(),
            sidebar_spawned: false,
            bg_shell_cmd: None,
        }
    }

    #[test]
    fn collect_empty_repo_groups_produces_no_lines() {
        let state = AppState::new("%0".into());
        let collected = collect(&state, 40, 20);
        assert!(collected.lines.is_empty());
        assert!(collected.line_to_row.is_empty());
        assert!(collected.pending_spawn.is_empty());
        assert!(collected.pending_remove.is_empty());
    }

    #[test]
    fn collect_skips_group_when_status_filter_excludes_all_panes() {
        let mut state = AppState::new("%0".into());
        // The group has only Running panes, so filter to Waiting to drop them all.
        state.global.status_filter = StatusFilter::Waiting;
        state.repo_groups = vec![RepoGroup {
            name: "repo".into(),
            has_focus: false,
            panes: vec![(make_pane("%1", PaneStatus::Running), PaneGitInfo::default())],
        }];
        let collected = collect(&state, 40, 20);
        assert!(collected.lines.is_empty());
        assert!(collected.pending_spawn.is_empty());
    }

    #[test]
    fn collect_records_pending_spawn_when_repo_root_present() {
        let mut state = AppState::new("%0".into());
        let git_info = PaneGitInfo {
            repo_root: Some("/tmp/repo".into()),
            branch: None,
            is_worktree: false,
            worktree_name: None,
        };
        state.repo_groups = vec![RepoGroup {
            name: "repo".into(),
            has_focus: false,
            panes: vec![(make_pane("%1", PaneStatus::Running), git_info)],
        }];
        let collected = collect(&state, 40, 20);
        assert_eq!(
            collected.pending_spawn.len(),
            1,
            "groups with a repo_root should emit a spawn target"
        );
        assert_eq!(collected.pending_spawn[0].1, "repo");
        assert_eq!(collected.pending_spawn[0].2, "/tmp/repo");
        // At least the header plus one pane row should have been pushed.
        assert!(!collected.lines.is_empty());
    }

    #[test]
    fn collect_no_pending_spawn_without_repo_root() {
        let mut state = AppState::new("%0".into());
        state.repo_groups = vec![RepoGroup {
            name: "raw-path".into(),
            has_focus: false,
            panes: vec![(make_pane("%1", PaneStatus::Running), PaneGitInfo::default())],
        }];
        let collected = collect(&state, 40, 20);
        assert!(
            collected.pending_spawn.is_empty(),
            "groups without repo_root must not produce spawn targets"
        );
    }

    #[test]
    fn collect_pending_spawn_grows_with_repo_root_bearing_groups() {
        let mut state = AppState::new("%0".into());
        let with_root = |root: &str, name: &str, pane_id: &str| RepoGroup {
            name: name.into(),
            has_focus: false,
            panes: vec![(
                make_pane(pane_id, PaneStatus::Running),
                PaneGitInfo {
                    repo_root: Some(root.into()),
                    branch: None,
                    is_worktree: false,
                    worktree_name: None,
                },
            )],
        };
        state.repo_groups = vec![
            with_root("/repo/a", "a", "%1"),
            with_root("/repo/b", "b", "%2"),
            with_root("/repo/c", "c", "%3"),
        ];
        let collected = collect(&state, 40, 20);
        assert_eq!(collected.pending_spawn.len(), 3);
    }

    #[test]
    fn other_window_spawn_target_moves_with_bottom_alignment_gap() {
        let mut state = AppState::new("%0".into());
        state.current_window_id = "@current".into();
        let git_info = |root: &str| PaneGitInfo {
            repo_root: Some(root.into()),
            branch: None,
            is_worktree: false,
            worktree_name: None,
        };
        state.repo_groups = vec![
            RepoGroup {
                name: "current".into(),
                has_focus: true,
                panes: vec![(
                    make_pane("%current", PaneStatus::Running),
                    git_info("/repo/current"),
                )],
            },
            RepoGroup {
                name: "other".into(),
                has_focus: false,
                panes: vec![(
                    make_pane("%other", PaneStatus::Running),
                    git_info("/repo/other"),
                )],
            },
        ];
        state.pane_locations.insert(
            "%current".into(),
            PaneLocation {
                session_name: "main".into(),
                window_id: "@current".into(),
                window_name: "editor".into(),
            },
        );
        state.pane_locations.insert(
            "%other".into(),
            PaneLocation {
                session_name: "main".into(),
                window_id: "@other".into(),
                window_name: "api".into(),
            },
        );
        state.rebuild_row_targets();

        let collected = collect(&state, 40, 10);

        let spawn_lines: Vec<usize> = collected
            .pending_spawn
            .iter()
            .map(|(line, _, _)| *line)
            .collect();
        assert_eq!(spawn_lines, vec![0, 8]);
        assert_eq!(collected.line_to_row[9], Some(1));
    }

    #[test]
    fn focused_codex_remove_target_sits_before_right_enclosure() {
        let mut state = AppState::new("%0".into());
        state.focus_state.focused_pane_id = Some("%1".into());

        let mut pane = make_pane("%1", PaneStatus::Running);
        pane.agent = AgentType::Codex;
        pane.sidebar_spawned = true;
        let git_info = PaneGitInfo {
            repo_root: Some("/tmp/repo".into()),
            branch: Some("feature/focus".into()),
            is_worktree: true,
            worktree_name: None,
        };
        state.repo_groups = vec![RepoGroup {
            name: "repo".into(),
            has_focus: true,
            panes: vec![(pane, git_info)],
        }];

        let collected = collect(&state, 30, 20);
        assert_eq!(collected.pending_remove, vec![(2, 28, "%1".into())]);
    }
}
