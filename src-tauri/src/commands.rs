use crate::{db::active, models::*, providers, runtime::Runtime, schedule};
use serde_json::{Value, json};
use std::sync::Arc;
use tauri::State;

#[tauri::command]
pub fn snapshot(rt: State<'_, Arc<Runtime>>) -> Result<Value> {
    Ok(
        json!({"agents":rt.db.list::<Agent>("agent")?,"groups":rt.db.list::<Group>("group")?,"integrations":rt.db.list::<Integration>("integration")?,"memories":rt.db.list::<Memory>("memory")?,"jobs":rt.db.list::<Job>("job")?,"executions":rt.db.runs()?,"approvals":rt.db.list::<Approval>("approval")?,"occurrences":rt.db.list::<Value>("occurrence")?}),
    )
}
fn valid_id(value: &str) -> Result<()> {
    uuid::Uuid::parse_str(value).map_err(|_| "Invalid resource ID".to_string())?;
    Ok(())
}
fn named(name: &str) -> Result<()> {
    if name.trim().is_empty() || name.len() > 200 {
        return Err("Name must contain 1–200 characters".into());
    }
    Ok(())
}
#[tauri::command]
pub fn save_entity(rt: State<'_, Arc<Runtime>>, kind: String, value: Value) -> Result<()> {
    save(&rt, &kind, value)
}
pub fn save(rt: &Runtime, kind: &str, value: Value) -> Result<()> {
    let entity_id = value["id"].as_str().ok_or("Missing resource ID")?;
    valid_id(entity_id)?;
    let _guard = rt.mutations.lock().unwrap();
    match kind {
        "agent" => {
            let a: Agent = serde_json::from_value(value).map_err(|e| e.to_string())?;
            named(&a.name)?;
            if !matches!(a.provider.as_str(), "claude" | "codex" | "cursor") {
                return Err("Unsupported provider".into());
            }
            if a.policies
                .values()
                .any(|v| !matches!(v.as_str(), "allow" | "ask" | "deny"))
            {
                return Err("Policies must be allow, ask, or deny".into());
            }
            for id in &a.integrations {
                let _: Integration = rt.db.get("integration", id)?;
            }
            if rt
                .db
                .runs()?
                .iter()
                .any(|r| r.agent_id == a.id && active(&r.status))
            {
                return Err(
                    "Finish or cancel this agent's executions before changing its configuration"
                        .into(),
                );
            }
            rt.db.put("agent", &a.id, &a)?;
        }
        "group" => {
            let g: Group = serde_json::from_value(value).map_err(|e| e.to_string())?;
            named(&g.name)?;
            if g.handoff_limit == 0 || g.handoff_limit > 1000 {
                return Err("Handoff limit must be 1–1000".into());
            }
            let unique: std::collections::HashSet<_> = g.members.iter().collect();
            if unique.len() != g.members.len() {
                return Err("Duplicate group member".into());
            }
            for id in &g.members {
                let _: Agent = rt.db.get("agent", id)?;
            }
            if let Ok(old) = rt.db.get::<Group>("group", &g.id)
                && old.members != g.members
                && rt
                    .db
                    .runs()?
                    .iter()
                    .any(|r| r.group_id.as_deref() == Some(&g.id) && active(&r.status))
            {
                return Err("Cancel group executions before changing members".into());
            }
            rt.db.put("group", &g.id, &g)?;
        }
        "integration" => {
            let mut i: Integration = serde_json::from_value(value).map_err(|e| e.to_string())?;
            named(&i.name)?;
            i.has_secret = rt
                .db
                .get::<Integration>("integration", &i.id)
                .map(|old| old.has_secret)
                .unwrap_or(false);
            if i.command.is_empty() == i.url.is_empty() {
                return Err("Provide either a URL or a command".into());
            }
            if !i.url.is_empty() {
                let url = reqwest::Url::parse(&i.url).map_err(|_| "Invalid URL")?;
                if !matches!(url.scheme(), "http" | "https")
                    || !url.username().is_empty()
                    || url.password().is_some()
                {
                    return Err("Use an HTTP(S) URL without embedded credentials; save a bearer token separately".into());
                }
            }
            rt.db.put("integration", &i.id, &i)?;
        }
        "memory" => {
            let m: Memory = serde_json::from_value(value).map_err(|e| e.to_string())?;
            let _: Agent = rt.db.get("agent", &m.agent_id)?;
            if m.content.trim().is_empty() || m.content.len() > 20_000 {
                return Err("Memory must contain 1–20,000 bytes".into());
            }
            rt.db.put("memory", &m.id, &m)?;
        }
        "job" => {
            let mut j: Job = serde_json::from_value(value).map_err(|e| e.to_string())?;
            named(&j.name)?;
            let _: Agent = rt.db.get("agent", &j.agent_id)?;
            if let Some(g) = &j.group_id {
                let group: Group = rt.db.get("group", g)?;
                if !group.members.contains(&j.agent_id) {
                    return Err("Starting agent must belong to the group".into());
                }
            }
            if j.prompt.trim().is_empty() {
                return Err("Job needs a task".into());
            }
            let old = rt.db.get::<Job>("job", &j.id).ok();
            if old.as_ref().is_none_or(|o| {
                o.local_time != j.local_time
                    || o.timezone != j.timezone
                    || o.recurrence != j.recurrence
            }) {
                schedule::validate(&mut j)?;
            } else {
                j.next_at = old.unwrap().next_at;
            }
            rt.db.put("job", &j.id, &j)?;
        }
        _ => return Err("Unsupported resource".into()),
    };
    rt.notify();
    Ok(())
}
#[tauri::command]
pub fn delete_entity(rt: State<'_, Arc<Runtime>>, kind: String, id: String) -> Result<()> {
    let _guard = rt.mutations.lock().unwrap();
    if !matches!(
        kind.as_str(),
        "agent" | "group" | "integration" | "memory" | "job"
    ) {
        return Err("Unsupported resource".into());
    }
    if kind == "agent"
        && rt
            .db
            .list::<crate::browser::Profile>("browser_profile")?
            .iter()
            .any(|p| p.agent_id == id)
    {
        return Err("Delete the agent’s saved browser profile first".into());
    }
    if kind == "agent"
        && (rt
            .db
            .list::<Group>("group")?
            .iter()
            .any(|g| g.members.contains(&id))
            || rt.db.list::<Job>("job")?.iter().any(|j| j.agent_id == id))
    {
        return Err("Remove agent from groups and jobs first".into());
    }
    if kind == "group"
        && rt
            .db
            .list::<Job>("job")?
            .iter()
            .any(|j| j.group_id.as_deref() == Some(&id))
    {
        return Err("Remove group jobs first".into());
    }
    if kind == "integration"
        && rt
            .db
            .list::<Agent>("agent")?
            .iter()
            .any(|a| a.integrations.contains(&id))
    {
        return Err("Unassign this integration from agents first".into());
    }
    if rt.db.runs()?.iter().any(|r| {
        active(&r.status)
            && match kind.as_str() {
                "agent" => r.agent_id == id,
                "group" => r.group_id.as_deref() == Some(&id),
                "job" => r.job_id.as_deref() == Some(&id),
                _ => false,
            }
    }) {
        return Err("Cancel active work before deleting".into());
    }
    rt.db.delete(&kind, &id)?;
    if kind == "integration"
        && let Ok(entry) = keyring::Entry::new("opendots", &id)
    {
        let _ = entry.delete_credential();
    }
    rt.notify();
    Ok(())
}
#[tauri::command]
pub fn messages(rt: State<'_, Arc<Runtime>>, conversation_id: String) -> Result<Vec<Message>> {
    rt.db.messages(&conversation_id)
}
#[tauri::command]
pub fn execution_events(rt: State<'_, Arc<Runtime>>, execution_id: String) -> Result<Vec<Event>> {
    rt.db.events(&execution_id)
}
#[tauri::command]
pub fn send_message(rt: State<'_, Arc<Runtime>>, input: SendInput) -> Result<Execution> {
    let _guard = rt.mutations.lock().unwrap();
    rt.enqueue(input, None)
}
#[tauri::command]
pub fn cancel_execution(rt: State<'_, Arc<Runtime>>, execution_id: String) -> Result<()> {
    rt.cancel(&execution_id)
}
#[tauri::command]
pub fn retry_execution(rt: State<'_, Arc<Runtime>>, execution_id: String) -> Result<Execution> {
    let r = rt.db.run(&execution_id)?;
    if active(&r.status) {
        return Err("Execution is still active".into());
    }
    let _guard = rt.mutations.lock().unwrap();
    rt.enqueue(
        SendInput {
            agent_id: r.agent_id,
            group_id: r.group_id,
            prompt: r.prompt,
            job_id: r.job_id,
        },
        None,
    )
}
#[tauri::command]
pub fn resolve_approval(
    rt: State<'_, Arc<Runtime>>,
    approval_id: String,
    allow: bool,
) -> Result<()> {
    rt.decide(&approval_id, allow)
}
#[tauri::command]
pub fn run_job(rt: State<'_, Arc<Runtime>>, job_id: String) -> Result<Execution> {
    let _guard = rt.mutations.lock().unwrap();
    let j: Job = rt.db.get("job", &job_id)?;
    if rt
        .db
        .runs()?
        .iter()
        .any(|r| r.job_id.as_deref() == Some(&j.id) && active(&r.status))
    {
        return Err("This job already has active work".into());
    }
    rt.enqueue(
        SendInput {
            agent_id: j.agent_id,
            group_id: j.group_id,
            prompt: j.prompt,
            job_id: Some(j.id),
        },
        None,
    )
}
#[tauri::command]
pub async fn discover_provider(provider: String, path: String) -> Value {
    providers::discover(&provider, &path).await
}
#[tauri::command]
pub async fn check_integration(
    rt: State<'_, Arc<Runtime>>,
    integration_id: String,
) -> Result<Value> {
    let i: Integration = rt.db.get("integration", &integration_id)?;
    let mut client = crate::mcp::Client::connect(&i).await?;
    let result = client.request(2, "tools/list", json!({})).await;
    client.close().await;
    result
}
#[tauri::command]
pub fn set_secret(
    rt: State<'_, Arc<Runtime>>,
    integration_id: String,
    secret: String,
) -> Result<()> {
    let mut integration: Integration = rt.db.get("integration", &integration_id)?;
    let entry = keyring::Entry::new("opendots", &integration_id).map_err(|e| e.to_string())?;
    if secret.is_empty() {
        match entry.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => {}
            Err(e) => return Err(e.to_string()),
        }
    } else {
        entry.set_password(&secret).map_err(|e| e.to_string())?;
    }
    integration.has_secret = !secret.is_empty();
    rt.db.put("integration", &integration_id, &integration)?;
    rt.notify();
    Ok(())
}
#[tauri::command]
pub fn import_skill(path: String) -> Result<String> {
    let p = std::path::Path::new(&path);
    if p.extension().and_then(|s| s.to_str()) != Some("md") {
        return Err("Choose a Markdown (.md) file".into());
    }
    let meta = std::fs::metadata(p).map_err(|e| e.to_string())?;
    if meta.len() > 100_000 {
        return Err("Skill exceeds 100 KB".into());
    }
    std::fs::read_to_string(p).map_err(|e| e.to_string())
}
#[tauri::command]
pub fn create_example(rt: State<'_, Arc<Runtime>>) -> Result<String> {
    let _guard = rt.mutations.lock().unwrap();
    let mut members = Vec::new();
    for role in ["Researcher", "Creator", "Publisher"] {
        let a = Agent {
            name: role.into(),
            role: role.into(),
            instructions: format!(
                "You are the {role} in Sira Production. Ask the user to teach you the workflow and assign the required tools before performing real work. Never fabricate media or publishing results."
            ),
            ..Agent::default()
        };
        rt.db.put("agent", &a.id, &a)?;
        members.push(a.id);
    }
    let g=Group{name:"Sira Production".into(),purpose:"Collaborate on content for an AI influencer: research ideas, create media, review, and publish using user-taught procedures.".into(),members,..Group::default()};
    rt.db.put("group", &g.id, &g)?;
    rt.notify();
    Ok(g.id)
}
