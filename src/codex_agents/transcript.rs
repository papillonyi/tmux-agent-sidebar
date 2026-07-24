use std::collections::{HashMap, HashSet};
use std::fs::File;
use std::io::{self, Read, Seek, SeekFrom};
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use indexmap::IndexMap;
use serde_json::Value;

use super::CodexAgentStatus;

const CONTENT_ANCHOR_BYTES: usize = 4 * 1024;
const MAX_DISCOVERY_BACKOFF_TICKS: u64 = 30;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct FileIdentity {
    device: u64,
    inode: u64,
}

impl FileIdentity {
    fn from_metadata(metadata: &std::fs::Metadata) -> Self {
        Self {
            device: metadata.dev(),
            inode: metadata.ino(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ChildLifecycleSnapshot {
    pub status: CodexAgentStatus,
    pub started_at_ms: Option<u64>,
    pub finished_at_ms: Option<u64>,
    pub last_event_at_ms: u64,
}

#[derive(Debug, Clone)]
struct ChildRolloutTracker {
    path: PathBuf,
    child_id: String,
    parent_session_id: String,
    offset: u64,
    modified: Option<SystemTime>,
    file_identity: Option<FileIdentity>,
    initialized: bool,
    partial_line: Vec<u8>,
    content_anchor: Vec<u8>,
    validated: bool,
    lifecycle: Option<ChildLifecycleSnapshot>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ChildRefreshOutcome {
    Present,
    Missing,
}

#[derive(Debug, Clone)]
struct DiscoveryRetry {
    next_tick: u64,
    delay_ticks: u64,
}

impl DiscoveryRetry {
    fn due_now(tick: u64) -> Self {
        Self {
            next_tick: tick,
            delay_ticks: 1,
        }
    }

    fn postpone(&mut self, tick: u64) {
        self.next_tick = tick.saturating_add(self.delay_ticks);
        self.delay_ticks = self
            .delay_ticks
            .saturating_mul(2)
            .min(MAX_DISCOVERY_BACKOFF_TICKS);
    }
}

impl ChildRolloutTracker {
    fn new(path: PathBuf, child_id: &str, parent_session_id: &str) -> Self {
        Self {
            path,
            child_id: child_id.to_owned(),
            parent_session_id: parent_session_id.to_owned(),
            offset: 0,
            modified: None,
            file_identity: None,
            initialized: false,
            partial_line: Vec::new(),
            content_anchor: Vec::new(),
            validated: false,
            lifecycle: None,
        }
    }

    fn refresh(&mut self) -> io::Result<ChildRefreshOutcome> {
        let mut file = match File::open(&self.path) {
            Ok(file) => file,
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                return Ok(ChildRefreshOutcome::Missing);
            }
            Err(error) => return Err(error),
        };
        let metadata = file.metadata()?;
        let len = metadata.len();
        let modified = metadata.modified().ok();
        let file_identity = FileIdentity::from_metadata(&metadata);

        let mut rebuild = !self.initialized
            || self.file_identity != Some(file_identity)
            || len < self.offset
            || (len == self.offset && self.modified != modified);
        if !rebuild && !self.content_anchor_matches(&mut file)? {
            rebuild = true;
        }
        if !rebuild && self.offset == len && self.modified == modified {
            return Ok(ChildRefreshOutcome::Present);
        }

        let start = if rebuild { 0 } else { self.offset };
        if rebuild {
            self.reset_file();
        }

        file.seek(SeekFrom::Start(start))?;
        let mut appended = Vec::new();
        file.read_to_end(&mut appended)?;
        let consumed_offset = file.stream_position()?;
        let final_metadata = file.metadata()?;
        let final_modified = final_metadata.modified().ok();
        let final_file_identity = FileIdentity::from_metadata(&final_metadata);
        self.update_content_anchor(&appended);

        let mut buffered = std::mem::take(&mut self.partial_line);
        buffered.extend_from_slice(&appended);
        let complete_len = buffered
            .iter()
            .rposition(|byte| *byte == b'\n')
            .map_or(0, |index| index + 1);
        self.partial_line = buffered.split_off(complete_len);
        for line in buffered.split(|byte| *byte == b'\n') {
            self.fold_line(line);
        }

        self.offset = consumed_offset;
        self.modified = final_modified;
        self.file_identity = Some(final_file_identity);
        self.initialized = true;
        Ok(ChildRefreshOutcome::Present)
    }

    fn reset_file(&mut self) {
        self.offset = 0;
        self.modified = None;
        self.file_identity = None;
        self.initialized = false;
        self.partial_line.clear();
        self.content_anchor.clear();
        self.validated = false;
        self.lifecycle = None;
    }

    fn content_anchor_matches(&self, file: &mut File) -> io::Result<bool> {
        if self.content_anchor.is_empty() {
            return Ok(true);
        }
        let anchor_len = self.content_anchor.len() as u64;
        file.seek(SeekFrom::Start(self.offset.saturating_sub(anchor_len)))?;
        let mut current = vec![0; self.content_anchor.len()];
        match file.read_exact(&mut current) {
            Ok(()) => Ok(current == self.content_anchor),
            Err(error) if error.kind() == io::ErrorKind::UnexpectedEof => Ok(false),
            Err(error) => Err(error),
        }
    }

    fn update_content_anchor(&mut self, appended: &[u8]) {
        self.content_anchor.extend_from_slice(appended);
        if self.content_anchor.len() > CONTENT_ANCHOR_BYTES {
            let keep_from = self.content_anchor.len() - CONTENT_ANCHOR_BYTES;
            self.content_anchor.drain(..keep_from);
        }
    }

    fn fold_line(&mut self, line: &[u8]) {
        let Ok(record) = serde_json::from_slice::<Value>(line) else {
            return;
        };
        match record.get("type").and_then(Value::as_str) {
            Some("session_meta") => {
                self.validated = self.valid_session_meta(record.get("payload"));
            }
            Some("event_msg") if self.validated => {
                self.fold_lifecycle_event(record.get("payload"));
            }
            _ => {}
        }
    }

    fn valid_session_meta(&self, payload: Option<&Value>) -> bool {
        let Some(payload) = payload else {
            return false;
        };
        if payload.get("id").and_then(Value::as_str) != Some(self.child_id.as_str())
            || payload.get("session_id").and_then(Value::as_str)
                != Some(self.parent_session_id.as_str())
        {
            return false;
        }
        if payload
            .get("parent_thread_id")
            .and_then(Value::as_str)
            .is_some_and(|id| id != self.parent_session_id)
        {
            return false;
        }
        if payload
            .get("thread_source")
            .and_then(Value::as_str)
            .is_some_and(|source| source != "subagent")
        {
            return false;
        }

        match payload.get("source") {
            None | Some(Value::Null) => true,
            Some(Value::String(source)) => source == "subagent",
            Some(Value::Object(source)) => {
                let Some(subagent) = source.get("subagent") else {
                    return false;
                };
                subagent
                    .pointer("/thread_spawn/parent_thread_id")
                    .and_then(Value::as_str)
                    .is_none_or(|id| id == self.parent_session_id)
            }
            Some(_) => false,
        }
    }

    fn fold_lifecycle_event(&mut self, payload: Option<&Value>) {
        let Some(payload) = payload else {
            return;
        };
        let event_type = payload.get("type").and_then(Value::as_str);
        let (status, occurred_at_ms) = match event_type {
            Some("task_started") => {
                let Some(timestamp) = seconds_to_millis(payload.get("started_at")) else {
                    return;
                };
                (CodexAgentStatus::Working, timestamp)
            }
            Some("task_complete") => {
                let Some(timestamp) = seconds_to_millis(payload.get("completed_at")) else {
                    return;
                };
                (CodexAgentStatus::Done, timestamp)
            }
            Some("turn_aborted")
                if payload.get("reason").and_then(Value::as_str) == Some("interrupted") =>
            {
                let Some(timestamp) = seconds_to_millis(payload.get("completed_at")) else {
                    return;
                };
                (CodexAgentStatus::Interrupted, timestamp)
            }
            _ => return,
        };

        if self
            .lifecycle
            .as_ref()
            .is_some_and(|current| occurred_at_ms < current.last_event_at_ms)
        {
            return;
        }
        let event_started_at_ms = seconds_to_millis(payload.get("started_at"));
        let previous_started_at_ms = self
            .lifecycle
            .as_ref()
            .and_then(|current| current.started_at_ms);
        self.lifecycle = Some(ChildLifecycleSnapshot {
            status,
            started_at_ms: event_started_at_ms.or(previous_started_at_ms),
            finished_at_ms: match status {
                CodexAgentStatus::Done | CodexAgentStatus::Interrupted => Some(occurred_at_ms),
                CodexAgentStatus::Working | CodexAgentStatus::Unknown => None,
            },
            last_event_at_ms: occurred_at_ms,
        });
    }
}

fn seconds_to_millis(value: Option<&Value>) -> Option<u64> {
    value.as_ref()?.as_u64()?.checked_mul(1_000)
}

#[derive(Debug, Clone, PartialEq, Eq)]
#[allow(dead_code)]
pub(crate) struct CatalogAgent {
    pub id: String,
    pub path: String,
    pub interrupted_at_ms: Option<u64>,
}

#[derive(Debug, Clone, Default)]
#[allow(dead_code)]
pub(crate) struct TranscriptTracker {
    path: Option<PathBuf>,
    parent_session_id: Option<String>,
    offset: u64,
    modified: Option<SystemTime>,
    file_identity: Option<FileIdentity>,
    initialized: bool,
    partial_line: Vec<u8>,
    content_anchor: Vec<u8>,
    agents: IndexMap<String, CatalogAgent>,
    pending_close_targets: HashMap<String, String>,
    closed_ids: HashSet<String>,
    child_trackers: HashMap<String, ChildRolloutTracker>,
    child_lifecycles: HashMap<String, ChildLifecycleSnapshot>,
    discovery_retries: HashMap<String, DiscoveryRetry>,
    discovery_tick: u64,
}

#[allow(dead_code)]
impl TranscriptTracker {
    pub(crate) fn set_context(
        &mut self,
        path: Option<&str>,
        parent_session_id: Option<&str>,
    ) -> bool {
        let path = path.filter(|value| !value.is_empty()).map(PathBuf::from);
        let parent_session_id = parent_session_id.map(str::to_owned);
        if self.path == path && self.parent_session_id == parent_session_id {
            return false;
        }

        self.path = path;
        self.parent_session_id = parent_session_id;
        self.reset_context();
        true
    }

    pub(crate) fn refresh(&mut self) -> io::Result<()> {
        self.refresh_parent()?;
        self.refresh_children();
        Ok(())
    }

    fn refresh_parent(&mut self) -> io::Result<()> {
        let Some(path) = self.path.clone() else {
            return Ok(());
        };
        if self.parent_session_id.is_none() {
            return Ok(());
        }

        let mut file = match File::open(&path) {
            Ok(file) => file,
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                self.reset_parent();
                return Ok(());
            }
            Err(error) => return Err(error),
        };
        let metadata = file.metadata()?;
        let len = metadata.len();
        let modified = metadata.modified().ok();
        let file_identity = FileIdentity::from_metadata(&metadata);

        if self.initialized
            && self.offset == len
            && self.modified == modified
            && self.file_identity == Some(file_identity)
        {
            return Ok(());
        }

        let mut rebuild = !self.initialized
            || self.file_identity != Some(file_identity)
            || len < self.offset
            || (len == self.offset && self.modified != modified);
        // Atomic replacement changes the file identity. An in-place
        // truncate-and-rewrite keeps it, so also verify the bytes immediately
        // before the saved cursor before treating growth as an append.
        if !rebuild && !self.content_anchor_matches(&mut file)? {
            rebuild = true;
        }
        let start = if rebuild { 0 } else { self.offset };
        if rebuild {
            self.reset_parent();
        }

        file.seek(SeekFrom::Start(start))?;
        let mut appended = Vec::new();
        file.read_to_end(&mut appended)?;
        // A writer may append after the initial metadata read. Commit the
        // cursor and metadata for what this file handle actually consumed so
        // those bytes are not read into `partial_line` a second time.
        let consumed_offset = file.stream_position()?;
        let final_metadata = file.metadata()?;
        let final_modified = final_metadata.modified().ok();
        let final_file_identity = FileIdentity::from_metadata(&final_metadata);
        self.update_content_anchor(&appended);

        let mut buffered = std::mem::take(&mut self.partial_line);
        buffered.extend_from_slice(&appended);
        let complete_len = buffered
            .iter()
            .rposition(|byte| *byte == b'\n')
            .map_or(0, |index| index + 1);
        self.partial_line = buffered.split_off(complete_len);
        for line in buffered.split(|byte| *byte == b'\n') {
            self.fold_line(line);
        }

        self.offset = consumed_offset;
        self.modified = final_modified;
        self.file_identity = Some(final_file_identity);
        self.initialized = true;
        Ok(())
    }

    pub(crate) fn agents(&self) -> &IndexMap<String, CatalogAgent> {
        &self.agents
    }

    pub(crate) fn closed_ids(&self) -> &HashSet<String> {
        &self.closed_ids
    }

    pub(crate) fn child_lifecycles(&self) -> &HashMap<String, ChildLifecycleSnapshot> {
        &self.child_lifecycles
    }

    fn reset_context(&mut self) {
        self.reset_parent();
        self.child_trackers.clear();
        self.child_lifecycles.clear();
        self.discovery_retries.clear();
        self.discovery_tick = 0;
    }

    fn reset_parent(&mut self) {
        self.offset = 0;
        self.modified = None;
        self.file_identity = None;
        self.initialized = false;
        self.partial_line.clear();
        self.content_anchor.clear();
        self.agents.clear();
        self.pending_close_targets.clear();
        self.closed_ids.clear();
    }

    fn refresh_children(&mut self) {
        let (Some(parent_session_id), Some(sessions_root)) = (
            self.parent_session_id.clone(),
            self.path.as_deref().and_then(sessions_root),
        ) else {
            self.child_trackers.clear();
            self.child_lifecycles.clear();
            self.discovery_retries.clear();
            return;
        };

        self.discovery_tick = self.discovery_tick.saturating_add(1);
        let tick = self.discovery_tick;
        let live_ids = self
            .agents
            .keys()
            .filter(|id| !self.closed_ids.contains(*id))
            .cloned()
            .collect::<HashSet<_>>();
        self.child_trackers.retain(|id, _| live_ids.contains(id));
        self.child_lifecycles.retain(|id, _| live_ids.contains(id));
        self.discovery_retries.retain(|id, _| live_ids.contains(id));

        let tracked_ids = self.child_trackers.keys().cloned().collect::<Vec<_>>();
        for id in tracked_ids {
            let Some(current) = self.child_trackers.get(&id).cloned() else {
                continue;
            };
            let mut candidate = current;
            match candidate.refresh() {
                Ok(ChildRefreshOutcome::Present) if candidate.validated => {
                    if let Some(lifecycle) = candidate.lifecycle.clone() {
                        self.child_lifecycles.insert(id.clone(), lifecycle);
                    }
                    self.child_trackers.insert(id.clone(), candidate);
                    self.discovery_retries.remove(&id);
                }
                Ok(ChildRefreshOutcome::Missing) => {
                    self.child_trackers.remove(&id);
                    self.discovery_retries
                        .entry(id)
                        .or_insert_with(|| DiscoveryRetry::due_now(tick));
                }
                Ok(ChildRefreshOutcome::Present) | Err(_) => {
                    // Preserve the last validated tracker and lifecycle on a
                    // transient read or validation failure.
                }
            }
        }

        for id in &live_ids {
            if !self.child_trackers.contains_key(id) {
                self.discovery_retries
                    .entry(id.clone())
                    .or_insert_with(|| DiscoveryRetry::due_now(tick));
            }
        }
        let due_ids = self
            .discovery_retries
            .iter()
            .filter(|(_, retry)| retry.next_tick <= tick)
            .map(|(id, _)| id.clone())
            .collect::<HashSet<_>>();
        if due_ids.is_empty() {
            return;
        }

        let candidates = discover_child_rollouts(&sessions_root, &due_ids);
        for id in due_ids {
            let mut resolved = false;
            if let Some(paths) = candidates.get(&id) {
                for path in paths {
                    let mut candidate =
                        ChildRolloutTracker::new(path.clone(), &id, &parent_session_id);
                    if matches!(candidate.refresh(), Ok(ChildRefreshOutcome::Present))
                        && candidate.validated
                    {
                        if let Some(lifecycle) = candidate.lifecycle.clone() {
                            self.child_lifecycles.insert(id.clone(), lifecycle);
                        }
                        self.child_trackers.insert(id.clone(), candidate);
                        self.discovery_retries.remove(&id);
                        resolved = true;
                        break;
                    }
                }
            }
            if !resolved {
                self.discovery_retries
                    .entry(id)
                    .or_insert_with(|| DiscoveryRetry::due_now(tick))
                    .postpone(tick);
            }
        }
    }

    fn content_anchor_matches(&self, file: &mut File) -> io::Result<bool> {
        if self.content_anchor.is_empty() {
            return Ok(true);
        }
        let anchor_len = self.content_anchor.len() as u64;
        file.seek(SeekFrom::Start(self.offset.saturating_sub(anchor_len)))?;
        let mut current = vec![0; self.content_anchor.len()];
        file.read_exact(&mut current)?;
        Ok(current == self.content_anchor)
    }

    fn update_content_anchor(&mut self, appended: &[u8]) {
        self.content_anchor.extend_from_slice(appended);
        if self.content_anchor.len() > CONTENT_ANCHOR_BYTES {
            let keep_from = self.content_anchor.len() - CONTENT_ANCHOR_BYTES;
            self.content_anchor.drain(..keep_from);
        }
    }

    fn fold_line(&mut self, line: &[u8]) {
        let Ok(record) = serde_json::from_slice::<Value>(line) else {
            return;
        };
        match record.get("type").and_then(Value::as_str) {
            Some("event_msg") => self.fold_event(record.get("payload")),
            Some("response_item") => self.fold_response(record.get("payload")),
            _ => {}
        }
    }

    fn fold_event(&mut self, payload: Option<&Value>) {
        let Some(payload) = payload else {
            return;
        };
        if payload.get("type").and_then(Value::as_str) != Some("sub_agent_activity") {
            return;
        }
        let status = match payload.get("kind").and_then(Value::as_str) {
            Some("started") => CodexAgentStatus::Working,
            Some("interrupted") => CodexAgentStatus::Interrupted,
            _ => return,
        };
        let Some(raw_id) = payload.get("agent_thread_id").and_then(Value::as_str) else {
            return;
        };
        let id = crate::cli::sanitize_tmux_value(raw_id);
        if id.is_empty() {
            return;
        }
        let path = payload
            .get("agent_path")
            .and_then(Value::as_str)
            .map(crate::cli::sanitize_tmux_value)
            .unwrap_or_default();

        match status {
            CodexAgentStatus::Working => {
                if let Some(agent) = self.agents.get_mut(&id) {
                    if agent.path.is_empty() && !path.is_empty() {
                        agent.path = path;
                    }
                } else {
                    self.agents.insert(
                        id.clone(),
                        CatalogAgent {
                            id,
                            path,
                            interrupted_at_ms: None,
                        },
                    );
                }
            }
            CodexAgentStatus::Interrupted => {
                let Some(occurred_at_ms) = payload.get("occurred_at_ms").and_then(Value::as_u64)
                else {
                    return;
                };
                if let Some(agent) = self.agents.get_mut(&id) {
                    agent.interrupted_at_ms = Some(
                        agent
                            .interrupted_at_ms
                            .map_or(occurred_at_ms, |current| current.max(occurred_at_ms)),
                    );
                }
            }
            CodexAgentStatus::Done | CodexAgentStatus::Unknown => {}
        }
    }

    fn fold_response(&mut self, payload: Option<&Value>) {
        let Some(payload) = payload else {
            return;
        };
        match payload.get("type").and_then(Value::as_str) {
            Some("function_call")
                if payload.get("name").and_then(Value::as_str) == Some("close_agent") =>
            {
                self.remember_close_target(payload);
            }
            Some("function_call_output") => self.complete_close(payload),
            _ => {}
        }
    }

    fn remember_close_target(&mut self, payload: &Value) {
        let Some(call_id) = payload.get("call_id").and_then(Value::as_str) else {
            return;
        };
        let Some(arguments) = payload.get("arguments").and_then(Value::as_str) else {
            return;
        };
        let Ok(arguments) = serde_json::from_str::<Value>(arguments) else {
            return;
        };
        let Some(raw_target) = arguments.get("target").and_then(Value::as_str) else {
            return;
        };
        let target = crate::cli::sanitize_tmux_value(raw_target);
        if call_id.is_empty() || target.is_empty() {
            return;
        }
        self.pending_close_targets
            .insert(call_id.to_owned(), target);
    }

    fn complete_close(&mut self, payload: &Value) {
        let Some(call_id) = payload.get("call_id").and_then(Value::as_str) else {
            return;
        };
        let Some(target) = self.pending_close_targets.remove(call_id) else {
            return;
        };
        let Some(output) = payload.get("output").and_then(Value::as_str) else {
            return;
        };
        let Ok(output) = serde_json::from_str::<Value>(output) else {
            return;
        };
        if output
            .get("previous_status")
            .and_then(Value::as_str)
            .is_none()
        {
            return;
        }

        if self.agents.contains_key(&target) {
            self.closed_ids.insert(target);
            return;
        }
        let mut matches = self
            .agents
            .values()
            .filter(|agent| agent.path == target)
            .map(|agent| agent.id.clone());
        let Some(id) = matches.next() else {
            return;
        };
        if matches.next().is_none() {
            self.closed_ids.insert(id);
        }
    }
}

fn sessions_root(path: &Path) -> Option<PathBuf> {
    path.ancestors()
        .find(|ancestor| ancestor.file_name().and_then(|name| name.to_str()) == Some("sessions"))
        .map(Path::to_path_buf)
}

fn discover_child_rollouts(
    root: &Path,
    child_ids: &HashSet<String>,
) -> HashMap<String, Vec<PathBuf>> {
    let mut matches = HashMap::<String, Vec<PathBuf>>::new();
    let mut stack = vec![root.to_path_buf()];

    while let Some(directory) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(directory) else {
            continue;
        };
        for entry in entries {
            let Ok(entry) = entry else {
                continue;
            };
            let Ok(file_type) = entry.file_type() else {
                continue;
            };
            if file_type.is_dir() {
                stack.push(entry.path());
                continue;
            }
            if !file_type.is_file() {
                continue;
            }
            let Some(file_name) = entry.file_name().to_str().map(str::to_owned) else {
                continue;
            };
            for id in child_ids {
                if file_name.ends_with(&format!("-{id}.jsonl")) {
                    matches.entry(id.clone()).or_default().push(entry.path());
                    break;
                }
            }
        }
    }

    for paths in matches.values_mut() {
        paths.sort_unstable_by(|left, right| right.cmp(left));
    }
    matches
}

#[cfg(test)]
mod tests {
    use std::fs::{self, OpenOptions};
    use std::io::{self, Write};
    use std::path::Path;

    use serde_json::{Value, json};

    use super::TranscriptTracker;

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

    fn close_output(call_id: &str, output: &str) -> Value {
        json!({
            "type": "response_item",
            "payload": {
                "type": "function_call_output",
                "call_id": call_id,
                "output": output
            }
        })
    }

    fn write_lines(path: &Path, lines: &[Value]) -> io::Result<()> {
        let mut file = fs::File::create(path)?;
        for line in lines {
            writeln!(file, "{line}")?;
        }
        Ok(())
    }

    fn append_lines(path: &Path, lines: &[Value]) -> io::Result<()> {
        let mut file = OpenOptions::new().append(true).open(path)?;
        for line in lines {
            writeln!(file, "{line}")?;
        }
        Ok(())
    }

    fn tracker_for(path: &Path, parent_session_id: &str) -> TranscriptTracker {
        let mut tracker = TranscriptTracker::default();
        assert!(tracker.set_context(path.to_str(), Some(parent_session_id)));
        tracker
    }

    #[test]
    fn preserves_full_identity_first_seen_order_and_interruption() -> io::Result<()> {
        let dir = tempfile::tempdir()?;
        let path = dir.path().join("rollout.jsonl");
        let agent_a = "019fb1d2-8b6f-7ac0-a8a4-3ad05c8fd115";
        write_lines(
            &path,
            &[
                activity("started", agent_a, "", 1_784_257_433_136),
                activity("started", "agent-b", "/root/task2_build", 1_784_257_440_000),
                activity("started", agent_a, "/root/task1_review", 1_784_257_445_000),
                activity(
                    "started",
                    agent_a,
                    "/root/duplicate-must-not-replace",
                    1_784_257_450_000,
                ),
                activity(
                    "interrupted",
                    agent_a,
                    "/root/task1_review",
                    1_784_257_531_130,
                ),
                activity(
                    "interrupted",
                    agent_a,
                    "/root/task1_review",
                    1_784_257_500_000,
                ),
            ],
        )?;

        let mut tracker = tracker_for(&path, "parent-1");
        tracker.refresh()?;

        assert_eq!(
            tracker
                .agents()
                .keys()
                .map(String::as_str)
                .collect::<Vec<_>>(),
            vec![agent_a, "agent-b"]
        );
        assert_eq!(tracker.agents()[agent_a].id, agent_a);
        assert_eq!(tracker.agents()[agent_a].path, "/root/task1_review");
        assert_eq!(
            tracker.agents()[agent_a].interrupted_at_ms,
            Some(1_784_257_531_130)
        );
        assert_eq!(tracker.agents()["agent-b"].interrupted_at_ms, None);
        Ok(())
    }

    #[test]
    fn irrelevant_and_malformed_records_do_not_remove_or_reorder_agents() -> io::Result<()> {
        let dir = tempfile::tempdir()?;
        let path = dir.path().join("rollout.jsonl");
        write_lines(
            &path,
            &[
                activity("started", "agent-a", "/root/task-a", 10),
                activity("started", "agent-b", "/root/task-b", 20),
                json!({
                    "type": "event_msg",
                    "payload": {
                        "type": "world_state",
                        "agents": [{ "id": "agent-b" }]
                    }
                }),
                activity("interacted", "agent-b", "/root/task-b", 30),
                activity("future-kind", "agent-a", "/root/task-a", 40),
            ],
        )?;
        let mut file = OpenOptions::new().append(true).open(&path)?;
        file.write_all(b"{ malformed json\n")?;

        let mut tracker = tracker_for(&path, "parent-1");
        tracker.refresh()?;

        assert_eq!(
            tracker
                .agents()
                .keys()
                .map(String::as_str)
                .collect::<Vec<_>>(),
            vec!["agent-a", "agent-b"]
        );
        assert!(tracker.closed_ids().is_empty());
        Ok(())
    }

    #[test]
    fn normalizes_catalog_fields_and_ignores_empty_ids() -> io::Result<()> {
        let dir = tempfile::tempdir()?;
        let path = dir.path().join("rollout.jsonl");
        write_lines(
            &path,
            &[
                activity("started", "agent|\na", "/root/task|\na", 10),
                activity("started", "", "/root/ignored", 20),
            ],
        )?;

        let mut tracker = tracker_for(&path, "parent-1");
        tracker.refresh()?;

        assert_eq!(
            tracker
                .agents()
                .keys()
                .map(String::as_str)
                .collect::<Vec<_>>(),
            vec!["agent  a"]
        );
        assert_eq!(tracker.agents()["agent  a"].path, "/root/task  a");
        Ok(())
    }

    #[test]
    fn successful_close_resolves_exact_id_and_unique_catalog_path() -> io::Result<()> {
        let dir = tempfile::tempdir()?;
        let path = dir.path().join("rollout.jsonl");
        write_lines(
            &path,
            &[
                activity("started", "agent-a", "/root/task-a", 10),
                activity("started", "agent-b", "/root/task-b", 20),
                close_call("agent-a", "call-close-a"),
                close_output("call-close-a", r#"{"previous_status":"completed"}"#),
                close_call("/root/task-b", "call-close-b"),
                close_output("call-close-b", r#"{"previous_status":"completed"}"#),
            ],
        )?;

        let mut tracker = tracker_for(&path, "parent-1");
        tracker.refresh()?;

        assert_eq!(
            tracker
                .closed_ids()
                .iter()
                .map(String::as_str)
                .collect::<std::collections::HashSet<_>>(),
            std::collections::HashSet::from(["agent-a", "agent-b"])
        );
        Ok(())
    }

    #[test]
    fn close_requires_correlated_success_and_unambiguous_target() -> io::Result<()> {
        let dir = tempfile::tempdir()?;
        let path = dir.path().join("rollout.jsonl");
        write_lines(
            &path,
            &[
                activity("started", "agent-a", "/root/shared", 10),
                activity("started", "agent-b", "/root/shared", 20),
                activity("started", "agent-c", "/root/unique", 30),
                close_call("agent-c", "call-failed"),
                close_output("call-failed", r#"{"error":"close failed"}"#),
                close_call("agent-c", "call-non-json"),
                close_output("call-non-json", "not json"),
                close_call("agent-missing", "call-unknown"),
                close_output("call-unknown", r#"{"previous_status":"completed"}"#),
                close_call("/root/shared", "call-ambiguous"),
                close_output("call-ambiguous", r#"{"previous_status":"completed"}"#),
                close_output("call-without-request", r#"{"previous_status":"completed"}"#),
            ],
        )?;

        let mut tracker = tracker_for(&path, "parent-1");
        tracker.refresh()?;

        assert!(tracker.closed_ids().is_empty());
        Ok(())
    }

    #[test]
    fn first_refresh_reads_full_rollout_and_later_append_is_incremental() -> io::Result<()> {
        let dir = tempfile::tempdir()?;
        let path = dir.path().join("rollout.jsonl");
        let mut file = fs::File::create(&path)?;
        writeln!(
            file,
            "{}",
            activity("started", "agent-a", "/root/task-a", 10)
        )?;
        file.write_all(&vec![b'x'; 300 * 1024])?;
        file.write_all(b"\n")?;
        writeln!(
            file,
            "{}",
            activity("started", "agent-b", "/root/task-b", 20)
        )?;

        let mut tracker = tracker_for(&path, "parent-1");
        tracker.refresh()?;
        assert_eq!(
            tracker
                .agents()
                .keys()
                .map(String::as_str)
                .collect::<Vec<_>>(),
            vec!["agent-a", "agent-b"]
        );

        append_lines(
            &path,
            &[
                activity("started", "agent-a", "/root/duplicate", 30),
                activity("started", "agent-c", "/root/task-c", 40),
            ],
        )?;
        tracker.refresh()?;
        assert_eq!(
            tracker
                .agents()
                .keys()
                .map(String::as_str)
                .collect::<Vec<_>>(),
            vec!["agent-a", "agent-b", "agent-c"]
        );
        Ok(())
    }

    #[test]
    fn partial_final_line_is_held_until_newline_arrives() -> io::Result<()> {
        let dir = tempfile::tempdir()?;
        let path = dir.path().join("rollout.jsonl");
        let line = activity("started", "agent-a", "/root/task-a", 10).to_string();
        let split = line.len() / 2;
        fs::write(&path, &line.as_bytes()[..split])?;

        let mut tracker = tracker_for(&path, "parent-1");
        tracker.refresh()?;
        assert!(tracker.agents().is_empty());

        let mut file = OpenOptions::new().append(true).open(&path)?;
        file.write_all(&line.as_bytes()[split..])?;
        file.write_all(b"\n")?;
        tracker.refresh()?;
        assert_eq!(
            tracker
                .agents()
                .keys()
                .map(String::as_str)
                .collect::<Vec<_>>(),
            vec!["agent-a"]
        );
        Ok(())
    }

    #[test]
    fn truncating_or_replacing_file_rebuilds_catalog() -> io::Result<()> {
        let dir = tempfile::tempdir()?;
        let path = dir.path().join("rollout.jsonl");
        write_lines(
            &path,
            &[
                activity("started", "agent-a", "/root/task-a", 10),
                activity("started", "agent-b", "/root/task-b", 20),
            ],
        )?;

        let mut tracker = tracker_for(&path, "parent-1");
        tracker.refresh()?;
        write_lines(&path, &[activity("started", "agent-c", "/root/task-c", 30)])?;
        tracker.refresh()?;

        assert_eq!(
            tracker
                .agents()
                .keys()
                .map(String::as_str)
                .collect::<Vec<_>>(),
            vec!["agent-c"]
        );
        Ok(())
    }

    #[test]
    fn atomic_replacement_longer_than_previous_offset_rebuilds_catalog() -> io::Result<()> {
        let dir = tempfile::tempdir()?;
        let path = dir.path().join("rollout.jsonl");
        write_lines(&path, &[activity("started", "agent-a", "/root/task-a", 10)])?;

        let mut tracker = tracker_for(&path, "parent-1");
        tracker.refresh()?;

        let replacement = dir.path().join("replacement.jsonl");
        write_lines(
            &replacement,
            &[activity("started", "agent-b", "/root/task-b", 20)],
        )?;
        let mut file = OpenOptions::new().append(true).open(&replacement)?;
        writeln!(file, "{}", "x".repeat(1024))?;
        fs::rename(replacement, &path)?;
        tracker.refresh()?;

        assert_eq!(
            tracker
                .agents()
                .keys()
                .map(String::as_str)
                .collect::<Vec<_>>(),
            vec!["agent-b"]
        );
        Ok(())
    }

    #[test]
    fn in_place_rewrite_longer_than_previous_offset_rebuilds_catalog() -> io::Result<()> {
        let dir = tempfile::tempdir()?;
        let path = dir.path().join("rollout.jsonl");
        write_lines(&path, &[activity("started", "agent-a", "/root/task-a", 10)])?;

        let mut tracker = tracker_for(&path, "parent-1");
        tracker.refresh()?;

        let mut file = fs::File::create(&path)?;
        writeln!(
            file,
            "{}",
            activity("started", "agent-b", "/root/task-b", 20)
        )?;
        writeln!(file, "{}", "x".repeat(1024))?;
        tracker.refresh()?;

        assert_eq!(
            tracker
                .agents()
                .keys()
                .map(String::as_str)
                .collect::<Vec<_>>(),
            vec!["agent-b"]
        );
        Ok(())
    }

    #[test]
    fn parent_session_change_clears_and_rebuilds_same_path() -> io::Result<()> {
        let dir = tempfile::tempdir()?;
        let path = dir.path().join("rollout.jsonl");
        write_lines(&path, &[activity("started", "agent-a", "/root/task-a", 10)])?;

        let mut tracker = tracker_for(&path, "parent-1");
        tracker.refresh()?;
        write_lines(&path, &[activity("started", "agent-b", "/root/task-b", 20)])?;
        assert!(tracker.set_context(path.to_str(), Some("parent-2")));
        tracker.refresh()?;

        assert_eq!(
            tracker
                .agents()
                .keys()
                .map(String::as_str)
                .collect::<Vec<_>>(),
            vec!["agent-b"]
        );
        Ok(())
    }

    #[test]
    fn unavailable_transcript_is_a_safe_empty_catalog() -> io::Result<()> {
        let dir = tempfile::tempdir()?;
        let missing = dir.path().join("missing.jsonl");
        let mut tracker = tracker_for(&missing, "parent-1");

        tracker.refresh()?;

        assert!(tracker.agents().is_empty());
        assert!(tracker.closed_ids().is_empty());
        Ok(())
    }
}
