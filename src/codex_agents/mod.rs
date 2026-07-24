pub mod journal;

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
