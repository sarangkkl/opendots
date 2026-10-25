use crate::{models::*, runtime::Runtime};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{collections::BTreeSet, sync::Arc};
use tauri::State;
#[derive(Deserialize)]
pub struct ChatInput {
    pub conversation_id: String,
    pub content: String,
    pub request_id: String,
    #[serde(default)]
    pub mentions: Vec<String>,
    pub reply_to: Option<String>,
}
#[derive(Serialize, Deserialize)]
pub struct ChatResult {
    pub message: Message,
    pub executions: Vec<Execution>,
}
impl Runtime {
    pub fn post_chat(&self, input: ChatInput) -> Result<ChatResult> {
        let _guard = self.mutations.lock().unwrap();
        uuid::Uuid::parse_str(&input.request_id).map_err(|_| "Invalid dispatch key")?;
        if input.content.trim().is_empty() || input.content.len() > 100_000 {
            return Err("Message must contain 1–100,000 bytes".into());
        }
        let key = format!("user:{}", input.request_id);
        if let Ok(previous) = self.db.get::<ChatResult>("dispatch", &key) {
            return Ok(previous);
        }
        let group = if let Some(id) = input.conversation_id.strip_prefix("group:") {
            Some(self.db.get::<Group>("group", id)?)
        } else {
            None
        };
        let direct = if let Some(id) = input.conversation_id.strip_prefix("agent:") {
            Some(self.db.get::<Agent>("agent", id)?)
        } else {
            None
        };
        if group.is_none() && direct.is_none() {
            return Err("Invalid conversation".into());
        }
        let parent = match input.reply_to.as_ref() {
            Some(id) => {
                let m: Message = self.db.get("messages", id)?;
                if m.conversation_id != input.conversation_id {
                    return Err("Reply belongs to another conversation".into());
                }
                Some(m)
            }
            None => None,
        };
        let mut recipients: BTreeSet<String> = input
            .mentions
            .iter()
            .filter(|id| id.as_str() != "you")
            .cloned()
            .collect();
        if input.mentions.is_empty() {
            if let Some(a) = direct.as_ref() {
                recipients.insert(a.id.clone());
            } else if let Some(a) = parent.as_ref().and_then(|m| m.agent_id.clone()) {
                recipients.insert(a);
            }
        }
        for target in &recipients {
            if group.as_ref().is_some_and(|g| !g.members.contains(target))
                || direct.as_ref().is_some_and(|a| a.id != *target)
            {
                return Err("Mention is not a conversation member".into());
            }
        }
        if !recipients.is_empty() && group.as_ref().is_some_and(|g| g.paused) {
            return Err("Group is paused".into());
        }
        let message_id = id();
        let thread = parent
            .as_ref()
            .map(|m| m.thread_id.clone().unwrap_or(m.id.clone()));
        let mut message = Message {
            reply_count: 0,
            position: 0,
            id: message_id.clone(),
            conversation_id: input.conversation_id.clone(),
            thread_id: thread.clone(),
            reply_to: input.reply_to,
            mentions: input.mentions,
            attachments: vec![],
            agent_id: None,
            execution_id: None,
            role: "user".into(),
            content: input.content.clone(),
            created_at: now(),
        };
        let prior = parent
            .as_ref()
            .and_then(|m| m.execution_id.as_ref())
            .and_then(|id| self.db.run(id).ok())
            .filter(|r| r.status == "waiting_user");
        let executions: Vec<Execution> = recipients
            .into_iter()
            .map(|agent_id| {
                let run_id = id();
                Execution {
                    id: run_id.clone(),
                    thread_id: Some(thread.clone().unwrap_or(message_id.clone())),
                    agent_id,
                    conversation_id: input.conversation_id.clone(),
                    group_id: group.as_ref().map(|g| g.id.clone()),
                    job_id: prior.as_ref().and_then(|r| r.job_id.clone()),
                    chain_id: prior.as_ref().map(|r| r.chain_id.clone()).unwrap_or(run_id),
                    depth: prior.as_ref().map(|r| r.depth).unwrap_or(0),
                    prompt: input.content.clone(),
                    status: "queued".into(),
                    created_at: now(),
                    updated_at: now(),
                    error: None,
                }
            })
            .collect();
        message.execution_id = executions.first().map(|r| r.id.clone());
        let result = ChatResult {
            message,
            executions,
        };
        self.db.chat_transaction(&key, &result)?;
        self.notify();
        Ok(result)
    }
    pub async fn post_agent_message(&self, run: &Execution, args: Value) -> Result<Value> {
        let content = args["message"]
            .as_str()
            .filter(|s| !s.trim().is_empty())
            .ok_or("Missing message")?;
        let request = args["request_id"].as_str().ok_or("Missing request_id")?;
        let mentions: Vec<String> =
            serde_json::from_value(args.get("mentions").cloned().unwrap_or(json!([])))
                .map_err(|e| e.to_string())?;
        let artifacts: Vec<String> =
            serde_json::from_value(args.get("artifacts").cloned().unwrap_or(json!([])))
                .map_err(|e| e.to_string())?;
        for reference in &artifacts {
            self.browser_file(&run.agent_id, reference)?;
        }
        let mut outputs = vec![];
        for recipient in mentions
            .iter()
            .filter(|s| s.as_str() != "you")
            .collect::<BTreeSet<_>>()
        {
            outputs.push(Box::pin(self.tool(&run.id,"send_to_agent",json!({"agent_id":recipient,"message":content,"request_id":format!("{request}:{recipient}"),"artifacts":artifacts}))).await?);
        }
        if mentions.is_empty() || mentions.iter().any(|s| s == "you") {
            let _guard = self.mutations.lock().unwrap();
            let key = format!("{}:post:{request}", run.id);
            if let Ok(v) = self.db.get::<Value>("agent_post", &key) {
                return Ok(v);
            }
            let m = Message {
                reply_count: 0,
                position: 0,
                id: id(),
                conversation_id: run.conversation_id.clone(),
                thread_id: run.thread_id.clone(),
                reply_to: None,
                mentions: mentions.clone(),
                attachments: artifacts.clone(),
                agent_id: Some(run.agent_id.clone()),
                execution_id: Some(run.id.clone()),
                role: "assistant".into(),
                content: content.into(),
                created_at: now(),
            };
            let waiting = mentions.iter().any(|s| s == "you")
                && args["request_answer"].as_bool().unwrap_or(false);
            let notification=mentions.iter().any(|s|s=="you").then(||json!({"id":m.id,"message_id":m.id,"conversation_id":m.conversation_id,"thread_id":m.thread_id,"agent_id":run.agent_id,"execution_id":run.id,"content":content,"read":false,"status":if waiting{"waiting"}else{"unread"},"created_at":now()}));
            let result = json!({"message_id":m.id,"handoffs":outputs});
            self.db
                .agent_post(run, &m, &key, &result, notification.as_ref(), waiting)?;
            self.notify();
            return Ok(result);
        }
        Ok(json!({"handoffs":outputs}))
    }
}
#[tauri::command]
pub fn post_chat(rt: State<'_, Arc<Runtime>>, input: ChatInput) -> Result<ChatResult> {
    rt.post_chat(input)
}
#[tauri::command]
pub fn history(
    rt: State<'_, Arc<Runtime>>,
    conversation_id: String,
    before: Option<String>,
    thread_id: Option<String>,
) -> Result<Vec<Message>> {
    let messages = rt
        .db
        .history(&conversation_id, before.as_deref(), thread_id.as_deref())?;
    if before.is_none()
        && let Some(last) = messages.last()
    {
        let key = format!(
            "{}:{}",
            conversation_id,
            thread_id.as_deref().unwrap_or("main")
        );
        let previous = rt.db.get::<i64>("read_marker", &key).unwrap_or(0);
        if last.position > previous {
            rt.db.put("read_marker", &key, &last.position)?;
        }
    }
    Ok(messages)
}
#[tauri::command]
pub fn read_notification(rt: State<'_, Arc<Runtime>>, id: String) -> Result<()> {
    let mut n: Value = rt.db.get("notification", &id)?;
    n["read"] = json!(true);
    rt.db.put("notification", &id, &n)?;
    rt.notify();
    Ok(())
}

#[tauri::command]
pub fn chat_message(rt: State<'_, Arc<Runtime>>, id: String) -> Result<Message> {
    rt.db.get("messages", &id)
}
