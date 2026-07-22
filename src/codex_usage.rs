use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::PathBuf;
use std::time::SystemTime;

/// Maximum amount of an existing transcript read when a sidebar first sees it.
/// Codex rollout files can grow large, while the latest `token_count` event is
/// normally near the end. Later refreshes only read bytes appended after the
/// saved offset.
const INITIAL_TAIL_BYTES: u64 = 256 * 1024;

/// Token totals emitted by Codex in a rollout JSONL `token_count` event.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CodexTokenUsage {
    /// Cumulative tokens consumed by model calls in the current Codex session.
    pub total_tokens: u64,
    /// Tokens in the most recent model call, used as a context-footprint proxy.
    pub context_tokens: u64,
    /// Active model's context-window capacity, when Codex reports it.
    pub model_context_window: u64,
}

impl CodexTokenUsage {
    pub fn context_percent(&self) -> Option<u64> {
        if self.model_context_window == 0 {
            return None;
        }
        let numerator =
            u128::from(self.context_tokens) * 100 + u128::from(self.model_context_window) / 2;
        let percent = numerator / u128::from(self.model_context_window);
        Some(percent.min(u128::from(u64::MAX)) as u64)
    }
}

/// Per-pane cursor for best-effort incremental reads of Codex rollout JSONL.
///
/// The transcript schema is not a stable public hook interface, so every parse
/// is deliberately optional: malformed, missing, or changed records leave the
/// sidebar running and retain the most recent valid usage for the same file.
#[derive(Debug, Clone, Default)]
pub(crate) struct CodexUsageTracker {
    path: Option<PathBuf>,
    offset: u64,
    modified: Option<SystemTime>,
    initialized: bool,
    partial_line: Vec<u8>,
}

impl CodexUsageTracker {
    /// Switch the tracker to `path`. Returns true when the pane moved to a
    /// different transcript and callers should clear the old session's usage.
    pub(crate) fn set_path(&mut self, path: Option<&str>) -> bool {
        let next = path.filter(|value| !value.is_empty()).map(PathBuf::from);
        if self.path == next {
            return false;
        }
        self.path = next;
        self.offset = 0;
        self.modified = None;
        self.initialized = false;
        self.partial_line.clear();
        true
    }

    /// Read newly appended records and return the latest valid usage event.
    /// On the first read (or after truncation/rewrite), only the tail is read.
    pub(crate) fn refresh(&mut self) -> std::io::Result<Option<CodexTokenUsage>> {
        let Some(path) = self.path.as_deref() else {
            return Ok(None);
        };
        let metadata = std::fs::metadata(path)?;
        let len = metadata.len();
        let modified = metadata.modified().ok();

        if self.initialized && self.offset == len && self.modified == modified {
            return Ok(None);
        }

        let reload_tail = !self.initialized
            || len < self.offset
            || (len == self.offset && self.modified != modified);
        let start = if reload_tail {
            len.saturating_sub(INITIAL_TAIL_BYTES)
        } else {
            self.offset
        };

        let mut file = File::open(path)?;
        file.seek(SeekFrom::Start(start))?;
        let mut appended = Vec::new();
        file.read_to_end(&mut appended)?;

        let mut data = if reload_tail {
            self.partial_line.clear();
            if start > 0 {
                // The tail may begin in the middle of a large JSON record.
                // Discard that prefix and start at the next complete line.
                match appended.iter().position(|byte| *byte == b'\n') {
                    Some(newline) => appended.split_off(newline + 1),
                    None => Vec::new(),
                }
            } else {
                appended
            }
        } else {
            let mut combined = std::mem::take(&mut self.partial_line);
            combined.extend_from_slice(&appended);
            combined
        };

        let complete_len = data
            .iter()
            .rposition(|byte| *byte == b'\n')
            .map_or(0, |index| index + 1);
        self.partial_line = data.split_off(complete_len);

        let mut latest = None;
        for line in data.split(|byte| *byte == b'\n') {
            if let Some(usage) = parse_token_count_line(line) {
                latest = Some(usage);
            }
        }

        self.offset = len;
        self.modified = modified;
        self.initialized = true;
        Ok(latest)
    }
}

fn parse_token_count_line(line: &[u8]) -> Option<CodexTokenUsage> {
    if line.is_empty() {
        return None;
    }
    let value: serde_json::Value = serde_json::from_slice(line).ok()?;
    if value.get("type")?.as_str()? != "event_msg" {
        return None;
    }
    let payload = value.get("payload")?;
    if payload.get("type")?.as_str()? != "token_count" {
        return None;
    }
    let info = payload.get("info")?;
    let total_tokens = info
        .get("total_token_usage")?
        .get("total_tokens")?
        .as_u64()?;
    let context_tokens = info
        .get("last_token_usage")
        .and_then(|usage| usage.get("total_tokens"))
        .and_then(serde_json::Value::as_u64)
        .unwrap_or(0);
    let model_context_window = info
        .get("model_context_window")
        .and_then(serde_json::Value::as_u64)
        .unwrap_or(0);

    Some(CodexTokenUsage {
        total_tokens,
        context_tokens,
        model_context_window,
    })
}

pub(crate) fn compact_token_count(tokens: u64) -> String {
    if tokens < 1_000 {
        return tokens.to_string();
    }
    if tokens < 999_950 {
        return format_compact(tokens, 1_000, "k");
    }
    format_compact(tokens, 1_000_000, "m")
}

fn format_compact(tokens: u64, unit: u64, suffix: &str) -> String {
    let tenths = (u128::from(tokens) * 10 + u128::from(unit) / 2) / u128::from(unit);
    let whole = tenths / 10;
    let fraction = tenths % 10;
    if fraction == 0 {
        format!("{whole}{suffix}")
    } else {
        format!("{whole}.{fraction}{suffix}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn token_line(total: u64, last: u64, window: u64) -> String {
        serde_json::json!({
            "timestamp": "2026-07-22T06:41:01Z",
            "type": "event_msg",
            "payload": {
                "type": "token_count",
                "info": {
                    "total_token_usage": { "total_tokens": total },
                    "last_token_usage": { "total_tokens": last },
                    "model_context_window": window
                }
            }
        })
        .to_string()
    }

    #[test]
    fn parses_realistic_token_count_event() {
        let usage = parse_token_count_line(token_line(65_336, 17_682, 258_400).as_bytes())
            .expect("token_count should parse");
        assert_eq!(usage.total_tokens, 65_336);
        assert_eq!(usage.context_tokens, 17_682);
        assert_eq!(usage.model_context_window, 258_400);
        assert_eq!(usage.context_percent(), Some(7));
    }

    #[test]
    fn ignores_unrelated_or_malformed_records() {
        assert!(
            parse_token_count_line(br#"{"type":"event_msg","payload":{"type":"turn_started"}}"#)
                .is_none()
        );
        assert!(parse_token_count_line(b"not json").is_none());
        assert!(parse_token_count_line(b"").is_none());
    }

    #[test]
    fn tracker_reads_initial_value_and_only_new_appends() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("rollout.jsonl");
        std::fs::write(&path, format!("{}\n", token_line(1_000, 800, 10_000))).unwrap();

        let mut tracker = CodexUsageTracker::default();
        assert!(tracker.set_path(path.to_str()));
        assert_eq!(tracker.refresh().unwrap().unwrap().total_tokens, 1_000);
        assert!(tracker.refresh().unwrap().is_none());

        let mut file = std::fs::OpenOptions::new()
            .append(true)
            .open(&path)
            .unwrap();
        writeln!(
            file,
            "{{\"type\":\"event_msg\",\"payload\":{{\"type\":\"other\"}}}}"
        )
        .unwrap();
        writeln!(file, "{}", token_line(2_500, 900, 10_000)).unwrap();
        assert_eq!(tracker.refresh().unwrap().unwrap().total_tokens, 2_500);
    }

    #[test]
    fn tracker_waits_for_partial_jsonl_record_to_complete() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("partial.jsonl");
        let line = token_line(42, 21, 100);
        let split = line.len() / 2;
        std::fs::write(&path, &line.as_bytes()[..split]).unwrap();

        let mut tracker = CodexUsageTracker::default();
        tracker.set_path(path.to_str());
        assert!(tracker.refresh().unwrap().is_none());

        let mut file = std::fs::OpenOptions::new()
            .append(true)
            .open(&path)
            .unwrap();
        file.write_all(&line.as_bytes()[split..]).unwrap();
        file.write_all(b"\n").unwrap();
        assert_eq!(tracker.refresh().unwrap().unwrap().total_tokens, 42);
    }

    #[test]
    fn switching_transcript_resets_incremental_cursor() {
        let dir = tempfile::tempdir().unwrap();
        let first = dir.path().join("first.jsonl");
        let second = dir.path().join("second.jsonl");
        std::fs::write(&first, format!("{}\n", token_line(10, 5, 100))).unwrap();
        std::fs::write(&second, format!("{}\n", token_line(20, 6, 100))).unwrap();

        let mut tracker = CodexUsageTracker::default();
        tracker.set_path(first.to_str());
        assert_eq!(tracker.refresh().unwrap().unwrap().total_tokens, 10);
        assert!(tracker.set_path(second.to_str()));
        assert_eq!(tracker.refresh().unwrap().unwrap().total_tokens, 20);
    }

    #[test]
    fn compact_token_counts_are_stable() {
        assert_eq!(compact_token_count(999), "999");
        assert_eq!(compact_token_count(1_000), "1k");
        assert_eq!(compact_token_count(65_336), "65.3k");
        assert_eq!(compact_token_count(999_999), "1m");
        assert_eq!(compact_token_count(1_250_000), "1.3m");
    }

    #[test]
    fn missing_context_window_has_no_percentage() {
        let usage = CodexTokenUsage {
            total_tokens: 1,
            context_tokens: 1,
            model_context_window: 0,
        };
        assert_eq!(usage.context_percent(), None);
    }
}
