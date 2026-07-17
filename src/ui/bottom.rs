mod activity;
mod git;

use ratatui::{
    Frame,
    layout::{Alignment, Rect},
    style::{Color, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph},
};

use crate::state::{AppState, BottomPanel, Focus};

use super::text::display_width;

fn render_centered(frame: &mut Frame, area: Rect, text: &str, color: Color) {
    // Vertically center: pad with empty lines above
    let top_pad = area.height.saturating_sub(1) / 2;
    let mut lines: Vec<Line<'_>> = Vec::new();
    for _ in 0..top_pad {
        lines.push(Line::from(""));
    }
    lines.push(Line::from(Span::styled(text, Style::default().fg(color))));
    let paragraph = Paragraph::new(lines).alignment(Alignment::Center);
    frame.render_widget(paragraph, area);
}

fn split_bottom_areas(area: Rect) -> (Rect, Rect) {
    let git_height = area.height / 2;
    let activity_height = area.height - git_height;
    (
        Rect::new(area.x, area.y, area.width, git_height),
        Rect::new(
            area.x,
            area.y.saturating_add(git_height),
            area.width,
            activity_height,
        ),
    )
}

fn draw_card_frame(
    frame: &mut Frame,
    state: &AppState,
    area: Rect,
    title: &str,
    selected: bool,
) -> Option<Rect> {
    if area.width == 0 || area.height == 0 {
        return None;
    }

    let border_color = if selected && state.focus_state.focus == Focus::BottomPanel {
        state.theme.accent
    } else {
        state.theme.border_inactive
    };
    let title_color = if selected {
        state.theme.accent
    } else {
        state.theme.text_muted
    };

    let block = Block::default()
        .borders(Borders::ALL)
        .style(Style::default().fg(border_color));
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let title_width = display_width(title);
    let fill_width = (area.width as usize).saturating_sub(title_width + 4);
    let top_line = Line::from(vec![
        Span::styled("╭ ", Style::default().fg(border_color)),
        Span::styled(title.to_string(), Style::default().fg(title_color)),
        Span::styled(
            format!(" {}╮", "─".repeat(fill_width)),
            Style::default().fg(border_color),
        ),
    ]);
    frame.render_widget(
        Paragraph::new(top_line),
        Rect::new(area.x, area.y, area.width, 1),
    );

    let bottom_line = Line::from(Span::styled(
        format!("╰{}╯", "─".repeat((area.width as usize).saturating_sub(2))),
        Style::default().fg(border_color),
    ));
    let bottom_rect = Rect::new(
        area.x,
        area.y + area.height.saturating_sub(1),
        area.width,
        1,
    );
    frame.render_widget(Paragraph::new(bottom_line), bottom_rect);

    (inner.width > 0 && inner.height > 0).then_some(inner)
}

pub fn draw_bottom(frame: &mut Frame, state: &mut AppState, area: Rect) {
    let (git_area, activity_area) = split_bottom_areas(area);

    if let Some(inner) = draw_card_frame(
        frame,
        state,
        git_area,
        "Git",
        state.active_bottom_panel == BottomPanel::Git,
    ) {
        git::draw_git_content(frame, state, inner);
    }
    if let Some(inner) = draw_card_frame(
        frame,
        state,
        activity_area,
        "Activity",
        state.active_bottom_panel == BottomPanel::Activity,
    ) {
        activity::draw_activity_content(frame, state, inner);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::{Terminal, backend::TestBackend};

    use crate::ui::text::truncate_to_width;

    fn render_bottom(state: &mut AppState, width: u16, height: u16) -> String {
        let backend = TestBackend::new(width, height);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|frame| draw_bottom(frame, state, Rect::new(0, 0, width, height)))
            .unwrap();
        let buffer = terminal.backend().buffer();
        (0..height)
            .map(|y| {
                let mut line = String::new();
                for x in 0..width {
                    line.push_str(buffer[(x, y)].symbol());
                }
                line.trim_end().to_string()
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    #[test]
    fn split_bottom_areas_even_height() {
        let area = Rect::new(3, 5, 28, 20);
        let (git, activity) = split_bottom_areas(area);
        assert_eq!(git, Rect::new(3, 5, 28, 10));
        assert_eq!(activity, Rect::new(3, 15, 28, 10));
    }

    #[test]
    fn split_bottom_areas_odd_height_gives_extra_row_to_activity() {
        let area = Rect::new(3, 5, 28, 9);
        let (git, activity) = split_bottom_areas(area);
        assert_eq!(git, Rect::new(3, 5, 28, 4));
        assert_eq!(activity, Rect::new(3, 9, 28, 5));
    }

    #[test]
    fn split_bottom_areas_one_row_skips_git() {
        let area = Rect::new(0, 0, 28, 1);
        let (git, activity) = split_bottom_areas(area);
        assert_eq!(git.height, 0);
        assert_eq!(activity, Rect::new(0, 0, 28, 1));
    }

    #[test]
    fn renders_git_above_activity_in_equal_cards() {
        let mut state = AppState::new("%99".into());
        insta::assert_snapshot!(render_bottom(&mut state, 28, 20), @r"
        ╭ Git ─────────────────────╮
        │                          │
        │                          │
        │                          │
        │    Working tree clean    │
        │                          │
        │                          │
        │                          │
        │                          │
        ╰──────────────────────────╯
        ╭ Activity ────────────────╮
        │                          │
        │                          │
        │                          │
        │      No activity yet     │
        │                          │
        │                          │
        │                          │
        │                          │
        ╰──────────────────────────╯
        ");
    }

    #[test]
    fn renders_populated_git_and_activity_together() {
        let mut state = AppState::new("%99".into());
        state.git.branch = "main".into();
        state.git.diff_stat = Some((3, 1));
        state.git.unstaged_files = vec![crate::git::GitFileEntry {
            status: 'M',
            name: "src/lib.rs".into(),
            additions: 3,
            deletions: 1,
            path: String::new(),
        }];
        state.activity.entries = vec![crate::activity::ActivityEntry {
            timestamp: "10:00".into(),
            tool: "Read".into(),
            label: "src/lib.rs".into(),
        }];

        insta::assert_snapshot!(render_bottom(&mut state, 28, 20), @r"
        ╭ Git ─────────────────────╮
        │                          │
        │main                      │
        │+3/-1              1 files│
        │──────────────────────────│
        │Unstaged (1)              │
        │M src/lib.rs         +3/-1│
        │                          │
        │                          │
        ╰──────────────────────────╯
        ╭ Activity ────────────────╮
        │                          │
        │10:00                 Read│
        │  src/lib.rs              │
        │                          │
        │                          │
        │                          │
        │                          │
        │                          │
        ╰──────────────────────────╯
        ");
    }

    #[test]
    fn truncate_to_width_short() {
        assert_eq!(truncate_to_width("hello", 10), "hello");
    }

    #[test]
    fn truncate_to_width_exact() {
        assert_eq!(truncate_to_width("hello", 5), "hello");
    }

    #[test]
    fn truncate_to_width_truncated() {
        let result = truncate_to_width("hello world", 8);
        assert!(result.ends_with('…'));
        assert!(result.len() <= 10); // 7 chars + ellipsis in bytes
    }
}
