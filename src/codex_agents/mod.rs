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

        let mut transcript = self.transcript.clone();
        transcript.set_context(transcript_path, parent_session_id);
        if transcript.refresh().is_ok() {
            self.transcript = transcript;
        }

        let mut journal = self.journal.clone();
        journal.set_context(pane_id, parent_session_id);
        if journal.refresh().is_ok() {
            self.journal = journal;
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
            ));
        }

        for snapshot in self.journal.snapshots().values() {
            if self.transcript.agents().contains_key(&snapshot.agent_id)
                || self.transcript.closed_ids().contains(&snapshot.agent_id)
            {
                continue;
            }
            self.agents
                .push(Self::merge_agent(&snapshot.agent_id, None, Some(snapshot)));
        }
    }

    fn merge_agent(
        id: &str,
        catalog: Option<&transcript::CatalogAgent>,
        lifecycle: Option<&journal::LifecycleSnapshot>,
    ) -> CodexAgentInfo {
        let interrupted_at_ms = catalog.and_then(|agent| agent.interrupted_at_ms);
        let status = match (lifecycle, interrupted_at_ms) {
            (Some(snapshot), Some(interrupted)) if interrupted > snapshot.last_event_at_ms => {
                CodexAgentStatus::Interrupted
            }
            (Some(snapshot), _) => snapshot.status,
            (None, Some(_)) => CodexAgentStatus::Interrupted,
            (None, None) => CodexAgentStatus::Unknown,
        };

        CodexAgentInfo {
            id: id.to_owned(),
            path: catalog
                .map(|agent| agent.path.clone())
                .filter(|path| !path.is_empty())
                .unwrap_or_default(),
            fallback_agent_type: lifecycle
                .map(|snapshot| snapshot.agent_type.clone())
                .unwrap_or_default(),
            status,
            started_at: lifecycle
                .and_then(|snapshot| snapshot.started_at_ms)
                .map(|timestamp| timestamp / 1_000),
            finished_at: lifecycle
                .and_then(|snapshot| snapshot.finished_at_ms)
                .map(|timestamp| timestamp / 1_000),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::fs::{self, File};
    use std::io::{self, Write};
    use std::path::Path;

    use serde_json::{Value, json};

    use super::{CodexAgentStatus, CodexAgentTracker};
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
}
