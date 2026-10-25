use super::*;
use tauri::State;

#[tauri::command]
pub fn browser_profiles(rt: State<'_, Arc<Runtime>>) -> Result<Value> {
    Ok(
        json!({"profiles":rt.db.list::<Profile>("browser_profile")?,"uploads":rt.db.list::<Value>("browser_upload")?,"executable":cdp::executable("").ok()}),
    )
}
#[tauri::command]
pub async fn save_browser_profile(rt: State<'_, Arc<Runtime>>, profile: Profile) -> Result<()> {
    uuid::Uuid::parse_str(&profile.id).map_err(|_| "Invalid profile ID")?;
    let _: Agent = rt.db.get("agent", &profile.agent_id)?;
    if profile.name.trim().is_empty() {
        return Err("Profile needs a name".into());
    }
    let sessions = rt.browsers.sessions.lock().await;
    if sessions.contains_key(&profile.id) {
        return Err("Close the browser before editing its profile".into());
    }
    for p in rt.db.list::<Profile>("browser_profile")? {
        if p.id != profile.id && p.agent_id == profile.agent_id {
            return Err("Each agent has one separate saved profile".into());
        }
    }
    if let Ok(old) = rt.db.get::<Profile>("browser_profile", &profile.id)
        && old.agent_id != profile.agent_id
    {
        return Err(
            "Saved logins cannot be reassigned to another agent; create a new profile".into(),
        );
    }
    rt.db.put("browser_profile", &profile.id, &profile)?;
    rt.notify();
    Ok(())
}
#[tauri::command]
pub async fn delete_browser_profile(rt: State<'_, Arc<Runtime>>, profile_id: String) -> Result<()> {
    let p: Profile = rt.db.get("browser_profile", &profile_id)?;
    let sessions = rt.browsers.sessions.lock().await;
    if sessions.contains_key(&profile_id) {
        return Err("Close this browser before deleting its saved logins".into());
    }
    let dir = rt.root.join("browser-profiles").join(&p.id);
    if dir.exists() {
        std::fs::remove_dir_all(dir).map_err(|e| e.to_string())?;
    }
    rt.db.delete("browser_profile", &p.id)?;
    rt.notify();
    Ok(())
}
#[tauri::command]
pub async fn browser_control(
    rt: State<'_, Arc<Runtime>>,
    profile_id: String,
    mode: String,
) -> Result<()> {
    rt.browser_manual(&profile_id, &mode).await
}
#[tauri::command]
pub async fn browser_status(rt: State<'_, Arc<Runtime>>, execution_id: String) -> Result<Value> {
    rt.browser_view(&execution_id).await
}
#[tauri::command]
pub fn browser_artifacts(rt: State<'_, Arc<Runtime>>, execution_id: String) -> Result<Value> {
    rt.db.run(&execution_id)?;
    let dir = rt.browser_dir(&execution_id)?;
    let mut files = vec![];
    for sub in ["", "downloads"] {
        let root = dir.join(sub);
        if let Ok(entries) = std::fs::read_dir(root) {
            for entry in entries.flatten() {
                let meta = entry.metadata().map_err(|e| e.to_string())?;
                if meta.is_file() {
                    let name = entry.file_name().to_string_lossy().to_string();
                    files.push(json!({"name":if sub.is_empty(){name.clone()}else{format!("{sub}/{name}")},"bytes":meta.len(),"complete":!name.ends_with(".partial")&&!name.ends_with(".crdownload")}));
                }
            }
        }
    }
    Ok(json!(files))
}
pub(super) fn artifact(rt: &Runtime, execution: &str, name: &str) -> Result<PathBuf> {
    rt.db.run(execution)?;
    let root = rt
        .browser_dir(execution)?
        .canonicalize()
        .map_err(|e| e.to_string())?;
    let file = root.join(name).canonicalize().map_err(|e| e.to_string())?;
    if !file.starts_with(&root) || !file.is_file() {
        return Err("Invalid artifact".into());
    }
    Ok(file)
}
#[tauri::command]
pub fn read_browser_artifact(
    rt: State<'_, Arc<Runtime>>,
    execution_id: String,
    name: String,
) -> Result<Value> {
    let file = artifact(&rt, &execution_id, &name)?;
    if std::fs::metadata(&file).map_err(|e| e.to_string())?.len() > 50_000_000 {
        return Err("Artifact exceeds preview limit; export it instead".into());
    }
    let mime = match file.extension().and_then(|x| x.to_str()) {
        Some("webm") => "video/webm",
        Some("png") => "image/png",
        Some("txt" | "json") => "text/plain",
        _ => return Err("This artifact can be exported but not previewed".into()),
    };
    Ok(json!({"mime":mime,"data":STANDARD.encode(std::fs::read(file).map_err(|e|e.to_string())?)}))
}
#[tauri::command]
pub fn export_browser_artifact(
    rt: State<'_, Arc<Runtime>>,
    execution_id: String,
    name: String,
    destination: String,
) -> Result<()> {
    let file = artifact(&rt, &execution_id, &name)?;
    let mut out = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(destination)
        .map_err(|e| e.to_string())?;
    let mut input = std::fs::File::open(file).map_err(|e| e.to_string())?;
    std::io::copy(&mut input, &mut out).map_err(|e| e.to_string())?;
    Ok(())
}
#[tauri::command]
pub fn delete_browser_artifact(
    rt: State<'_, Arc<Runtime>>,
    execution_id: String,
    name: String,
) -> Result<()> {
    if crate::db::active(&rt.db.run(&execution_id)?.status) {
        return Err("Finish the execution before deleting its artifacts".into());
    }
    std::fs::remove_file(artifact(&rt, &execution_id, &name)?).map_err(|e| e.to_string())
}
#[tauri::command]
pub fn add_browser_upload(
    rt: State<'_, Arc<Runtime>>,
    agent_id: String,
    path: String,
) -> Result<String> {
    let _: Agent = rt.db.get("agent", &agent_id)?;
    let source = std::path::Path::new(&path)
        .canonicalize()
        .map_err(|e| e.to_string())?;
    if !source.is_file() {
        return Err("Select a file".into());
    }
    // Copy the explicitly selected file so later replacement of its path cannot change an approved upload.
    let key = id();
    let relative = format!(
        "inputs/{key}/{}",
        source
            .file_name()
            .ok_or("File has no name")?
            .to_string_lossy()
    );
    rt.import_work_file(&agent_id, &relative, &source)?;
    let dest = rt.work_path(&agent_id)?.join(relative);
    rt.db.put("browser_upload",&key,&json!({"id":key,"agent_id":agent_id,"path":dest,"name":dest.file_name().unwrap().to_string_lossy()}))?;
    rt.notify();
    Ok(key)
}
#[tauri::command]
pub fn replay_browser(rt: State<'_, Arc<Runtime>>, execution_id: String) -> Result<Execution> {
    let source = rt.db.run(&execution_id)?;
    if crate::db::active(&source.status) {
        return Err("Finish the source execution before replay".into());
    }
    let _guard = rt.mutations.lock().unwrap();
    let run = rt.enqueue(
        SendInput {
            thread_id: None,
            agent_id: source.agent_id,
            group_id: source.group_id,
            job_id: None,
            prompt: format!(
                "Explicit browser replay of {execution_id}; stop on ambiguous or uncertain outcomes"
            ),
        },
        None,
    )?;
    rt.db.put("browser_replay", &run.id, &execution_id)?;
    Ok(run)
}
impl Runtime {
    pub async fn browser_replay_run(&self, run: &str, source: &str) -> Result<String> {
        let events = self.db.events(source)?;
        let mut count = 0;
        for (n, event) in events.iter().enumerate() {
            if event.kind != "browser_action_started" {
                continue;
            }
            let finished = events[n + 1..]
                .iter()
                .take_while(|e| e.kind != "browser_action_started")
                .find(|e| e.kind == "browser_action_finished");
            if finished.is_none_or(|e| e.data["ok"] != true) {
                return Err(
                    "Replay stopped at an action with uncertain outcome; review it manually".into(),
                );
            }
            let output = self
                .browser_tool(run, "browser_observe", json!({"screenshot":true}))
                .await?;
            let Content::Text { text } = &output.content[0] else {
                return Err("Missing observation".into());
            };
            let state: Value = serde_json::from_str(text).map_err(|e| e.to_string())?;
            let mut action = event.data["action"].clone();
            action["snapshot"] = state["snapshot"].clone();
            if action.get("index").is_some()
                || (action["action"] == "click_at" && !event.data["fingerprint"].is_null())
            {
                let fingerprint = &event.data["fingerprint"];
                let matches: Vec<_> = state["elements"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter(|e| {
                        e["tag"] == fingerprint["tag"]
                            && e["name"] == fingerprint["name"]
                            && e["type"] == fingerprint["type"]
                            && e["role"] == fingerprint["role"]
                    })
                    .collect();
                if matches.len() != 1 {
                    return Err(
                        "Replay target missing or ambiguous; stopped before interaction".into(),
                    );
                }
                if action["action"] == "click_at" {
                    let old = &event.data["fingerprint"];
                    let new = matches[0];
                    for (axis, size) in [("x", "width"), ("y", "height")] {
                        let relative =
                            (number(&action, axis)? - number(old, axis)?) / number(old, size)?;
                        action[axis] = json!(number(new, axis)? + relative * number(new, size)?);
                    }
                } else {
                    action["index"] = matches[0]["index"].clone();
                }
            } else if matches!(
                action["action"].as_str(),
                Some("click_at" | "drag" | "switch_tab" | "close_tab")
            ) {
                return Err("Coordinate or tab replay requires manual relocation; stopped before interaction".into());
            }
            self.browser_tool(run, "browser_action", action).await?;
            count += 1;
            // Yield for page navigation and resulting observations, never retry an action.
            self.browser_tool(run, "browser_wait", json!({"seconds":1}))
                .await?;
        }
        Ok(format!(
            "Replayed {count} browser actions under current approvals. Review website results."
        ))
    }
}
