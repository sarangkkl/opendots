use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;

pub type Result<T> = std::result::Result<T, String>;
pub fn id() -> String {
    uuid::Uuid::new_v4().to_string()
}
pub fn now() -> String {
    chrono::Utc::now().to_rfc3339()
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Agent {
    pub id: String,
    pub name: String,
    pub role: String,
    pub instructions: String,
    pub provider: String,
    pub model: String,
    pub executable: String,
    pub workspace: String,
    pub skills: Vec<String>,
    pub integrations: Vec<String>,
    pub policies: BTreeMap<String, String>,
}
impl Default for Agent {
    fn default() -> Self {
        Self {
            id: id(),
            name: String::new(),
            role: String::new(),
            instructions: String::new(),
            provider: "claude".into(),
            model: String::new(),
            executable: String::new(),
            workspace: String::new(),
            skills: vec![],
            integrations: vec![],
            policies: BTreeMap::new(),
        }
    }
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Group {
    pub id: String,
    pub name: String,
    pub purpose: String,
    pub members: Vec<String>,
    pub automatic: bool,
    pub paused: bool,
    pub handoff_limit: u32,
}
impl Default for Group {
    fn default() -> Self {
        Self {
            id: id(),
            name: String::new(),
            purpose: String::new(),
            members: vec![],
            automatic: true,
            paused: false,
            handoff_limit: 20,
        }
    }
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Integration {
    pub id: String,
    pub name: String,
    pub url: String,
    pub command: String,
    pub args: Vec<String>,
    pub has_secret: bool,
}
impl Default for Integration {
    fn default() -> Self {
        Self {
            id: id(),
            name: String::new(),
            url: String::new(),
            command: String::new(),
            args: vec![],
            has_secret: false,
        }
    }
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Job {
    pub id: String,
    pub name: String,
    pub agent_id: String,
    pub group_id: Option<String>,
    pub prompt: String,
    pub timezone: String,
    pub local_time: String,
    pub recurrence: String,
    pub next_at: Option<String>,
    pub paused: bool,
}
impl Default for Job {
    fn default() -> Self {
        Self {
            id: id(),
            name: String::new(),
            agent_id: String::new(),
            group_id: None,
            prompt: String::new(),
            timezone: "UTC".into(),
            local_time: String::new(),
            recurrence: "once".into(),
            next_at: None,
            paused: false,
        }
    }
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Execution {
    pub id: String,
    pub agent_id: String,
    pub conversation_id: String,
    pub group_id: Option<String>,
    pub job_id: Option<String>,
    pub chain_id: String,
    pub depth: u32,
    pub prompt: String,
    pub status: String,
    pub created_at: String,
    pub updated_at: String,
    pub error: Option<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Event {
    pub seq: i64,
    pub execution_id: String,
    pub kind: String,
    pub data: Value,
    pub created_at: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    pub id: String,
    pub conversation_id: String,
    pub agent_id: Option<String>,
    pub execution_id: Option<String>,
    pub role: String,
    pub content: String,
    pub created_at: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Memory {
    pub id: String,
    pub agent_id: String,
    pub content: String,
    pub approved: bool,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Approval {
    pub id: String,
    pub execution_id: String,
    pub kind: String,
    pub payload: Value,
    pub status: String,
    pub created_at: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SendInput {
    pub agent_id: String,
    pub group_id: Option<String>,
    pub prompt: String,
    pub job_id: Option<String>,
}
