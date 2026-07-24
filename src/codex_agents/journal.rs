use std::fs::{File, OpenOptions};
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::os::fd::{AsRawFd, RawFd};
use std::path::PathBuf;
use std::time::SystemTime;

use indexmap::IndexMap;
use serde_json::Value;

use super::CodexAgentStatus;

const JOURNAL_VERSION: u64 = 1;

pub fn journal_file_path(pane_id: &str) -> PathBuf {
    let encoded = pane_id.replace('%', "_");
    PathBuf::from(format!("/tmp/tmux-agent-agents{encoded}.jsonl"))
}

struct FileLock {
    fd: RawFd,
}

impl FileLock {
    fn acquire(file: &File, operation: libc::c_int) -> io::Result<Self> {
        let fd = file.as_raw_fd();
        if unsafe { libc::flock(fd, operation) } != 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(Self { fd })
    }
}

impl Drop for FileLock {
    fn drop(&mut self) {
        unsafe {
            libc::flock(self.fd, libc::LOCK_UN);
        }
    }
}

pub(crate) fn append_lifecycle_event(
    pane_id: &str,
    parent_session_id: &str,
    agent_id: &str,
    agent_type: &str,
    status: CodexAgentStatus,
    occurred_at_ms: u64,
) -> io::Result<()> {
    let state = match status {
        CodexAgentStatus::Working => "working",
        CodexAgentStatus::Done => "done",
        CodexAgentStatus::Interrupted | CodexAgentStatus::Unknown => {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "hook journal only accepts working or done",
            ));
        }
    };
    let record = serde_json::json!({
        "version": JOURNAL_VERSION,
        "parent_session_id": parent_session_id,
        "agent_id": crate::cli::sanitize_tmux_value(agent_id),
        "agent_type": crate::cli::sanitize_tmux_value(agent_type),
        "state": state,
        "occurred_at_ms": occurred_at_ms,
    });
    let mut encoded = serde_json::to_vec(&record).map_err(io::Error::other)?;
    encoded.push(b'\n');

    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .read(true)
        .open(journal_file_path(pane_id))?;
    let _lock = FileLock::acquire(&file, libc::LOCK_EX)?;
    file.write_all(&encoded)
}

pub(crate) fn remove_journal(pane_id: &str) {
    let _ = std::fs::remove_file(journal_file_path(pane_id));
}

#[derive(Debug, Clone, PartialEq, Eq)]
#[allow(dead_code)]
pub(crate) struct LifecycleSnapshot {
    pub agent_id: String,
    pub agent_type: String,
    pub status: CodexAgentStatus,
    pub started_at_ms: Option<u64>,
    pub finished_at_ms: Option<u64>,
    pub last_event_at_ms: u64,
}

#[derive(Debug, Clone, Default)]
#[allow(dead_code)]
pub(crate) struct JournalTracker {
    path: Option<PathBuf>,
    parent_session_id: Option<String>,
    offset: u64,
    modified: Option<SystemTime>,
    partial_line: Vec<u8>,
    snapshots: IndexMap<String, LifecycleSnapshot>,
}

#[allow(dead_code)]
impl JournalTracker {
    pub(crate) fn set_context(&mut self, pane_id: &str, parent_session_id: Option<&str>) -> bool {
        let path = Some(journal_file_path(pane_id));
        let parent_session_id = parent_session_id.map(str::to_owned);
        if self.path == path && self.parent_session_id == parent_session_id {
            return false;
        }

        self.path = path;
        self.parent_session_id = parent_session_id;
        self.reset();
        true
    }

    pub(crate) fn refresh(&mut self) -> io::Result<()> {
        let Some(path) = self.path.as_ref() else {
            return Ok(());
        };
        if self.parent_session_id.is_none() {
            return Ok(());
        }

        let mut file = match OpenOptions::new().read(true).open(path) {
            Ok(file) => file,
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                self.reset();
                return Ok(());
            }
            Err(error) => return Err(error),
        };
        let _lock = FileLock::acquire(&file, libc::LOCK_SH)?;
        let metadata = file.metadata()?;
        if metadata.len() < self.offset {
            self.reset();
        }
        file.seek(SeekFrom::Start(self.offset))?;
        let mut appended = Vec::new();
        file.read_to_end(&mut appended)?;
        self.offset += appended.len() as u64;
        self.modified = metadata.modified().ok();

        if appended.is_empty() {
            return Ok(());
        }

        let mut buffered = std::mem::take(&mut self.partial_line);
        buffered.extend_from_slice(&appended);
        let Some(last_newline) = buffered.iter().rposition(|byte| *byte == b'\n') else {
            self.partial_line = buffered;
            return Ok(());
        };
        self.partial_line = buffered.split_off(last_newline + 1);
        for line in buffered[..last_newline].split(|byte| *byte == b'\n') {
            self.fold_line(line);
        }
        Ok(())
    }

    pub(crate) fn snapshots(&self) -> &IndexMap<String, LifecycleSnapshot> {
        &self.snapshots
    }

    fn reset(&mut self) {
        self.offset = 0;
        self.modified = None;
        self.partial_line.clear();
        self.snapshots.clear();
    }

    fn fold_line(&mut self, line: &[u8]) {
        let Ok(record) = serde_json::from_slice::<Value>(line) else {
            return;
        };
        if record.get("version").and_then(Value::as_u64) != Some(JOURNAL_VERSION)
            || record.get("parent_session_id").and_then(Value::as_str)
                != self.parent_session_id.as_deref()
        {
            return;
        }
        let Some(agent_id) = record.get("agent_id").and_then(Value::as_str) else {
            return;
        };
        let Some(agent_type) = record.get("agent_type").and_then(Value::as_str) else {
            return;
        };
        let Some(occurred_at_ms) = record.get("occurred_at_ms").and_then(Value::as_u64) else {
            return;
        };
        let incoming_status = match record.get("state").and_then(Value::as_str) {
            Some("working") => CodexAgentStatus::Working,
            Some("done") => CodexAgentStatus::Done,
            _ => return,
        };

        if let Some(current) = self.snapshots.get_mut(agent_id) {
            current.agent_type = agent_type.to_owned();
            current.last_event_at_ms = occurred_at_ms;
            match incoming_status {
                CodexAgentStatus::Working if current.status != CodexAgentStatus::Working => {
                    current.status = CodexAgentStatus::Working;
                    current.started_at_ms = Some(occurred_at_ms);
                    current.finished_at_ms = None;
                }
                CodexAgentStatus::Done if current.status != CodexAgentStatus::Done => {
                    current.status = CodexAgentStatus::Done;
                    current.finished_at_ms.get_or_insert(occurred_at_ms);
                }
                CodexAgentStatus::Working | CodexAgentStatus::Done => {}
                CodexAgentStatus::Interrupted | CodexAgentStatus::Unknown => return,
            }
            return;
        }

        let (started_at_ms, finished_at_ms) = match incoming_status {
            CodexAgentStatus::Working => (Some(occurred_at_ms), None),
            CodexAgentStatus::Done => (None, Some(occurred_at_ms)),
            CodexAgentStatus::Interrupted | CodexAgentStatus::Unknown => return,
        };
        self.snapshots.insert(
            agent_id.to_owned(),
            LifecycleSnapshot {
                agent_id: agent_id.to_owned(),
                agent_type: agent_type.to_owned(),
                status: incoming_status,
                started_at_ms,
                finished_at_ms,
                last_event_at_ms: occurred_at_ms,
            },
        );
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::io;
    use std::sync::Arc;
    use std::thread;

    use super::{JournalTracker, append_lifecycle_event, journal_file_path, remove_journal};
    use crate::codex_agents::CodexAgentStatus;

    fn unique_pane(test_name: &str) -> String {
        format!("%JOURNAL_{test_name}_{}", std::process::id())
    }

    fn tracker_for(pane: &str, parent_session_id: &str) -> JournalTracker {
        let mut tracker = JournalTracker::default();
        assert!(tracker.set_context(pane, Some(parent_session_id)));
        tracker
    }

    #[test]
    fn folds_start_and_stop_into_one_done_snapshot() -> io::Result<()> {
        let pane = unique_pane("folds_start_and_stop");
        remove_journal(&pane);

        append_lifecycle_event(
            &pane,
            "parent-1",
            "agent-a",
            "worker",
            CodexAgentStatus::Working,
            10_000,
        )?;
        append_lifecycle_event(
            &pane,
            "parent-1",
            "agent-a",
            "worker",
            CodexAgentStatus::Done,
            25_000,
        )?;

        let mut tracker = tracker_for(&pane, "parent-1");
        tracker.refresh()?;
        let snapshot = tracker.snapshots().get("agent-a").expect("agent-a");
        assert_eq!(snapshot.status, CodexAgentStatus::Done);
        assert_eq!(snapshot.started_at_ms, Some(10_000));
        assert_eq!(snapshot.finished_at_ms, Some(25_000));
        assert_eq!(snapshot.last_event_at_ms, 25_000);

        remove_journal(&pane);
        Ok(())
    }

    #[test]
    fn duplicate_start_preserves_first_start() -> io::Result<()> {
        let pane = unique_pane("duplicate_start");
        remove_journal(&pane);
        append_lifecycle_event(
            &pane,
            "parent-1",
            "agent-a",
            "worker",
            CodexAgentStatus::Working,
            10_000,
        )?;
        append_lifecycle_event(
            &pane,
            "parent-1",
            "agent-a",
            "worker",
            CodexAgentStatus::Working,
            12_000,
        )?;

        let mut tracker = tracker_for(&pane, "parent-1");
        tracker.refresh()?;
        let snapshot = tracker.snapshots().get("agent-a").expect("agent-a");
        assert_eq!(snapshot.started_at_ms, Some(10_000));
        assert_eq!(snapshot.last_event_at_ms, 12_000);

        remove_journal(&pane);
        Ok(())
    }

    #[test]
    fn duplicate_stop_preserves_first_finish() -> io::Result<()> {
        let pane = unique_pane("duplicate_stop");
        remove_journal(&pane);
        for (status, occurred_at_ms) in [
            (CodexAgentStatus::Working, 10_000),
            (CodexAgentStatus::Done, 25_000),
            (CodexAgentStatus::Done, 30_000),
        ] {
            append_lifecycle_event(
                &pane,
                "parent-1",
                "agent-a",
                "worker",
                status,
                occurred_at_ms,
            )?;
        }

        let mut tracker = tracker_for(&pane, "parent-1");
        tracker.refresh()?;
        let snapshot = tracker.snapshots().get("agent-a").expect("agent-a");
        assert_eq!(snapshot.finished_at_ms, Some(25_000));
        assert_eq!(snapshot.last_event_at_ms, 30_000);

        remove_journal(&pane);
        Ok(())
    }

    #[test]
    fn later_start_after_done_restarts_and_clears_finish() -> io::Result<()> {
        let pane = unique_pane("restart_after_done");
        remove_journal(&pane);
        for (status, occurred_at_ms) in [
            (CodexAgentStatus::Working, 10_000),
            (CodexAgentStatus::Done, 25_000),
            (CodexAgentStatus::Working, 40_000),
        ] {
            append_lifecycle_event(
                &pane,
                "parent-1",
                "agent-a",
                "worker",
                status,
                occurred_at_ms,
            )?;
        }

        let mut tracker = tracker_for(&pane, "parent-1");
        tracker.refresh()?;
        let snapshot = tracker.snapshots().get("agent-a").expect("agent-a");
        assert_eq!(snapshot.status, CodexAgentStatus::Working);
        assert_eq!(snapshot.started_at_ms, Some(40_000));
        assert_eq!(snapshot.finished_at_ms, None);

        remove_journal(&pane);
        Ok(())
    }

    #[test]
    fn stop_before_start_creates_done_without_start() -> io::Result<()> {
        let pane = unique_pane("stop_before_start");
        remove_journal(&pane);
        append_lifecycle_event(
            &pane,
            "parent-1",
            "agent-a",
            "worker",
            CodexAgentStatus::Done,
            25_000,
        )?;

        let mut tracker = tracker_for(&pane, "parent-1");
        tracker.refresh()?;
        let snapshot = tracker.snapshots().get("agent-a").expect("agent-a");
        assert_eq!(snapshot.status, CodexAgentStatus::Done);
        assert_eq!(snapshot.started_at_ms, None);
        assert_eq!(snapshot.finished_at_ms, Some(25_000));

        remove_journal(&pane);
        Ok(())
    }

    #[test]
    fn ignores_records_for_other_parent_sessions() -> io::Result<()> {
        let pane = unique_pane("parent_filter");
        remove_journal(&pane);
        append_lifecycle_event(
            &pane,
            "parent-2",
            "agent-b",
            "worker",
            CodexAgentStatus::Working,
            10_000,
        )?;
        append_lifecycle_event(
            &pane,
            "parent-1",
            "agent-a",
            "worker",
            CodexAgentStatus::Working,
            11_000,
        )?;

        let mut tracker = tracker_for(&pane, "parent-1");
        tracker.refresh()?;
        assert_eq!(
            tracker
                .snapshots()
                .keys()
                .map(String::as_str)
                .collect::<Vec<_>>(),
            vec!["agent-a"]
        );

        remove_journal(&pane);
        Ok(())
    }

    #[test]
    fn ignores_missing_malformed_and_unknown_version_lines() -> io::Result<()> {
        let pane = unique_pane("invalid_lines");
        remove_journal(&pane);
        let path = journal_file_path(&pane);
        fs::write(
            &path,
            concat!(
                "\n",
                "not-json\n",
                "{\"version\":2,\"parent_session_id\":\"parent-1\",\"agent_id\":\"old\",\"agent_type\":\"worker\",\"state\":\"working\",\"occurred_at_ms\":1}\n",
                "{\"version\":1,\"parent_session_id\":\"parent-1\"}\n",
            ),
        )?;
        append_lifecycle_event(
            &pane,
            "parent-1",
            "agent-a",
            "worker",
            CodexAgentStatus::Working,
            10_000,
        )?;

        let mut tracker = tracker_for(&pane, "parent-1");
        tracker.refresh()?;
        assert_eq!(
            tracker
                .snapshots()
                .keys()
                .map(String::as_str)
                .collect::<Vec<_>>(),
            vec!["agent-a"]
        );

        remove_journal(&pane);
        Ok(())
    }

    #[test]
    fn concurrent_writers_produce_two_parseable_snapshots() -> io::Result<()> {
        let pane = Arc::new(unique_pane("concurrent_writers"));
        remove_journal(&pane);
        let mut writers = Vec::new();
        for (agent_id, occurred_at_ms) in [("agent-a", 10_000), ("agent-b", 11_000)] {
            let pane = Arc::clone(&pane);
            writers.push(thread::spawn(move || {
                append_lifecycle_event(
                    &pane,
                    "parent-1",
                    agent_id,
                    "worker",
                    CodexAgentStatus::Working,
                    occurred_at_ms,
                )
            }));
        }
        for writer in writers {
            writer.join().expect("writer thread")?;
        }

        let mut tracker = tracker_for(&pane, "parent-1");
        tracker.refresh()?;
        assert_eq!(tracker.snapshots().len(), 2);
        assert!(tracker.snapshots().contains_key("agent-a"));
        assert!(tracker.snapshots().contains_key("agent-b"));

        remove_journal(&pane);
        Ok(())
    }
}
