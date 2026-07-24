use ratatui::{
    style::{Modifier, Style},
    text::{Line, Span},
};

use super::ctx::RowCtx;
use crate::codex_agents::{CodexAgentInfo, CodexAgentStatus};
use crate::codex_usage::{CodexTokenUsage, compact_token_count};
use crate::tmux::{PaneStatus, SubagentInfo};
use crate::ui::text::{
    display_width, elapsed_label, truncate_to_width, wait_reason_label, wrap_text, wrap_text_char,
};

pub(super) fn token_usage_row(usage: &CodexTokenUsage, ctx: &RowCtx) -> Line<'static> {
    let total = compact_token_count(usage.total_tokens);
    let left_prefix = "  tok ";
    let left_width = display_width(left_prefix) + display_width(&total);
    let left_spans = vec![
        Span::styled(
            left_prefix,
            ctx.apply_bg(Style::default().fg(ctx.theme.text_muted)),
        ),
        Span::styled(
            total,
            ctx.apply_bg(Style::default().fg(ctx.theme.agent_codex)),
        ),
    ];

    let Some(percent) = usage.context_percent() else {
        return ctx.row_line(left_spans, left_width);
    };
    let right = format!("ctx {percent}%");
    let right_width = display_width(&right);
    if left_width + right_width > ctx.inner_width {
        return ctx.row_line(left_spans, left_width);
    }

    ctx.row_line_split(
        left_spans,
        left_width,
        vec![Span::styled(
            right,
            ctx.apply_bg(Style::default().fg(ctx.theme.text_active)),
        )],
        right_width,
    )
}

pub(super) fn task_progress_row(
    task_progress: Option<&crate::activity::TaskProgress>,
    ctx: &RowCtx,
) -> Option<Line<'static>> {
    use crate::activity::TaskStatus;
    let progress = task_progress?;
    if progress.is_empty() {
        return None;
    }

    let mut icons = String::with_capacity(progress.tasks.len() * 3);
    for (_, status) in &progress.tasks {
        let ch = match status {
            TaskStatus::Completed => "✔",
            TaskStatus::InProgress => "◼",
            TaskStatus::Pending => "◻",
        };
        icons.push_str(ch);
    }
    let summary = format!(
        "  {} {}/{}",
        icons,
        progress.completed_count(),
        progress.total()
    );
    let summary_dw = display_width(&summary);
    let task_color = ctx.theme.task_progress;
    Some(ctx.row_line(
        vec![Span::styled(
            summary,
            ctx.apply_bg(Style::default().fg(task_color)),
        )],
        summary_dw,
    ))
}

pub(super) fn subagent_rows(
    subagents: &[SubagentInfo],
    ctx: &RowCtx,
    now: u64,
) -> Vec<Line<'static>> {
    if subagents.is_empty() {
        return Vec::new();
    }
    let theme = ctx.theme;
    let subagent_color = theme.subagent;
    let tree_color = theme.text_muted;
    let last_idx = subagents.len() - 1;
    let mut out = Vec::with_capacity(subagents.len());
    for (i, subagent) in subagents.iter().enumerate() {
        let connector = if i == last_idx { "└ " } else { "├ " };
        let numbered = if subagent.label.contains('#') {
            subagent.label.clone()
        } else {
            format!("{} #{}", subagent.label, i + 1)
        };

        let elapsed = elapsed_label(subagent.started_at, now);
        let max_elapsed_width = ctx.inner_width.saturating_sub(2);
        let elapsed = truncate_to_width(&elapsed, max_elapsed_width);
        let elapsed_gap = usize::from(!elapsed.is_empty());
        let active_width = usize::from(ctx.inner_width > 0);
        let right_width = active_width + elapsed_gap + display_width(&elapsed);
        let right_gap = usize::from(ctx.inner_width > right_width);
        let left_budget = ctx.inner_width.saturating_sub(right_width + right_gap);

        let prefix = format!("  {}", connector);
        let prefix = truncate_to_width(&prefix, left_budget);
        let prefix_dw = display_width(&prefix);
        let max_sa_w = left_budget.saturating_sub(prefix_dw);
        let truncated_sa = truncate_to_width(&numbered, max_sa_w);
        let left_width = prefix_dw + display_width(&truncated_sa);

        let mut right_spans = Vec::with_capacity(2);
        if active_width > 0 {
            right_spans.push(Span::styled(
                "●",
                ctx.apply_bg(Style::default().fg(theme.status_running)),
            ));
        }
        if !elapsed.is_empty() {
            right_spans.push(Span::styled(
                format!(" {elapsed}"),
                ctx.apply_bg(Style::default().fg(theme.text_active)),
            ));
        }

        out.push(ctx.row_line_split(
            vec![
                Span::styled(prefix, ctx.apply_bg(Style::default().fg(tree_color))),
                Span::styled(
                    truncated_sa,
                    ctx.apply_bg(Style::default().fg(subagent_color)),
                ),
            ],
            left_width,
            right_spans,
            right_width,
        ));
    }
    out
}

fn id_prefix(id: &str) -> String {
    id.chars().take(8).collect()
}

pub(super) fn codex_agent_rows(
    parent_session_id: Option<&str>,
    agents: &[CodexAgentInfo],
    ctx: &RowCtx<'_>,
    now: u64,
) -> Vec<Line<'static>> {
    if agents.is_empty() {
        return Vec::new();
    }

    let theme = ctx.theme;
    let tree_style = ctx.apply_bg(Style::default().fg(theme.text_muted));
    let label_style = ctx.apply_bg(Style::default().fg(theme.subagent));
    let id_style = ctx.apply_bg(Style::default().fg(theme.text_muted));
    let duration_style = ctx.apply_bg(Style::default().fg(theme.text_active));
    let mut out = Vec::with_capacity(agents.len() + 1);

    let main_prefix = "  ├ ";
    let main_id = parent_session_id.map(id_prefix).unwrap_or_default();
    let main_right_width = display_width(&main_id);
    let main_gap = usize::from(!main_id.is_empty() && ctx.inner_width > main_right_width);
    let main_left_budget = ctx.inner_width.saturating_sub(main_right_width + main_gap);
    let main_prefix = truncate_to_width(main_prefix, main_left_budget);
    let main_prefix_width = display_width(&main_prefix);
    let main_label = truncate_to_width(
        "Main [default] (current)",
        main_left_budget.saturating_sub(main_prefix_width),
    );
    let main_left_width = main_prefix_width + display_width(&main_label);
    let main_right = if main_id.is_empty() {
        Vec::new()
    } else {
        vec![Span::styled(main_id, id_style)]
    };
    out.push(ctx.row_line_split(
        vec![
            Span::styled(main_prefix, tree_style),
            Span::styled(main_label, label_style),
        ],
        main_left_width,
        main_right,
        main_right_width,
    ));

    let last_idx = agents.len() - 1;
    for (index, agent) in agents.iter().enumerate() {
        let connector = if index == last_idx {
            "  └ "
        } else {
            "  ├ "
        };
        let label = if agent.path.is_empty() {
            agent.fallback_agent_type.clone()
        } else {
            agent.path.clone()
        };
        let id = id_prefix(&agent.id);
        let (status, status_color) = match agent.status {
            CodexAgentStatus::Working => ("● working", theme.status_running),
            CodexAgentStatus::Done => ("✓ done", theme.status_idle),
            CodexAgentStatus::Interrupted => ("○ interrupted", theme.status_waiting),
            CodexAgentStatus::Unknown => ("? unknown", theme.status_unknown),
        };
        let duration = match agent.status {
            CodexAgentStatus::Done => {
                elapsed_label(agent.started_at, agent.finished_at.unwrap_or(0))
            }
            CodexAgentStatus::Working => elapsed_label(agent.started_at, now),
            CodexAgentStatus::Interrupted | CodexAgentStatus::Unknown => String::new(),
        };

        let mandatory_right_width = display_width(&id) + 2 + display_width(status);
        let duration_width = display_width(&duration);
        let include_duration = !duration.is_empty()
            && ctx.inner_width
                >= display_width(connector) + 1 + 1 + mandatory_right_width + 1 + duration_width;
        let right_width =
            mandatory_right_width + usize::from(include_duration) * (1 + duration_width);
        let right_gap = usize::from(ctx.inner_width > right_width);
        let left_budget = ctx.inner_width.saturating_sub(right_width + right_gap);
        let connector = truncate_to_width(connector, left_budget);
        let connector_width = display_width(&connector);
        let label = truncate_to_width(&label, left_budget.saturating_sub(connector_width));
        let left_width = connector_width + display_width(&label);

        let mut right_spans = vec![
            Span::styled(id, id_style),
            Span::styled("  ", ctx.apply_bg(Style::default())),
            Span::styled(status, ctx.apply_bg(Style::default().fg(status_color))),
        ];
        if include_duration {
            right_spans.push(Span::styled(format!(" {duration}"), duration_style));
        }

        out.push(ctx.row_line_split(
            vec![
                Span::styled(connector, tree_style),
                Span::styled(label, label_style),
            ],
            left_width,
            right_spans,
            right_width,
        ));
    }

    out
}

pub(super) fn wait_reason_row(
    wait_reason: &str,
    status: &PaneStatus,
    ctx: &RowCtx,
) -> Option<Line<'static>> {
    if wait_reason.is_empty() {
        return None;
    }
    let reason = wait_reason_label(wait_reason);
    let text = format!("  {}", reason);
    let text_dw = display_width(&text);
    let reason_color = if matches!(status, PaneStatus::Error) {
        ctx.theme.status_error
    } else {
        ctx.theme.wait_reason
    };
    Some(ctx.row_line(
        vec![Span::styled(
            text,
            ctx.apply_bg(Style::default().fg(reason_color)),
        )],
        text_dw,
    ))
}

pub(super) fn background_hint_row(ctx: &RowCtx, cmd: &str) -> Line<'static> {
    const PREFIX: &str = "  $ ";
    let room = ctx.inner_width.saturating_sub(display_width(PREFIX));
    let shown = truncate_to_width(cmd.trim(), room);
    let text = format!("{PREFIX}{shown}");
    let text_dw = display_width(&text);
    ctx.row_line(
        vec![Span::styled(
            text,
            ctx.apply_bg(Style::default().fg(ctx.theme.status_running)),
        )],
        text_dw,
    )
}

pub(super) fn prompt_rows(pane: &crate::tmux::PaneInfo, ctx: &RowCtx) -> Vec<Line<'static>> {
    let theme = ctx.theme;
    let is_response = pane.prompt_is_response;
    let prompt_color = if ctx.active {
        theme.text_active
    } else {
        theme.text_inactive
    };
    let wrap_width = ctx.inner_width.saturating_sub(2);
    let wrapped = if is_response {
        wrap_text_char(&pane.prompt, wrap_width, 3)
    } else {
        wrap_text(&pane.prompt, wrap_width, 3)
    };

    let mut out = Vec::with_capacity(wrapped.len());
    for (li, wl) in wrapped.iter().enumerate() {
        if is_response && li == 0 {
            let arrow_color = theme.response_arrow;
            let text_dw = 2 + display_width(wl); // "▷ " width
            out.push(ctx.row_line(
                vec![
                    Span::styled(
                        "▷ ",
                        ctx.apply_bg(
                            Style::default()
                                .fg(arrow_color)
                                .add_modifier(Modifier::BOLD),
                        ),
                    ),
                    Span::styled(wl.clone(), ctx.apply_bg(Style::default().fg(prompt_color))),
                ],
                text_dw,
            ));
        } else {
            let indent = "  ";
            let text = format!("{}{}", indent, wl);
            let text_dw = display_width(&text);
            out.push(ctx.row_line(
                vec![Span::styled(
                    text,
                    ctx.apply_bg(Style::default().fg(prompt_color)),
                )],
                text_dw,
            ));
        }
    }
    out
}

pub(super) fn idle_hint_row(ctx: &RowCtx) -> Line<'static> {
    let text = "  Waiting for prompt…";
    let text_dw = display_width(text);
    let idle_color = if ctx.active {
        ctx.theme.text_active
    } else {
        ctx.theme.text_inactive
    };
    ctx.row_line(
        vec![Span::styled(
            text.to_string(),
            ctx.apply_bg(Style::default().fg(idle_color)),
        )],
        text_dw,
    )
}
