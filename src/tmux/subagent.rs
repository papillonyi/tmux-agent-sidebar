use super::types::SubagentInfo;

/// Marker separating the stable subagent identity from its locally observed
/// start time inside `@pane_subagents`.
///
/// Current entry format: `agent_type:agent_id;started_at=<epoch-seconds>`.
/// Legacy `agent_type` and `agent_type:agent_id` entries remain readable so a
/// running sidebar can survive an in-place binary upgrade.
const STARTED_AT_MARKER: &str = ";started_at=";

pub(crate) fn encode_subagent_entry(agent_type: &str, agent_id: &str, started_at: u64) -> String {
    format!("{agent_type}:{agent_id}{STARTED_AT_MARKER}{started_at}")
}

pub(crate) fn subagent_entry_agent_id(entry: &str) -> Option<&str> {
    let (identity, _) = split_started_at(entry);
    identity
        .rsplit_once(':')
        .map(|(_, id)| id)
        .filter(|id| !id.is_empty())
}

pub(super) fn parse_subagent_info(entry: &str) -> SubagentInfo {
    const ID_PREFIX_LEN: usize = 4;

    let (identity, started_at) = split_started_at(entry);
    let label = match identity.rsplit_once(':') {
        Some((agent_type, id)) if !id.is_empty() => {
            let prefix: String = id.chars().take(ID_PREFIX_LEN).collect();
            format!("{agent_type} #{prefix}")
        }
        _ => identity.to_string(),
    };

    SubagentInfo { label, started_at }
}

fn split_started_at(entry: &str) -> (&str, Option<u64>) {
    match entry.rsplit_once(STARTED_AT_MARKER) {
        Some((identity, raw_started_at)) => match raw_started_at.parse() {
            Ok(started_at) => (identity, Some(started_at)),
            Err(_) => (entry, None),
        },
        None => (entry, None),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encode_and_parse_current_entry() {
        let raw = encode_subagent_entry("reviewer", "agent-a81f1234", 1_700_000_000);
        assert_eq!(raw, "reviewer:agent-a81f1234;started_at=1700000000");
        assert_eq!(subagent_entry_agent_id(&raw), Some("agent-a81f1234"));
        assert_eq!(
            parse_subagent_info(&raw),
            SubagentInfo {
                label: "reviewer #agen".into(),
                started_at: Some(1_700_000_000),
            }
        );
    }

    #[test]
    fn legacy_entries_remain_readable() {
        assert_eq!(
            parse_subagent_info("Explore:sub-1234"),
            SubagentInfo {
                label: "Explore #sub-".into(),
                started_at: None,
            }
        );
        assert_eq!(
            parse_subagent_info("Plan"),
            SubagentInfo {
                label: "Plan".into(),
                started_at: None,
            }
        );
    }

    #[test]
    fn agent_type_may_contain_colons() {
        let raw = encode_subagent_entry("superpowers:reviewer", "sub-1234", 42);
        assert_eq!(subagent_entry_agent_id(&raw), Some("sub-1234"));
        assert_eq!(
            parse_subagent_info(&raw).label,
            "superpowers:reviewer #sub-"
        );
    }

    #[test]
    fn malformed_timestamp_is_treated_as_legacy_identity() {
        let raw = "Explore:sub-1;started_at=not-a-number";
        assert_eq!(
            subagent_entry_agent_id(raw),
            Some("sub-1;started_at=not-a-number")
        );
        assert_eq!(parse_subagent_info(raw).started_at, None);
    }
}
