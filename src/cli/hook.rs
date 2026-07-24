use crate::event::{AgentEvent, resolve_adapter};

use super::{read_stdin_json, tmux_pane};

mod activity;
mod context;
mod handlers;
mod notifications;

use context::{sync_pane_location, sync_transcript_path};
use notifications::notification_settings;

fn sync_event_transcript_path(
    pane: &str,
    kind: crate::event::AgentEventKind,
    transcript_path: Option<&str>,
) {
    if matches!(
        kind,
        crate::event::AgentEventKind::SubagentStart | crate::event::AgentEventKind::SubagentStop
    ) {
        return;
    }
    sync_transcript_path(
        pane,
        transcript_path,
        kind == crate::event::AgentEventKind::SessionStart,
    );
}

fn resolve_parent_session_id(pane: &str, event_session_id: Option<&str>) -> Option<String> {
    event_session_id
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
        .or_else(|| {
            let stored = crate::tmux::get_pane_option_value(pane, crate::tmux::PANE_SESSION_ID);
            (!stored.is_empty()).then_some(stored)
        })
}

fn append_codex_lifecycle(
    pane: &str,
    agent_name: &str,
    parent_session_id: Option<&str>,
    agent_id: Option<&str>,
    agent_type: &str,
    status: crate::codex_agents::CodexAgentStatus,
) {
    if agent_name != crate::tmux::CODEX_AGENT {
        return;
    }
    let Some(agent_id) = agent_id.filter(|value| !value.is_empty()) else {
        return;
    };
    let Some(parent_session_id) = resolve_parent_session_id(pane, parent_session_id) else {
        return;
    };
    let _ = crate::codex_agents::journal::append_lifecycle_event(
        pane,
        &parent_session_id,
        agent_id,
        agent_type,
        status,
        crate::time::now_epoch_millis(),
    );
}

// ─── hook subcommand ────────────────────────────────────────────────────────

pub(crate) fn cmd_hook(args: &[String]) -> i32 {
    let agent_name = args.first().map(|s| s.as_str()).unwrap_or("");
    let event_name = args.get(1).map(|s| s.as_str()).unwrap_or("");

    if agent_name.is_empty() || event_name.is_empty() {
        return 0;
    }

    let Some(adapter) = resolve_adapter(agent_name) else {
        return 0;
    };

    let pane = tmux_pane();
    if pane.is_empty() {
        return 0;
    }

    let input = read_stdin_json();
    let transcript_path = adapter.transcript_path(&input);
    let Some(event) = adapter.parse(event_name, &input) else {
        return 0;
    };

    // SessionStart begins a new parent session, so an absent path clears any
    // stale value. Other events only fill or refresh a path when Codex reports
    // one. Token values themselves are read by the TUI refresh loop.
    sync_event_transcript_path(&pane, event.kind(), transcript_path.as_deref());

    handle_event(&pane, agent_name, event)
}

// ─── event handler ──────────────────────────────────────────────────────────

fn handle_event(pane: &str, agent_name: &str, event: AgentEvent) -> i32 {
    match event {
        AgentEvent::SessionStart {
            agent,
            cwd,
            permission_mode,
            source,
            worktree,
            session_id,
            ..
        } => handlers::on_session_start(
            pane,
            &context::make_ctx(&agent, &cwd, &permission_mode, &worktree, &session_id),
            &source,
        ),
        AgentEvent::SessionEnd { end_reason } => {
            let notifications = notification_settings();
            handlers::on_session_end(pane, agent_name, &end_reason, &notifications)
        }
        AgentEvent::UserPromptSubmit {
            agent,
            cwd,
            permission_mode,
            prompt,
            worktree,
            session_id,
            ..
        } => handlers::on_user_prompt_submit(
            pane,
            &context::make_ctx(&agent, &cwd, &permission_mode, &worktree, &session_id),
            &prompt,
        ),
        AgentEvent::Notification {
            agent,
            cwd,
            permission_mode,
            wait_reason,
            meta_only,
            worktree,
            session_id,
            ..
        } => {
            let notifications = notification_settings();
            handlers::on_notification(
                pane,
                &context::make_ctx(&agent, &cwd, &permission_mode, &worktree, &session_id),
                &wait_reason,
                meta_only,
                &notifications,
            )
        }
        AgentEvent::Stop {
            agent,
            cwd,
            permission_mode,
            last_message,
            response,
            worktree,
            session_id,
            ..
        } => {
            let notifications = notification_settings();
            handlers::on_stop(
                pane,
                &context::make_ctx(&agent, &cwd, &permission_mode, &worktree, &session_id),
                &last_message,
                response.as_deref(),
                &notifications,
            )
        }
        AgentEvent::StopFailure {
            agent,
            cwd,
            permission_mode,
            error,
            worktree,
            session_id,
            ..
        } => {
            let notifications = notification_settings();
            handlers::on_stop_failure(
                pane,
                &context::make_ctx(&agent, &cwd, &permission_mode, &worktree, &session_id),
                &error,
                &notifications,
            )
        }
        AgentEvent::SubagentStart {
            agent_type,
            agent_id,
            session_id,
        } => {
            append_codex_lifecycle(
                pane,
                agent_name,
                session_id.as_deref(),
                agent_id.as_deref(),
                &agent_type,
                crate::codex_agents::CodexAgentStatus::Working,
            );
            handlers::on_subagent_start(pane, &agent_type, agent_id.as_deref())
        }
        AgentEvent::SubagentStop {
            agent_type,
            agent_id,
            session_id,
            ..
        } => {
            append_codex_lifecycle(
                pane,
                agent_name,
                session_id.as_deref(),
                agent_id.as_deref(),
                &agent_type,
                crate::codex_agents::CodexAgentStatus::Done,
            );
            handlers::on_subagent_stop(pane, agent_id.as_deref())
        }
        AgentEvent::ActivityLog {
            tool_name,
            tool_input,
            tool_response,
        } => activity::handle_activity_log(pane, &tool_name, &tool_input, &tool_response),
        AgentEvent::PermissionDenied {
            agent,
            cwd,
            permission_mode,
            worktree,
            session_id,
            ..
        } => {
            let notifications = notification_settings();
            handlers::on_permission_denied(
                pane,
                &context::make_ctx(&agent, &cwd, &permission_mode, &worktree, &session_id),
                &notifications,
            )
        }
        AgentEvent::CwdChanged {
            cwd,
            worktree,
            session_id,
            ..
        } => {
            sync_pane_location(pane, &cwd, &worktree, &session_id);
            0
        }
        AgentEvent::TaskCreated { .. } => 0,
        AgentEvent::TaskCompleted {
            task_id,
            task_subject,
        } => {
            let notifications = notification_settings();
            handlers::on_task_completed(pane, agent_name, &task_id, &task_subject, &notifications)
        }
        AgentEvent::TeammateIdle {
            teammate_name,
            idle_reason,
            ..
        } => handlers::on_teammate_idle(pane, &teammate_name, &idle_reason),
        AgentEvent::WorktreeCreate => 0,
        AgentEvent::WorktreeRemove { .. } => handlers::on_worktree_remove(pane),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::codex_agents::CodexAgentStatus;
    use crate::codex_agents::journal::{JournalTracker, journal_file_path, remove_journal};
    use crate::event::AgentEventKind;
    use crate::tmux;

    fn journal_status(
        pane: &str,
        parent_session_id: &str,
        agent_id: &str,
    ) -> Option<CodexAgentStatus> {
        let mut tracker = JournalTracker::default();
        tracker.set_context(pane, Some(parent_session_id));
        tracker.refresh().expect("refresh lifecycle journal");
        tracker
            .snapshots()
            .get(agent_id)
            .map(|snapshot| snapshot.status)
    }

    #[test]
    fn subagent_transcript_path_keeps_parent_value() {
        let _guard = tmux::test_mock::install();
        let pane = "%SUBAGENT_TRANSCRIPT";
        tmux::test_mock::set(pane, tmux::PANE_TRANSCRIPT_PATH, "/tmp/parent.jsonl");

        sync_event_transcript_path(
            pane,
            AgentEventKind::SubagentStart,
            Some("/tmp/child.jsonl"),
        );
        assert_eq!(
            tmux::test_mock::get(pane, tmux::PANE_TRANSCRIPT_PATH).as_deref(),
            Some("/tmp/parent.jsonl")
        );

        sync_event_transcript_path(pane, AgentEventKind::SubagentStop, Some("/tmp/child.jsonl"));
        assert_eq!(
            tmux::test_mock::get(pane, tmux::PANE_TRANSCRIPT_PATH).as_deref(),
            Some("/tmp/parent.jsonl")
        );
    }

    #[test]
    fn session_start_transcript_path_sets_and_clears_parent_value() {
        let _guard = tmux::test_mock::install();
        let pane = "%SESSION_TRANSCRIPT";

        sync_event_transcript_path(
            pane,
            AgentEventKind::SessionStart,
            Some("/tmp/parent.jsonl"),
        );
        assert_eq!(
            tmux::test_mock::get(pane, tmux::PANE_TRANSCRIPT_PATH).as_deref(),
            Some("/tmp/parent.jsonl")
        );

        sync_event_transcript_path(pane, AgentEventKind::SessionStart, None);
        assert!(!tmux::test_mock::contains(pane, tmux::PANE_TRANSCRIPT_PATH));
    }

    #[test]
    fn task_completed_does_not_write_pane_attention() {
        let _guard = tmux::test_mock::install();
        let pane = "%TASK_COMPLETED_ONLY";

        let exit = handle_event(
            pane,
            "claude",
            AgentEvent::TaskCompleted {
                task_id: "task-1".into(),
                task_subject: "finish tests".into(),
            },
        );

        assert_eq!(exit, 0);
        assert!(!tmux::test_mock::contains(pane, tmux::PANE_ATTENTION));
    }

    #[test]
    fn codex_subagent_start_and_stop_write_lifecycle_journal() {
        let _guard = tmux::test_mock::install();
        let pane = "%CODEX_SUBAGENT_LIFECYCLE";
        remove_journal(pane);

        assert_eq!(
            handle_event(
                pane,
                tmux::CODEX_AGENT,
                AgentEvent::SubagentStart {
                    agent_type: "worker".into(),
                    agent_id: Some("agent-a".into()),
                    session_id: Some("parent-1".into()),
                },
            ),
            0
        );
        assert_eq!(
            journal_status(pane, "parent-1", "agent-a"),
            Some(CodexAgentStatus::Working)
        );

        assert_eq!(
            handle_event(
                pane,
                tmux::CODEX_AGENT,
                AgentEvent::SubagentStop {
                    agent_type: "worker".into(),
                    agent_id: Some("agent-a".into()),
                    session_id: Some("parent-1".into()),
                    last_message: String::new(),
                    transcript_path: String::new(),
                },
            ),
            0
        );
        assert_eq!(
            journal_status(pane, "parent-1", "agent-a"),
            Some(CodexAgentStatus::Done)
        );

        remove_journal(pane);
    }

    #[test]
    fn codex_subagent_claude_event_does_not_write_lifecycle_journal() {
        let _guard = tmux::test_mock::install();
        let pane = "%CLAUDE_NO_CODEX_JOURNAL";
        remove_journal(pane);

        handle_event(
            pane,
            tmux::CLAUDE_AGENT,
            AgentEvent::SubagentStart {
                agent_type: "worker".into(),
                agent_id: Some("agent-a".into()),
                session_id: Some("parent-1".into()),
            },
        );

        assert!(!journal_file_path(pane).exists());
        remove_journal(pane);
    }

    #[test]
    fn codex_subagent_missing_session_id_uses_stored_parent_session() {
        let _guard = tmux::test_mock::install();
        let pane = "%CODEX_SUBAGENT_STORED_PARENT";
        remove_journal(pane);
        tmux::test_mock::set(pane, tmux::PANE_SESSION_ID, "parent-stored");

        handle_event(
            pane,
            tmux::CODEX_AGENT,
            AgentEvent::SubagentStart {
                agent_type: "worker".into(),
                agent_id: Some("agent-a".into()),
                session_id: None,
            },
        );

        assert_eq!(
            journal_status(pane, "parent-stored", "agent-a"),
            Some(CodexAgentStatus::Working)
        );
        remove_journal(pane);
    }

    #[test]
    fn codex_subagent_missing_or_empty_agent_id_writes_nothing() {
        let _guard = tmux::test_mock::install();
        let pane = "%CODEX_SUBAGENT_NO_ID";
        remove_journal(pane);

        for agent_id in [None, Some(String::new())] {
            handle_event(
                pane,
                tmux::CODEX_AGENT,
                AgentEvent::SubagentStart {
                    agent_type: "worker".into(),
                    agent_id,
                    session_id: Some("parent-1".into()),
                },
            );
        }

        assert!(!journal_file_path(pane).exists());
        remove_journal(pane);
    }
}
