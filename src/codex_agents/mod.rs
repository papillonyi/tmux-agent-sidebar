pub mod journal;
pub mod transcript;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CodexAgentStatus {
    Working,
    Done,
    Interrupted,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CodexAgentInfo {
    pub id: String,
    pub path: String,
    pub fallback_agent_type: String,
    pub status: CodexAgentStatus,
    pub started_at: Option<u64>,
    pub finished_at: Option<u64>,
}

#[derive(Debug, Clone, Default)]
pub(crate) struct CodexAgentTracker {
    transcript: transcript::TranscriptTracker,
    journal: journal::JournalTracker,
    agents: Vec<CodexAgentInfo>,
}

impl CodexAgentTracker {
    pub(crate) fn refresh(
        &mut self,
        pane_id: &str,
        parent_session_id: Option<&str>,
        transcript_path: Option<&str>,
    ) {
        if parent_session_id.is_none() {
            self.transcript
                .set_context(transcript_path, parent_session_id);
            self.journal.set_context(pane_id, parent_session_id);
            self.agents.clear();
            return;
        }

        if self.journal.set_context(pane_id, parent_session_id) {
            let _ = self.journal.refresh();
        } else {
            let mut journal = self.journal.clone();
            if journal.refresh().is_ok() {
                self.journal = journal;
            }
        }

        let journal_ids = self.journal.snapshots().keys().cloned().collect::<Vec<_>>();
        if self
            .transcript
            .set_context(transcript_path, parent_session_id)
        {
            let _ = self.transcript.refresh_with_child_ids(&journal_ids);
        } else {
            let mut transcript = self.transcript.clone();
            if transcript.refresh_with_child_ids(&journal_ids).is_ok() {
                self.transcript = transcript;
            }
        }

        self.rebuild_agents();
    }

    pub(crate) fn agents(&self) -> &[CodexAgentInfo] {
        &self.agents
    }

    fn rebuild_agents(&mut self) {
        self.agents.clear();

        for catalog in self.transcript.agents().values() {
            if self.transcript.closed_ids().contains(&catalog.id) {
                continue;
            }
            self.agents.push(Self::merge_agent(
                &catalog.id,
                Some(catalog),
                self.journal.snapshots().get(&catalog.id),
                self.transcript.child_lifecycles().get(&catalog.id),
            ));
        }

        for snapshot in self.journal.snapshots().values() {
            if self.transcript.agents().contains_key(&snapshot.agent_id)
                || self.transcript.closed_ids().contains(&snapshot.agent_id)
            {
                continue;
            }
            self.agents.push(Self::merge_agent(
                &snapshot.agent_id,
                None,
                Some(snapshot),
                self.transcript.child_lifecycles().get(&snapshot.agent_id),
            ));
        }
    }

    fn merge_agent(
        id: &str,
        catalog: Option<&transcript::CatalogAgent>,
        journal: Option<&journal::LifecycleSnapshot>,
        child: Option<&transcript::ChildLifecycleSnapshot>,
    ) -> CodexAgentInfo {
        let mut latest = catalog
            .and_then(|agent| agent.interrupted_at_ms)
            .map(|timestamp| (CodexAgentStatus::Interrupted, timestamp, None, None));
        if let Some(snapshot) = journal
            && latest
                .as_ref()
                .is_none_or(|(_, timestamp, _, _)| snapshot.last_event_at_ms >= *timestamp)
        {
            latest = Some((
                snapshot.status,
                snapshot.last_event_at_ms,
                snapshot.started_at_ms,
                snapshot.finished_at_ms,
            ));
        }
        if let Some(snapshot) = child
            && latest
                .as_ref()
                .is_none_or(|(_, timestamp, _, _)| snapshot.last_event_at_ms >= *timestamp)
        {
            latest = Some((
                snapshot.status,
                snapshot.last_event_at_ms,
                snapshot.started_at_ms,
                snapshot.finished_at_ms,
            ));
        }
        let (status, started_at_ms, finished_at_ms) = latest
            .map(|(status, _, started_at_ms, finished_at_ms)| {
                let started_at_ms =
                    started_at_ms.or_else(|| journal.and_then(|snapshot| snapshot.started_at_ms));
                let finished_at_ms = match status {
                    CodexAgentStatus::Working | CodexAgentStatus::Unknown => None,
                    CodexAgentStatus::Done | CodexAgentStatus::Interrupted => finished_at_ms
                        .or_else(|| journal.and_then(|snapshot| snapshot.finished_at_ms)),
                };
                (status, started_at_ms, finished_at_ms)
            })
            .unwrap_or((CodexAgentStatus::Unknown, None, None));

        CodexAgentInfo {
            id: id.to_owned(),
            path: catalog
                .map(|agent| agent.path.clone())
                .filter(|path| !path.is_empty())
                .unwrap_or_default(),
            fallback_agent_type: journal
                .map(|snapshot| snapshot.agent_type.clone())
                .unwrap_or_default(),
            status,
            started_at: started_at_ms.map(|timestamp| timestamp / 1_000),
            finished_at: finished_at_ms.map(|timestamp| timestamp / 1_000),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::fs::{self, File};
    use std::io::{self, Write};
    use std::path::Path;

    use serde_json::{Value, json};

    use super::{CodexAgentInfo, CodexAgentStatus, CodexAgentTracker};
    use crate::codex_agents::journal::{append_lifecycle_event, journal_file_path, remove_journal};

    fn unique_pane(test_name: &str) -> String {
        format!("%MERGE_{test_name}_{}", std::process::id())
    }

    fn activity(kind: &str, id: &str, path: &str, occurred_at_ms: u64) -> Value {
        json!({
            "type": "event_msg",
            "payload": {
                "type": "sub_agent_activity",
                "occurred_at_ms": occurred_at_ms,
                "agent_thread_id": id,
                "agent_path": path,
                "kind": kind
            }
        })
    }

    fn close_call(target: &str, call_id: &str) -> Value {
        json!({
            "type": "response_item",
            "payload": {
                "type": "function_call",
                "name": "close_agent",
                "arguments": json!({ "target": target }).to_string(),
                "call_id": call_id
            }
        })
    }

    fn close_output(call_id: &str) -> Value {
        json!({
            "type": "response_item",
            "payload": {
                "type": "function_call_output",
                "call_id": call_id,
                "output": r#"{"previous_status":"completed"}"#
            }
        })
    }

    fn write_lines(path: &Path, lines: &[Value]) -> io::Result<()> {
        let mut file = File::create(path)?;
        for line in lines {
            writeln!(file, "{line}")?;
        }
        Ok(())
    }

    fn rollout_path(
        root: &Path,
        date: (&str, &str, &str),
        timestamp: &str,
        id: &str,
    ) -> io::Result<std::path::PathBuf> {
        let dir = root.join("sessions").join(date.0).join(date.1).join(date.2);
        fs::create_dir_all(&dir)?;
        Ok(dir.join(format!("rollout-{timestamp}-{id}.jsonl")))
    }

    fn child_session_meta(id: &str, parent_session_id: &str) -> Value {
        json!({
            "type": "session_meta",
            "payload": {
                "id": id,
                "session_id": parent_session_id,
                "parent_thread_id": parent_session_id,
                "source": {
                    "subagent": {
                        "thread_spawn": {
                            "parent_thread_id": parent_session_id,
                            "depth": 1
                        }
                    }
                },
                "thread_source": "subagent"
            }
        })
    }

    fn child_task_started(started_at: u64) -> Value {
        json!({
            "type": "event_msg",
            "payload": {
                "type": "task_started",
                "turn_id": "turn-1",
                "started_at": started_at
            }
        })
    }

    fn child_task_complete(started_at: u64, completed_at: u64) -> Value {
        json!({
            "type": "event_msg",
            "payload": {
                "type": "task_complete",
                "turn_id": "turn-1",
                "started_at": started_at,
                "completed_at": completed_at
            }
        })
    }

    fn child_turn_interrupted(started_at: u64, completed_at: u64) -> Value {
        json!({
            "type": "event_msg",
            "payload": {
                "type": "turn_aborted",
                "turn_id": "turn-1",
                "reason": "interrupted",
                "started_at": started_at,
                "completed_at": completed_at
            }
        })
    }

    fn write_child_rollout(
        path: &Path,
        id: &str,
        parent_session_id: &str,
        events: &[Value],
    ) -> io::Result<()> {
        let mut lines = vec![child_session_meta(id, parent_session_id)];
        lines.extend_from_slice(events);
        write_lines(path, &lines)
    }

    #[test]
    fn merge_preserves_catalog_order_and_enriches_lifecycle() -> io::Result<()> {
        let dir = tempfile::tempdir()?;
        let transcript = dir.path().join("rollout.jsonl");
        let pane = unique_pane("order_and_lifecycle");
        remove_journal(&pane);
        write_lines(
            &transcript,
            &[
                activity("started", "agent-a", "/root/task1_review", 1_000),
                activity("started", "agent-b", "/root/task2_owner", 2_000),
            ],
        )?;
        for (id, agent_type, status, occurred_at_ms) in [
            ("agent-c", "journal-c", CodexAgentStatus::Working, 12_345),
            ("agent-b", "hook-owner", CodexAgentStatus::Working, 20_999),
            ("agent-a", "hook-review", CodexAgentStatus::Working, 10_999),
            ("agent-a", "hook-review", CodexAgentStatus::Done, 25_999),
        ] {
            append_lifecycle_event(&pane, "parent-1", id, agent_type, status, occurred_at_ms)?;
        }

        let mut tracker = CodexAgentTracker::default();
        tracker.refresh(&pane, Some("parent-1"), transcript.to_str());

        assert_eq!(
            tracker
                .agents()
                .iter()
                .map(|agent| agent.id.as_str())
                .collect::<Vec<_>>(),
            vec!["agent-a", "agent-b", "agent-c"],
        );
        let agent_a = &tracker.agents()[0];
        assert_eq!(agent_a.path, "/root/task1_review");
        assert_eq!(agent_a.fallback_agent_type, "hook-review");
        assert_eq!(agent_a.status, CodexAgentStatus::Done);
        assert_eq!(agent_a.started_at, Some(10));
        assert_eq!(agent_a.finished_at, Some(25));

        let agent_b = &tracker.agents()[1];
        assert_eq!(agent_b.path, "/root/task2_owner");
        assert_eq!(agent_b.status, CodexAgentStatus::Working);
        assert_eq!(agent_b.started_at, Some(20));
        assert_eq!(agent_b.finished_at, None);

        let agent_c = &tracker.agents()[2];
        assert!(agent_c.path.is_empty());
        assert_eq!(agent_c.fallback_agent_type, "journal-c");
        assert_eq!(agent_c.status, CodexAgentStatus::Working);
        assert_eq!(agent_c.started_at, Some(12));

        remove_journal(&pane);
        Ok(())
    }

    #[test]
    fn merge_catalog_only_is_unknown_and_empty_catalog_keeps_fallback_rows() -> io::Result<()> {
        let dir = tempfile::tempdir()?;
        let transcript = dir.path().join("rollout.jsonl");
        let pane = unique_pane("catalog_and_fallback");
        remove_journal(&pane);
        write_lines(
            &transcript,
            &[activity("started", "catalog-only", "/root/catalog", 1_000)],
        )?;
        append_lifecycle_event(
            &pane,
            "parent-1",
            "journal-only",
            "worker",
            CodexAgentStatus::Working,
            9_999,
        )?;

        let mut tracker = CodexAgentTracker::default();
        tracker.refresh(&pane, Some("parent-1"), transcript.to_str());
        assert_eq!(
            tracker.agents()[0].status,
            CodexAgentStatus::Unknown,
            "catalog metadata alone must not claim a lifecycle state",
        );

        fs::write(&transcript, "")?;
        tracker.refresh(&pane, Some("parent-1"), transcript.to_str());
        assert_eq!(tracker.agents().len(), 1);
        assert_eq!(tracker.agents()[0].id, "journal-only");
        assert!(tracker.agents()[0].path.is_empty());
        assert_eq!(tracker.agents()[0].fallback_agent_type, "worker");

        remove_journal(&pane);
        Ok(())
    }

    #[test]
    fn merge_newer_interruption_wins_but_later_resume_restores_working() -> io::Result<()> {
        let dir = tempfile::tempdir()?;
        let transcript = dir.path().join("rollout.jsonl");
        let pane = unique_pane("interruption_precedence");
        remove_journal(&pane);
        write_lines(
            &transcript,
            &[
                activity("started", "agent-a", "/root/task-a", 1_000),
                activity("interrupted", "agent-a", "/root/task-a", 30_000),
                activity("started", "agent-b", "/root/task-b", 2_000),
                activity("interrupted", "agent-b", "/root/task-b", 30_000),
            ],
        )?;
        for (id, status, occurred_at_ms) in [
            ("agent-a", CodexAgentStatus::Working, 10_000),
            ("agent-a", CodexAgentStatus::Done, 25_000),
            ("agent-b", CodexAgentStatus::Working, 40_000),
        ] {
            append_lifecycle_event(&pane, "parent-1", id, "worker", status, occurred_at_ms)?;
        }

        let mut tracker = CodexAgentTracker::default();
        tracker.refresh(&pane, Some("parent-1"), transcript.to_str());

        assert_eq!(tracker.agents()[0].status, CodexAgentStatus::Interrupted);
        assert_eq!(tracker.agents()[0].started_at, Some(10));
        assert_eq!(tracker.agents()[0].finished_at, Some(25));
        assert_eq!(tracker.agents()[1].status, CodexAgentStatus::Working);
        assert_eq!(tracker.agents()[1].started_at, Some(40));
        assert_eq!(tracker.agents()[1].finished_at, None);

        remove_journal(&pane);
        Ok(())
    }

    #[test]
    fn merge_omits_every_confirmed_closed_id() -> io::Result<()> {
        let dir = tempfile::tempdir()?;
        let transcript = dir.path().join("rollout.jsonl");
        let pane = unique_pane("closed_ids");
        remove_journal(&pane);
        write_lines(
            &transcript,
            &[
                activity("started", "agent-a", "/root/task-a", 1_000),
                activity("started", "agent-b", "/root/task-b", 2_000),
                close_call("agent-a", "close-a"),
                close_output("close-a"),
                close_call("/root/task-b", "close-b"),
                close_output("close-b"),
            ],
        )?;
        for id in ["agent-a", "agent-b"] {
            append_lifecycle_event(
                &pane,
                "parent-1",
                id,
                "worker",
                CodexAgentStatus::Working,
                10_000,
            )?;
        }

        let mut tracker = CodexAgentTracker::default();
        tracker.refresh(&pane, Some("parent-1"), transcript.to_str());

        assert!(tracker.agents().is_empty());

        remove_journal(&pane);
        Ok(())
    }

    #[test]
    fn merge_missing_parent_clears_agents() -> io::Result<()> {
        let dir = tempfile::tempdir()?;
        let transcript = dir.path().join("rollout.jsonl");
        let pane = unique_pane("missing_parent");
        remove_journal(&pane);
        write_lines(
            &transcript,
            &[activity("started", "agent-a", "/root/task-a", 1_000)],
        )?;

        let mut tracker = CodexAgentTracker::default();
        tracker.refresh(&pane, Some("parent-1"), transcript.to_str());
        assert_eq!(tracker.agents().len(), 1);

        tracker.refresh(&pane, None, transcript.to_str());
        assert!(tracker.agents().is_empty());

        remove_journal(&pane);
        Ok(())
    }

    #[test]
    fn merge_transcript_error_does_not_block_valid_journal_refresh() -> io::Result<()> {
        let dir = tempfile::tempdir()?;
        let pane = unique_pane("source_independence");
        remove_journal(&pane);
        append_lifecycle_event(
            &pane,
            "parent-1",
            "agent-a",
            "worker",
            CodexAgentStatus::Working,
            10_000,
        )?;

        let mut tracker = CodexAgentTracker::default();
        tracker.refresh(&pane, Some("parent-1"), dir.path().to_str());

        assert_eq!(tracker.agents().len(), 1);
        assert_eq!(tracker.agents()[0].id, "agent-a");
        assert_eq!(tracker.agents()[0].status, CodexAgentStatus::Working);

        assert!(journal_file_path(&pane).exists());
        remove_journal(&pane);
        Ok(())
    }

    #[test]
    fn merge_context_change_does_not_expose_previous_cache_when_new_read_fails() -> io::Result<()> {
        let dir = tempfile::tempdir()?;
        let transcript = dir.path().join("old-rollout.jsonl");
        let old_pane = unique_pane("old_context");
        let new_pane = unique_pane("new_context");
        remove_journal(&old_pane);
        remove_journal(&new_pane);
        write_lines(
            &transcript,
            &[activity(
                "started",
                "agent-from-old-context",
                "/root/old-task",
                1_000,
            )],
        )?;

        let mut tracker = CodexAgentTracker::default();
        tracker.refresh(&old_pane, Some("parent-old"), transcript.to_str());
        assert_eq!(tracker.agents().len(), 1);

        tracker.refresh(&new_pane, Some("parent-new"), dir.path().to_str());

        assert!(
            tracker.agents().is_empty(),
            "a failed first read in a new context must not expose agents cached for the old context",
        );

        remove_journal(&old_pane);
        remove_journal(&new_pane);
        Ok(())
    }

    #[test]
    fn merge_parent_change_clears_journal_only_cache() -> io::Result<()> {
        let pane = unique_pane("journal_parent_change");
        remove_journal(&pane);
        append_lifecycle_event(
            &pane,
            "parent-old",
            "agent-from-old-parent",
            "worker",
            CodexAgentStatus::Working,
            10_000,
        )?;

        let mut tracker = CodexAgentTracker::default();
        tracker.refresh(&pane, Some("parent-old"), None);
        assert_eq!(tracker.agents().len(), 1);

        tracker.refresh(&pane, Some("parent-new"), None);

        assert!(
            tracker.agents().is_empty(),
            "journal-only rows from the previous parent session must not survive a context change",
        );

        remove_journal(&pane);
        Ok(())
    }

    #[test]
    fn merge_same_context_read_errors_preserve_last_valid_source_caches() -> io::Result<()> {
        let dir = tempfile::tempdir()?;
        let transcript = dir.path().join("rollout.jsonl");
        let pane = unique_pane("same_context_errors");
        remove_journal(&pane);
        write_lines(
            &transcript,
            &[activity(
                "started",
                "agent-from-valid-cache",
                "/root/stable-task",
                1_000,
            )],
        )?;
        append_lifecycle_event(
            &pane,
            "parent-1",
            "agent-from-valid-cache",
            "worker",
            CodexAgentStatus::Working,
            10_999,
        )?;

        let mut tracker = CodexAgentTracker::default();
        tracker.refresh(&pane, Some("parent-1"), transcript.to_str());
        let expected = [CodexAgentInfo {
            id: "agent-from-valid-cache".into(),
            path: "/root/stable-task".into(),
            fallback_agent_type: "worker".into(),
            status: CodexAgentStatus::Working,
            started_at: Some(10),
            finished_at: None,
        }];
        assert_eq!(tracker.agents(), &expected);

        fs::remove_file(&transcript)?;
        fs::create_dir(&transcript)?;
        let journal = journal_file_path(&pane);
        fs::remove_file(&journal)?;
        fs::create_dir(&journal)?;

        tracker.refresh(&pane, Some("parent-1"), transcript.to_str());

        assert_eq!(
            tracker.agents(),
            &expected,
            "transient read errors in an unchanged context must preserve both source caches",
        );

        fs::remove_dir(&transcript)?;
        fs::remove_dir(&journal)?;
        Ok(())
    }

    #[test]
    fn child_terminal_rollouts_override_stale_working_journal() -> io::Result<()> {
        let dir = tempfile::tempdir()?;
        let parent_id = "019f9300-0000-7000-8000-000000000001";
        let complete_id = "019f9300-0000-7000-8000-000000000002";
        let interrupted_id = "019f9300-0000-7000-8000-000000000003";
        let parent = rollout_path(
            dir.path(),
            ("2026", "07", "24"),
            "2026-07-24T15-00-00",
            parent_id,
        )?;
        write_lines(
            &parent,
            &[
                activity("started", complete_id, "/root/complete", 10_000),
                activity("started", interrupted_id, "/root/interrupted", 11_000),
            ],
        )?;
        let complete = rollout_path(
            dir.path(),
            ("2026", "07", "24"),
            "2026-07-24T15-00-01",
            complete_id,
        )?;
        write_child_rollout(
            &complete,
            complete_id,
            parent_id,
            &[child_task_started(10), child_task_complete(10, 30)],
        )?;
        let interrupted = rollout_path(
            dir.path(),
            ("2026", "07", "25"),
            "2026-07-25T00-00-01",
            interrupted_id,
        )?;
        write_child_rollout(
            &interrupted,
            interrupted_id,
            parent_id,
            &[child_task_started(11), child_turn_interrupted(11, 40)],
        )?;
        let pane = unique_pane("child_terminal_override");
        remove_journal(&pane);
        for id in [complete_id, interrupted_id] {
            append_lifecycle_event(
                &pane,
                parent_id,
                id,
                "worker",
                CodexAgentStatus::Working,
                20_000,
            )?;
        }

        let mut tracker = CodexAgentTracker::default();
        tracker.refresh(&pane, Some(parent_id), parent.to_str());

        assert_eq!(tracker.agents()[0].status, CodexAgentStatus::Done);
        assert_eq!(tracker.agents()[0].started_at, Some(10));
        assert_eq!(tracker.agents()[0].finished_at, Some(30));
        assert_eq!(tracker.agents()[1].status, CodexAgentStatus::Interrupted);
        assert_eq!(tracker.agents()[1].started_at, Some(11));
        assert_eq!(tracker.agents()[1].finished_at, Some(40));

        remove_journal(&pane);
        Ok(())
    }

    #[test]
    fn journal_only_children_use_child_rollout_terminal_lifecycle() -> io::Result<()> {
        let dir = tempfile::tempdir()?;
        let parent_id = "019f930b-0000-7000-8000-000000000001";
        let complete_id = "019f930b-0000-7000-8000-000000000002";
        let interrupted_id = "019f930b-0000-7000-8000-000000000003";
        let parent = rollout_path(
            dir.path(),
            ("2026", "07", "24"),
            "2026-07-24T15-11-00",
            parent_id,
        )?;
        write_lines(&parent, &[])?;
        let complete = rollout_path(
            dir.path(),
            ("2026", "07", "24"),
            "2026-07-24T15-11-01",
            complete_id,
        )?;
        write_child_rollout(
            &complete,
            complete_id,
            parent_id,
            &[child_task_started(10), child_task_complete(10, 30)],
        )?;
        let interrupted = rollout_path(
            dir.path(),
            ("2026", "07", "24"),
            "2026-07-24T15-11-02",
            interrupted_id,
        )?;
        write_child_rollout(
            &interrupted,
            interrupted_id,
            parent_id,
            &[child_task_started(11), child_turn_interrupted(11, 40)],
        )?;
        let pane = unique_pane("journal_only_child_terminal");
        remove_journal(&pane);
        for id in [complete_id, interrupted_id] {
            append_lifecycle_event(
                &pane,
                parent_id,
                id,
                "worker",
                CodexAgentStatus::Working,
                20_000,
            )?;
        }

        let mut tracker = CodexAgentTracker::default();
        tracker.refresh(&pane, Some(parent_id), parent.to_str());

        assert_eq!(
            tracker.agents(),
            &[
                CodexAgentInfo {
                    id: complete_id.into(),
                    path: String::new(),
                    fallback_agent_type: "worker".into(),
                    status: CodexAgentStatus::Done,
                    started_at: Some(10),
                    finished_at: Some(30),
                },
                CodexAgentInfo {
                    id: interrupted_id.into(),
                    path: String::new(),
                    fallback_agent_type: "worker".into(),
                    status: CodexAgentStatus::Interrupted,
                    started_at: Some(11),
                    finished_at: Some(40),
                },
            ],
        );
        remove_journal(&pane);
        Ok(())
    }

    #[test]
    fn closed_journal_child_is_not_rediscovered_or_resurrected() -> io::Result<()> {
        let dir = tempfile::tempdir()?;
        let parent_id = "019f930c-0000-7000-8000-000000000001";
        let child_id = "019f930c-0000-7000-8000-000000000002";
        let parent = rollout_path(
            dir.path(),
            ("2026", "07", "24"),
            "2026-07-24T15-12-00",
            parent_id,
        )?;
        write_lines(
            &parent,
            &[activity(
                "started",
                child_id,
                "/root/closed-journal-child",
                10_000,
            )],
        )?;
        let child = rollout_path(
            dir.path(),
            ("2026", "07", "24"),
            "2026-07-24T15-12-01",
            child_id,
        )?;
        write_child_rollout(
            &child,
            child_id,
            parent_id,
            &[child_task_started(10), child_task_complete(10, 30)],
        )?;
        let pane = unique_pane("closed_journal_child");
        remove_journal(&pane);
        append_lifecycle_event(
            &pane,
            parent_id,
            child_id,
            "worker",
            CodexAgentStatus::Working,
            20_000,
        )?;
        let mut tracker = CodexAgentTracker::default();
        tracker.refresh(&pane, Some(parent_id), parent.to_str());
        assert_eq!(tracker.agents()[0].status, CodexAgentStatus::Done);

        let mut file = fs::OpenOptions::new().append(true).open(&parent)?;
        writeln!(file, "{}", close_call(child_id, "close-journal-child"))?;
        writeln!(file, "{}", close_output("close-journal-child"))?;
        tracker.refresh(&pane, Some(parent_id), parent.to_str());
        assert!(tracker.agents().is_empty());

        tracker.refresh(&pane, Some(parent_id), parent.to_str());
        assert!(
            tracker.agents().is_empty(),
            "retained journal and child rollout must not resurrect a confirmed closed ID",
        );
        remove_journal(&pane);
        Ok(())
    }

    #[test]
    fn inherited_parent_meta_does_not_clear_valid_child_identity() -> io::Result<()> {
        let dir = tempfile::tempdir()?;
        let parent_id = "019f930a-0000-7000-8000-000000000001";
        let child_id = "019f930a-0000-7000-8000-000000000002";
        let parent = rollout_path(
            dir.path(),
            ("2026", "07", "24"),
            "2026-07-24T15-10-00",
            parent_id,
        )?;
        write_lines(
            &parent,
            &[activity(
                "started",
                child_id,
                "/root/inherited-parent-meta",
                10_000,
            )],
        )?;
        let child = rollout_path(
            dir.path(),
            ("2026", "07", "24"),
            "2026-07-24T15-10-01",
            child_id,
        )?;
        write_lines(
            &child,
            &[
                child_session_meta(child_id, parent_id),
                json!({
                    "type": "session_meta",
                    "payload": {
                        "id": parent_id,
                        "session_id": parent_id,
                        "source": "cli",
                        "thread_source": "cli"
                    }
                }),
                child_task_started(10),
                child_turn_interrupted(10, 30),
            ],
        )?;
        let pane = unique_pane("child_inherited_parent_meta");
        remove_journal(&pane);
        append_lifecycle_event(
            &pane,
            parent_id,
            child_id,
            "worker",
            CodexAgentStatus::Working,
            20_000,
        )?;

        let mut tracker = CodexAgentTracker::default();
        tracker.refresh(&pane, Some(parent_id), parent.to_str());

        assert_eq!(
            tracker.agents(),
            &[CodexAgentInfo {
                id: child_id.into(),
                path: "/root/inherited-parent-meta".into(),
                fallback_agent_type: "worker".into(),
                status: CodexAgentStatus::Interrupted,
                started_at: Some(10),
                finished_at: Some(30),
            }],
        );
        remove_journal(&pane);
        Ok(())
    }

    #[test]
    fn child_rollout_rejects_identity_parent_and_source_mismatches() -> io::Result<()> {
        let dir = tempfile::tempdir()?;
        let parent_id = "019f9301-0000-7000-8000-000000000001";
        let ids = [
            "019f9301-0000-7000-8000-000000000002",
            "019f9301-0000-7000-8000-000000000003",
            "019f9301-0000-7000-8000-000000000004",
        ];
        let parent = rollout_path(
            dir.path(),
            ("2026", "07", "24"),
            "2026-07-24T15-01-00",
            parent_id,
        )?;
        write_lines(
            &parent,
            &ids.iter()
                .enumerate()
                .map(|(index, id)| {
                    activity(
                        "started",
                        id,
                        &format!("/root/mismatch-{index}"),
                        10_000 + index as u64,
                    )
                })
                .collect::<Vec<_>>(),
        )?;
        let bad_id = rollout_path(
            dir.path(),
            ("2026", "07", "24"),
            "2026-07-24T15-01-01",
            ids[0],
        )?;
        write_child_rollout(
            &bad_id,
            "019f9301-0000-7000-8000-ffffffffffff",
            parent_id,
            &[child_turn_interrupted(10, 30)],
        )?;
        let bad_parent = rollout_path(
            dir.path(),
            ("2026", "07", "24"),
            "2026-07-24T15-01-02",
            ids[1],
        )?;
        write_child_rollout(
            &bad_parent,
            ids[1],
            "019f9301-0000-7000-8000-eeeeeeeeeeee",
            &[child_turn_interrupted(10, 30)],
        )?;
        let bad_source = rollout_path(
            dir.path(),
            ("2026", "07", "24"),
            "2026-07-24T15-01-03",
            ids[2],
        )?;
        write_lines(
            &bad_source,
            &[
                json!({
                    "type": "session_meta",
                    "payload": {
                        "id": ids[2],
                        "session_id": parent_id,
                        "parent_thread_id": parent_id,
                        "source": { "user": {} },
                        "thread_source": "user"
                    }
                }),
                child_turn_interrupted(10, 30),
            ],
        )?;
        let pane = unique_pane("child_identity_mismatch");
        remove_journal(&pane);
        for id in ids {
            append_lifecycle_event(
                &pane,
                parent_id,
                id,
                "worker",
                CodexAgentStatus::Working,
                20_000,
            )?;
        }

        let mut tracker = CodexAgentTracker::default();
        tracker.refresh(&pane, Some(parent_id), parent.to_str());

        assert!(
            tracker
                .agents()
                .iter()
                .all(|agent| agent.status == CodexAgentStatus::Working)
        );
        remove_journal(&pane);
        Ok(())
    }

    #[test]
    fn missing_child_rollout_is_discovered_on_bounded_retry_across_dates() -> io::Result<()> {
        let dir = tempfile::tempdir()?;
        let parent_id = "019f9302-0000-7000-8000-000000000001";
        let child_id = "019f9302-0000-7000-8000-000000000002";
        let parent = rollout_path(
            dir.path(),
            ("2026", "07", "24"),
            "2026-07-24T23-59-59",
            parent_id,
        )?;
        write_lines(
            &parent,
            &[activity(
                "started",
                child_id,
                "/root/across-midnight",
                10_000,
            )],
        )?;
        let pane = unique_pane("child_discovery_retry");
        remove_journal(&pane);
        let mut tracker = CodexAgentTracker::default();
        tracker.refresh(&pane, Some(parent_id), parent.to_str());
        assert_eq!(tracker.agents()[0].status, CodexAgentStatus::Unknown);

        let child = rollout_path(
            dir.path(),
            ("2026", "07", "25"),
            "2026-07-25T00-00-01",
            child_id,
        )?;
        write_child_rollout(&child, child_id, parent_id, &[child_task_started(20)])?;
        tracker.refresh(&pane, Some(parent_id), parent.to_str());

        assert_eq!(tracker.agents()[0].status, CodexAgentStatus::Working);
        assert_eq!(tracker.agents()[0].started_at, Some(20));
        remove_journal(&pane);
        Ok(())
    }

    #[test]
    fn child_rollout_buffers_partial_lines_and_ignores_malformed_records() -> io::Result<()> {
        let dir = tempfile::tempdir()?;
        let parent_id = "019f9303-0000-7000-8000-000000000001";
        let child_id = "019f9303-0000-7000-8000-000000000002";
        let parent = rollout_path(
            dir.path(),
            ("2026", "07", "24"),
            "2026-07-24T15-03-00",
            parent_id,
        )?;
        write_lines(
            &parent,
            &[activity("started", child_id, "/root/partial", 10_000)],
        )?;
        let child = rollout_path(
            dir.path(),
            ("2026", "07", "24"),
            "2026-07-24T15-03-01",
            child_id,
        )?;
        write_child_rollout(&child, child_id, parent_id, &[child_task_started(10)])?;
        let pane = unique_pane("child_partial");
        remove_journal(&pane);
        let mut tracker = CodexAgentTracker::default();
        tracker.refresh(&pane, Some(parent_id), parent.to_str());
        assert_eq!(tracker.agents()[0].status, CodexAgentStatus::Working);

        let interrupted = child_turn_interrupted(10, 30).to_string();
        let split = interrupted.len() / 2;
        let mut file = fs::OpenOptions::new().append(true).open(&child)?;
        file.write_all(b"{ malformed child json\n")?;
        file.write_all(&interrupted.as_bytes()[..split])?;
        tracker.refresh(&pane, Some(parent_id), parent.to_str());
        assert_eq!(tracker.agents()[0].status, CodexAgentStatus::Working);

        let mut file = fs::OpenOptions::new().append(true).open(&child)?;
        file.write_all(&interrupted.as_bytes()[split..])?;
        file.write_all(b"\n")?;
        tracker.refresh(&pane, Some(parent_id), parent.to_str());
        assert_eq!(tracker.agents()[0].status, CodexAgentStatus::Interrupted);
        assert_eq!(tracker.agents()[0].finished_at, Some(30));
        remove_journal(&pane);
        Ok(())
    }

    #[test]
    fn child_rollout_rebuilds_after_rewrite_and_can_resume_after_interrupt() -> io::Result<()> {
        let dir = tempfile::tempdir()?;
        let parent_id = "019f9304-0000-7000-8000-000000000001";
        let child_id = "019f9304-0000-7000-8000-000000000002";
        let parent = rollout_path(
            dir.path(),
            ("2026", "07", "24"),
            "2026-07-24T15-04-00",
            parent_id,
        )?;
        write_lines(
            &parent,
            &[activity("started", child_id, "/root/rewrite", 10_000)],
        )?;
        let child = rollout_path(
            dir.path(),
            ("2026", "07", "24"),
            "2026-07-24T15-04-01",
            child_id,
        )?;
        write_child_rollout(
            &child,
            child_id,
            parent_id,
            &[child_task_started(10), child_task_complete(10, 20)],
        )?;
        let pane = unique_pane("child_rewrite");
        remove_journal(&pane);
        let mut tracker = CodexAgentTracker::default();
        tracker.refresh(&pane, Some(parent_id), parent.to_str());
        assert_eq!(tracker.agents()[0].status, CodexAgentStatus::Done);

        write_child_rollout(
            &child,
            child_id,
            parent_id,
            &[
                child_task_started(30),
                child_turn_interrupted(30, 40),
                child_task_started(50),
            ],
        )?;
        let mut file = fs::OpenOptions::new().append(true).open(&child)?;
        writeln!(file, "{}", "x".repeat(1024))?;
        tracker.refresh(&pane, Some(parent_id), parent.to_str());

        assert_eq!(tracker.agents()[0].status, CodexAgentStatus::Working);
        assert_eq!(tracker.agents()[0].started_at, Some(50));
        assert_eq!(tracker.agents()[0].finished_at, None);
        remove_journal(&pane);
        Ok(())
    }

    #[test]
    fn later_journal_start_wins_over_child_interruption() -> io::Result<()> {
        let dir = tempfile::tempdir()?;
        let parent_id = "019f9305-0000-7000-8000-000000000001";
        let child_id = "019f9305-0000-7000-8000-000000000002";
        let parent = rollout_path(
            dir.path(),
            ("2026", "07", "24"),
            "2026-07-24T15-05-00",
            parent_id,
        )?;
        write_lines(
            &parent,
            &[activity("started", child_id, "/root/later-journal", 10_000)],
        )?;
        let child = rollout_path(
            dir.path(),
            ("2026", "07", "24"),
            "2026-07-24T15-05-01",
            child_id,
        )?;
        write_child_rollout(
            &child,
            child_id,
            parent_id,
            &[child_task_started(10), child_turn_interrupted(10, 20)],
        )?;
        let pane = unique_pane("child_later_journal");
        remove_journal(&pane);
        let mut tracker = CodexAgentTracker::default();
        tracker.refresh(&pane, Some(parent_id), parent.to_str());
        assert_eq!(tracker.agents()[0].status, CodexAgentStatus::Interrupted);

        append_lifecycle_event(
            &pane,
            parent_id,
            child_id,
            "worker",
            CodexAgentStatus::Working,
            30_000,
        )?;

        tracker.refresh(&pane, Some(parent_id), parent.to_str());

        assert_eq!(tracker.agents()[0].status, CodexAgentStatus::Working);
        assert_eq!(tracker.agents()[0].started_at, Some(30));
        assert_eq!(tracker.agents()[0].finished_at, None);
        remove_journal(&pane);
        Ok(())
    }

    #[test]
    fn parent_context_reset_clears_child_lifecycle_cache() -> io::Result<()> {
        let dir = tempfile::tempdir()?;
        let parent_a_id = "019f9306-0000-7000-8000-000000000001";
        let child_a_id = "019f9306-0000-7000-8000-000000000002";
        let parent_a = rollout_path(
            dir.path(),
            ("2026", "07", "24"),
            "2026-07-24T15-06-00",
            parent_a_id,
        )?;
        write_lines(
            &parent_a,
            &[activity("started", child_a_id, "/root/old-child", 10_000)],
        )?;
        let child_a = rollout_path(
            dir.path(),
            ("2026", "07", "24"),
            "2026-07-24T15-06-01",
            child_a_id,
        )?;
        write_child_rollout(
            &child_a,
            child_a_id,
            parent_a_id,
            &[child_task_started(10), child_turn_interrupted(10, 20)],
        )?;
        let pane = unique_pane("child_context_reset");
        remove_journal(&pane);
        let mut tracker = CodexAgentTracker::default();
        tracker.refresh(&pane, Some(parent_a_id), parent_a.to_str());
        assert_eq!(tracker.agents()[0].status, CodexAgentStatus::Interrupted);

        let parent_b_id = "019f9306-0000-7000-8000-000000000003";
        let child_b_id = "019f9306-0000-7000-8000-000000000004";
        let parent_b = rollout_path(
            dir.path(),
            ("2026", "07", "25"),
            "2026-07-25T15-06-00",
            parent_b_id,
        )?;
        write_lines(
            &parent_b,
            &[activity("started", child_b_id, "/root/new-child", 30_000)],
        )?;
        tracker.refresh(&pane, Some(parent_b_id), parent_b.to_str());

        assert_eq!(tracker.agents().len(), 1);
        assert_eq!(tracker.agents()[0].id, child_b_id);
        assert_eq!(tracker.agents()[0].status, CodexAgentStatus::Unknown);
        remove_journal(&pane);
        Ok(())
    }

    #[test]
    fn close_prunes_child_lifecycle_before_same_context_rediscovery() -> io::Result<()> {
        let dir = tempfile::tempdir()?;
        let parent_id = "019f9307-0000-7000-8000-000000000001";
        let child_id = "019f9307-0000-7000-8000-000000000002";
        let parent = rollout_path(
            dir.path(),
            ("2026", "07", "24"),
            "2026-07-24T15-07-00",
            parent_id,
        )?;
        write_lines(
            &parent,
            &[activity("started", child_id, "/root/closed-child", 10_000)],
        )?;
        let child = rollout_path(
            dir.path(),
            ("2026", "07", "24"),
            "2026-07-24T15-07-01",
            child_id,
        )?;
        write_child_rollout(
            &child,
            child_id,
            parent_id,
            &[child_task_started(10), child_turn_interrupted(10, 20)],
        )?;
        let pane = unique_pane("child_close_prunes");
        remove_journal(&pane);
        let mut tracker = CodexAgentTracker::default();
        tracker.refresh(&pane, Some(parent_id), parent.to_str());
        assert_eq!(tracker.agents()[0].status, CodexAgentStatus::Interrupted);

        let mut file = fs::OpenOptions::new().append(true).open(&parent)?;
        writeln!(file, "{}", close_call(child_id, "close-child"))?;
        writeln!(file, "{}", close_output("close-child"))?;
        tracker.refresh(&pane, Some(parent_id), parent.to_str());
        assert!(tracker.agents().is_empty());

        fs::remove_file(&child)?;
        write_lines(
            &parent,
            &[activity("started", child_id, "/root/closed-child", 30_000)],
        )?;
        tracker.refresh(&pane, Some(parent_id), parent.to_str());

        assert_eq!(tracker.agents().len(), 1);
        assert_eq!(tracker.agents()[0].status, CodexAgentStatus::Unknown);
        remove_journal(&pane);
        Ok(())
    }
}
