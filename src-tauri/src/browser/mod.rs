mod cdp;
pub mod commands;
mod recording;
#[cfg(test)]
mod tests;
use crate::{models::*, process::Process, runtime::Runtime};
use base64::{Engine, engine::general_purpose::STANDARD};
use cdp::Cdp;
use recording::Capture;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    collections::HashMap,
    path::PathBuf,
    sync::{Arc, atomic::Ordering},
};
use tokio::sync::Mutex;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Profile {
    pub id: String,
    pub agent_id: String,
    pub name: String,
    #[serde(default)]
    pub executable: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum Content {
    Text {
        text: String,
    },
    Image {
        data: String,
        #[serde(rename = "mimeType")]
        mime_type: String,
    },
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolOutput {
    pub content: Vec<Content>,
}
impl ToolOutput {
    pub fn text(value: Value) -> Self {
        Self {
            content: vec![Content::Text {
                text: value.to_string(),
            }],
        }
    }
    pub fn codex(&self) -> Vec<Value> {
        self.content.iter().map(|c|match c { Content::Text{text}=>json!({"type":"inputText","text":text}), Content::Image{data,mime_type}=>json!({"type":"inputImage","imageUrl":format!("data:{mime_type};base64,{data}")}) }).collect()
    }
}
#[derive(Default)]
pub struct Browsers {
    sessions: Mutex<HashMap<String, Arc<Mutex<Session>>>>,
}
struct Session {
    process: Process,
    cdp: Cdp,
    tab: String,
    target: String,
    owner: String,
    snapshot: Option<Value>,
    capture: Option<Capture>,
    manual: bool,
}
pub const ACTIONS: &[&str] = &[
    "navigate",
    "click",
    "click_at",
    "type",
    "keys",
    "scroll",
    "drag",
    "select",
    "new_tab",
    "switch_tab",
    "close_tab",
    "dialog",
    "upload",
    "download",
    "back",
];
pub fn tools() -> Vec<Value> {
    vec![
        json!({"name":"browser_artifacts","description":"List files available to this agent for upload or explicit group handoff. Use reference strings; never guess filesystem paths.","inputSchema":{"type":"object","properties":{},"additionalProperties":false}}),
        json!({"name":"browser_observe","description":"Observe this agent's dedicated Chrome profile. Automatically launches a stopped browser or relaunches an exited browser using its saved profile; do not ask the user to reset it. Observe again after a restart; never retry a submission automatically. Returns indexed elements, tabs, snapshot and optionally an image. Always refresh after any action. Inaccessible cross-origin frames are explicitly reported.","inputSchema":{"type":"object","properties":{"screenshot":{"type":"boolean"}},"additionalProperties":false}}),
        json!({"name":"browser_action","description":"One approved browser interaction. Supply the latest snapshot. Coordinate values use CSS viewport pixels from observation; screenshot output uses the same dimensions. Unknown actions default to ask. Never retry a submission with an unknown outcome. Password entry requires human takeover.","inputSchema":{"type":"object","properties":{"action":{"type":"string","enum":ACTIONS},"snapshot":{"type":"string"},"index":{"type":"integer"},"text":{"type":"string"},"url":{"type":"string"},"x":{"type":"number"},"y":{"type":"number"},"to_x":{"type":"number"},"to_y":{"type":"number"},"tab":{"type":"string"},"accept":{"type":"boolean"},"artifact":{"type":"string"}},"required":["action","snapshot"],"additionalProperties":false}}),
        json!({"name":"browser_wait","description":"Wait up to 30 seconds without interacting with a page.","inputSchema":{"type":"object","properties":{"seconds":{"type":"integer","minimum":1,"maximum":30}},"required":["seconds"],"additionalProperties":false}}),
    ]
}
fn text<'a>(v: &'a Value, k: &str) -> Result<&'a str> {
    v[k].as_str().ok_or_else(|| format!("Missing {k}"))
}
fn number(v: &Value, k: &str) -> Result<f64> {
    v[k].as_f64()
        .filter(|n| n.is_finite())
        .ok_or_else(|| format!("Invalid {k}"))
}
fn url(value: &str) -> Result<()> {
    let u = reqwest::Url::parse(value).map_err(|_| "Invalid URL")?;
    if !matches!(u.scheme(), "http" | "https") || !u.username().is_empty() || u.password().is_some()
    {
        return Err("Only HTTP(S) URLs without credentials are supported".into());
    }
    Ok(())
}
impl Runtime {
    pub fn browser_profile(&self, agent: &str) -> Result<Profile> {
        self.db
            .list::<Profile>("browser_profile")?
            .into_iter()
            .find(|p| p.agent_id == agent)
            .ok_or("Assign a dedicated browser profile to this agent in Settings".into())
    }
    async fn browser_session(&self, profile: &Profile, owner: &str) -> Result<Arc<Mutex<Session>>> {
        // Serialize launch to prevent two Chromes opening the same profile.
        let mut sessions = self.browsers.sessions.lock().await;
        let mut restarted = false;
        if let Some(session) = sessions.get(&profile.id).cloned() {
            let mut old = session.lock().await;
            if old
                .process
                .child
                .try_wait()
                .map_err(|e| e.to_string())?
                .is_none()
            {
                return Ok(session.clone());
            }
            // An exited process must not keep a permanent lease or stale approval.
            old.snapshot = None;
            for mut approval in self.db.list::<Approval>("approval")? {
                if approval.execution_id == old.owner
                    && approval.kind == "browser"
                    && approval.status == "pending"
                {
                    approval.status = "expired".into();
                    self.db.put("approval", &approval.id, &approval)?;
                }
            }
            if let Some(capture) = old.capture.take() {
                capture.finish().await;
            }
            old.process.stop().await;
            sessions.remove(&profile.id);
            restarted = true;
        }
        if owner != "manual" && self.cancelled(owner) {
            return Err("Execution cancelled".into());
        }
        let exe = cdp::executable(&profile.executable)?;
        let dir = self.root.join("browser-profiles").join(&profile.id);
        let (process, cdp) = cdp::launch(&exe, &dir, cfg!(test)).await?;
        let targets = cdp.call("", "Target.getTargets", json!({})).await?;
        let target = targets["targetInfos"]
            .as_array()
            .and_then(|a| a.iter().find(|t| t["type"] == "page"))
            .and_then(|t| t["targetId"].as_str())
            .ok_or("Browser has no page")?
            .to_owned();
        let attached = cdp
            .call(
                "",
                "Target.attachToTarget",
                json!({"targetId":target,"flatten":true}),
            )
            .await?;
        let tab = text(&attached, "sessionId")?.to_owned();
        cdp.call(&tab, "Page.enable", json!({})).await?;
        let session = Arc::new(Mutex::new(Session {
            process,
            cdp,
            tab,
            target,
            owner: owner.into(),
            snapshot: None,
            capture: None,
            manual: owner == "manual",
        }));
        sessions.insert(profile.id.clone(), session.clone());
        if owner != "manual" {
            self.event(
                owner,
                if restarted {
                    "browser_restarted"
                } else {
                    "browser_launched"
                },
                json!({"profile_id":profile.id,"saved_logins_preserved":true}),
            );
        }
        Ok(session)
    }
    async fn acquire_browser(&self, run: &Execution) -> Result<Arc<Mutex<Session>>> {
        let profile = self.browser_profile(&run.agent_id)?;
        let mut session = self.browser_session(&profile, &run.id).await?;
        let mut recoveries = 0;
        let mut announced = false;
        loop {
            if self.cancelled(&run.id) {
                return Err("Execution cancelled".into());
            }
            let mut s = session.lock().await;
            if s.process
                .child
                .try_wait()
                .map_err(|e| e.to_string())?
                .is_some()
            {
                if recoveries >= 1 {
                    return Err("Browser repeatedly exited during startup. Check the browser executable and profile lock; no action was retried.".into());
                }
                drop(s);
                session = self.browser_session(&profile, &run.id).await?;
                recoveries += 1;
                continue;
            }
            if !s.manual && (s.owner.is_empty() || s.owner == run.id) {
                s.owner = run.id.clone();
                let download_dir = self.browser_dir(&run.id).join("downloads");
                std::fs::create_dir_all(&download_dir).map_err(|e| e.to_string())?;
                s.cdp.call("","Browser.setDownloadBehavior",json!({"behavior":"allowAndName","downloadPath":download_dir,"eventsEnabled":true})).await?;
                if s.capture.is_none() {
                    match Capture::start(s.cdp.clone(), s.tab.clone(), self.browser_dir(&run.id))
                        .await
                    {
                        Ok(c) => s.capture = Some(c),
                        Err(e) => self.event(&run.id, "recording_error", json!({"error":e})),
                    }
                }
                return Ok(session.clone());
            }
            drop(s);
            if !announced {
                self.event(
                    &run.id,
                    "browser_waiting",
                    json!({"reason":"Profile is in manual use or owned by another execution"}),
                );
                announced = true;
            }
            tokio::time::sleep(std::time::Duration::from_millis(200)).await;
        }
    }
    pub fn browser_file(&self, agent: &str, reference: &str) -> Result<PathBuf> {
        if let Some(rest) = reference.strip_prefix("browser:") {
            let (run, name) = rest.split_once('/').ok_or("Invalid artifact reference")?;
            let source = self.db.run(run)?;
            if source.agent_id != agent
                && self
                    .db
                    .get::<bool>("browser_share", &format!("{agent}:{reference}"))
                    != Ok(true)
            {
                return Err("Artifact was not explicitly shared with this agent".into());
            }
            let file = commands::artifact(self, run, name)?;
            if matches!(
                file.extension().and_then(|s| s.to_str()),
                Some("partial" | "crdownload")
            ) {
                return Err("Artifact is still being written".into());
            }
            Ok(file)
        } else {
            let record: Value = self.db.get("browser_upload", reference)?;
            if record["agent_id"] != agent {
                return Err("Upload was not assigned to this agent".into());
            }
            let path = PathBuf::from(text(&record, "path")?);
            if !path.is_file() {
                return Err("Upload no longer exists".into());
            }
            Ok(path)
        }
    }
    pub fn browser_files(&self, agent: &str) -> Result<Value> {
        let mut items: Vec<Value> = self
            .db
            .list::<Value>("browser_upload")?
            .into_iter()
            .filter(|v| v["agent_id"] == agent)
            .map(|v| json!({"reference":v["id"],"name":v["name"]}))
            .collect();
        for run in self.db.runs()?.into_iter().take(200) {
            for sub in ["", "downloads"] {
                if let Ok(entries) = std::fs::read_dir(self.browser_dir(&run.id).join(sub)) {
                    for entry in entries.flatten() {
                        if !entry.path().is_file() {
                            continue;
                        }
                        let name = entry.file_name().to_string_lossy().to_string();
                        let relative = if sub.is_empty() {
                            name.clone()
                        } else {
                            format!("{sub}/{name}")
                        };
                        let reference = format!("browser:{}/{relative}", run.id);
                        if self.browser_file(agent, &reference).is_ok() {
                            items.push(
                                json!({"reference":reference,"name":name,"execution_id":run.id}),
                            );
                        }
                    }
                }
            }
        }
        Ok(json!(items))
    }
    pub fn browser_dir(&self, run: &str) -> PathBuf {
        self.root.join("browser-artifacts").join(run)
    }
    pub async fn browser_release(&self, run: &str) {
        let sessions: Vec<_> = self
            .browsers
            .sessions
            .lock()
            .await
            .values()
            .cloned()
            .collect();
        for session in sessions {
            let mut s = session.lock().await;
            if s.owner == run {
                if let Some(c) = s.capture.take() {
                    c.finish().await;
                }
                s.owner.clear();
                s.snapshot = None;
            }
        }
        self.notify();
    }
    pub async fn browser_shutdown(&self) {
        let sessions = std::mem::take(&mut *self.browsers.sessions.lock().await);
        for (_, session) in sessions {
            let mut s = session.lock().await;
            if let Some(c) = s.capture.take() {
                c.finish().await;
            }
            let _ = s.cdp.call("", "Browser.close", json!({})).await;
            let _ = tokio::time::timeout(std::time::Duration::from_secs(5), s.process.child.wait())
                .await;
            s.process.stop().await;
        }
    }
    pub async fn browser_manual(&self, profile_id: &str, mode: &str) -> Result<()> {
        let p: Profile = self.db.get("browser_profile", profile_id)?;
        let session = if mode == "close" {
            match self.browsers.sessions.lock().await.get(profile_id).cloned() {
                Some(session) => session,
                None => return Ok(()),
            }
        } else {
            self.browser_session(&p, "manual").await?
        };
        let mut s = session.lock().await;
        match mode {
            "login" | "takeover" => {
                s.manual = true;
                s.snapshot = None;
                if let Some(c) = s.capture.take() {
                    c.paused.store(true, Ordering::SeqCst);
                    c.finish().await;
                }
                for mut a in self.db.list::<Approval>("approval")? {
                    if a.execution_id == s.owner && a.kind == "browser" && a.status == "pending" {
                        a.status = "expired".into();
                        self.db.put("approval", &a.id, &a)?;
                    }
                }
                if s.owner != "manual" && !s.owner.is_empty() {
                    self.event(&s.owner, "browser_takeover", json!({"recording":false}));
                }
                s.cdp
                    .call("", "Target.activateTarget", json!({"targetId":s.target}))
                    .await?;
            }
            "resume" => {
                s.manual = false;
                s.snapshot = None;
                if s.owner == "manual" {
                    s.owner.clear();
                }
            }
            "close" => {
                if !s.owner.is_empty()
                    && s.owner != "manual"
                    && self
                        .db
                        .run(&s.owner)
                        .is_ok_and(|r| crate::db::active(&r.status))
                {
                    return Err("Cancel the execution before closing its browser".into());
                }
                if let Some(c) = s.capture.take() {
                    c.finish().await;
                }
                let _ = s.cdp.call("", "Browser.close", json!({})).await;
                let _ =
                    tokio::time::timeout(std::time::Duration::from_secs(5), s.process.child.wait())
                        .await;
                s.process.stop().await;
                drop(s);
                self.browsers.sessions.lock().await.remove(profile_id);
            }
            _ => return Err("Unknown browser control".into()),
        }
        self.notify();
        Ok(())
    }
    pub async fn browser_view(&self, run_id: &str) -> Result<Value> {
        let run = self.db.run(run_id)?;
        let profile = self.browser_profile(&run.agent_id)?;
        let session = self
            .browsers
            .sessions
            .lock()
            .await
            .get(&profile.id)
            .cloned();
        if let Some(session) = session {
            let s = session.lock().await;
            Ok(
                json!({"profile":profile,"manual":s.manual,"owner":s.owner,"tab":s.target,"frame":if s.owner==run_id && !s.manual { s.capture.as_ref().map(|c|c.latest.lock().unwrap().clone()) } else { None }}),
            )
        } else {
            Ok(json!({"profile":profile}))
        }
    }
    async fn observe_locked(
        &self,
        s: &mut Session,
        run: &str,
        screenshot: bool,
    ) -> Result<ToolOutput> {
        let dialog = s.cdp.dialogs.lock().unwrap().get(&s.tab).cloned();
        let mut state = if let Some(dialog) = dialog {
            json!({"url":dialog["url"],"title":"Browser dialog", "dialog":dialog,"elements":[],"viewport":{"width":0,"height":0}})
        } else {
            s.cdp
                .eval(&s.tab, include_str!("observe.js").into())
                .await?
        };
        state["snapshot"] = json!(id());
        state["tab"] = json!(s.target);
        let tabs = s.cdp.call("", "Target.getTargets", json!({})).await?;
        state["tabs"] = json!(
            tabs["targetInfos"]
                .as_array()
                .into_iter()
                .flatten()
                .filter(|v| v["type"] == "page")
                .map(|v| json!({"id":v["targetId"],"title":v["title"],"url":v["url"]}))
                .collect::<Vec<_>>()
        );
        s.snapshot = Some(state.clone());
        let mut output = ToolOutput::text(state.clone());
        if screenshot && state.get("dialog").is_none() {
            let width = state["viewport"]["width"].as_f64().unwrap_or(1280.0);
            let height = state["viewport"]["height"].as_f64().unwrap_or(720.0);
            let shot=s.cdp.call(&s.tab,"Page.captureScreenshot",json!({"format":"png","captureBeyondViewport":false,"clip":{"x":state["viewport"]["scrollX"],"y":state["viewport"]["scrollY"],"width":width,"height":height,"scale":1}})).await?;
            s.snapshot.as_mut().unwrap()["screenshot_hash"] =
                json!(image_hash(text(&shot, "data")?));
            let raw = STANDARD
                .decode(text(&shot, "data")?)
                .map_err(|e| e.to_string())?;
            // Normalize to CSS pixels regardless of display device scale.
            let decoded = image::load_from_memory(&raw)
                .map_err(|e| e.to_string())?
                .resize_exact(
                    width as u32,
                    height as u32,
                    image::imageops::FilterType::Triangle,
                );
            let mut bytes = std::io::Cursor::new(Vec::new());
            decoded
                .write_to(&mut bytes, image::ImageFormat::Png)
                .map_err(|e| e.to_string())?;
            let name = format!("screenshot-{}.png", id());
            let dir = self.browser_dir(run);
            std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
            std::fs::write(dir.join(&name), bytes.get_ref()).map_err(|e| e.to_string())?;
            output.content.push(Content::Image {
                data: STANDARD.encode(bytes.into_inner()),
                mime_type: "image/png".into(),
            });
            self.event(
                run,
                "browser_screenshot",
                json!({"artifact":name,"snapshot":state["snapshot"]}),
            );
        }
        Ok(output)
    }
    pub async fn browser_tool(&self, run_id: &str, name: &str, args: Value) -> Result<ToolOutput> {
        let run = self.db.run(run_id)?;
        if !crate::db::active(&run.status) || self.cancelled(run_id) {
            return Err("Execution is not active".into());
        }
        if name == "browser_artifacts" {
            return Ok(ToolOutput::text(self.browser_files(&run.agent_id)?));
        }
        if name == "browser_wait" {
            let seconds = args["seconds"]
                .as_u64()
                .filter(|s| *s > 0 && *s <= 30)
                .ok_or("Wait must be 1–30 seconds")?;
            for _ in 0..seconds * 10 {
                if self.cancelled(run_id) {
                    return Err("Execution cancelled".into());
                }
                tokio::time::sleep(std::time::Duration::from_millis(100)).await;
            }
            return Ok(ToolOutput::text(json!({"waited":seconds})));
        }
        let session = self.acquire_browser(&run).await?;
        if name == "browser_observe" {
            let mut s = session.lock().await;
            return self
                .observe_locked(&mut s, run_id, args["screenshot"].as_bool().unwrap_or(true))
                .await;
        }
        let action = text(&args, "action")?;
        if !ACTIONS.contains(&action) {
            return Err("Unsupported browser action".into());
        }
        let before = {
            let s = session.lock().await;
            if s.manual {
                return Err("Browser is in manual control".into());
            }
            let before = s.snapshot.clone().ok_or("Observe the browser first")?;
            if before["snapshot"] != args["snapshot"] {
                return Err("Stale snapshot; observe again".into());
            }
            before
        };
        if let Some(index) = args["index"].as_u64() {
            let target = before["elements"]
                .as_array()
                .and_then(|a| a.get(index as usize))
                .ok_or("Invalid element index")?;
            if target["secret"] == true {
                return Err("Password fields require manual takeover; secrets must not enter tool arguments".into());
            }
        }
        let coordinate_target =
            if matches!(action, "click_at" | "drag") && args.get("index").is_none() {
                let (x, y) = (number(&args, "x")?, number(&args, "y")?);
                before["elements"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .rev()
                    .find(|e| {
                        let (ex, ey, w, h) = (
                            e["x"].as_f64().unwrap_or(-1.),
                            e["y"].as_f64().unwrap_or(-1.),
                            e["width"].as_f64().unwrap_or(0.),
                            e["height"].as_f64().unwrap_or(0.),
                        );
                        x >= ex && x < ex + w && y >= ey && y < ey + h
                    })
                    .cloned()
            } else {
                None
            };
        if matches!(action, "click_at" | "drag") && before.get("screenshot_hash").is_none() {
            return Err("Coordinate interactions require a fresh screenshot observation".into());
        }
        let agent: Agent = self.db.get("agent", &run.agent_id)?;
        match agent.policies.get(&format!("browser:{action}")).map(String::as_str).unwrap_or("ask") {
            "deny"=>return Err("Browser action denied by agent policy".into()),
            "allow"=>{},
            _=>self.approve(run_id,"browser",json!({"agent":agent.name,"site":before["url"],"dialog":before["dialog"],"action":args,"target":args["index"].as_u64().and_then(|i|before["elements"].get(i as usize))})).await?
        }
        let mut s = session.lock().await;
        if self.cancelled(run_id) || s.manual || s.owner != run_id {
            return Err("Browser action cancelled or control changed".into());
        }
        if s.snapshot.as_ref().map(|v| &v["snapshot"]) != Some(&args["snapshot"]) {
            return Err("Snapshot changed during approval; observe again".into());
        }
        let dialog = s.cdp.dialogs.lock().unwrap().get(&s.tab).cloned();
        if action == "dialog" {
            if dialog.as_ref() != before.get("dialog") || dialog.is_none() {
                return Err("Dialog changed; observe and approve again".into());
            }
        } else {
            if dialog.is_some() {
                return Err("Browser has an open dialog; observe and handle it first".into());
            }
            let current=s.cdp.eval(&s.tab,"({url:location.href,documentId:window.__openDots?.documentId,revision:window.__openDots?.revision,scrollX,scrollY,width:innerWidth,height:innerHeight})".into()).await?;
            if current["url"] != before["url"]
                || current["documentId"] != before["documentId"]
                || current["revision"] != before["revision"]
                || current["scrollX"] != before["viewport"]["scrollX"]
                || current["scrollY"] != before["viewport"]["scrollY"]
                || current["width"] != before["viewport"]["width"]
                || current["height"] != before["viewport"]["height"]
            {
                s.snapshot = None;
                return Err("Page changed during approval; observe and approve again".into());
            }
            if let Some(index) = args["index"]
                .as_u64()
                .or_else(|| coordinate_target.as_ref().and_then(|e| e["index"].as_u64()))
                && s.cdp
                    .eval(&s.tab, format!("{}({index})", include_str!("validate.js")))
                    .await?
                    != true
            {
                return Err("Target moved or disappeared; observe and approve again".into());
            }
            if matches!(action, "click_at" | "drag") {
                let v = &before["viewport"];
                let shot=s.cdp.call(&s.tab,"Page.captureScreenshot",json!({"format":"png","captureBeyondViewport":false,"clip":{"x":v["scrollX"],"y":v["scrollY"],"width":v["width"],"height":v["height"],"scale":1}})).await?;
                if json!(image_hash(text(&shot, "data")?)) != before["screenshot_hash"] {
                    return Err(
                        "Screenshot changed during approval; observe and approve again".into(),
                    );
                }
            }
            if action == "keys"
                && s.cdp
                    .eval(&s.tab, "document.activeElement?.type==='password'".into())
                    .await?
                    == true
            {
                return Err("Password fields require manual takeover".into());
            }
        }
        s.snapshot = None;
        self.event(run_id,"browser_action_started",json!({"action":args,"url":before["url"],"fingerprint":args["index"].as_u64().and_then(|i|before["elements"].get(i as usize)).or(coordinate_target.as_ref())}));
        let result = self.perform(&mut s, run_id, &args, &before).await;
        self.event(
            run_id,
            "browser_action_finished",
            json!({"action":action,"ok":result.is_ok(),"error":result.as_ref().err()}),
        );
        result.map(ToolOutput::text)
    }
    async fn perform(
        &self,
        s: &mut Session,
        run: &str,
        a: &Value,
        before: &Value,
    ) -> Result<Value> {
        let action = text(a, "action")?;
        let index = a["index"].as_u64();
        let point = || -> Result<(f64, f64)> {
            let (x, y) = if let Some(i) = index {
                let e = &before["elements"][i as usize];
                (
                    number(e, "x")? + number(e, "width")? / 2.,
                    number(e, "y")? + number(e, "height")? / 2.,
                )
            } else {
                (number(a, "x")?, number(a, "y")?)
            };
            if x < 0.
                || y < 0.
                || x >= number(&before["viewport"], "width")?
                || y >= number(&before["viewport"], "height")?
            {
                return Err("Target is outside viewport; scroll and observe first".into());
            }
            Ok((x, y))
        };
        let element = || -> Result<String> {
            let i = index.ok_or("Element index required")?;
            Ok(format!("window.__openDots.elements[{i}].element"))
        };
        match action {
            "navigate" | "download" => {
                let dest = text(a, "url")?;
                url(dest)?;
                if action == "download" {
                    let dir = self.browser_dir(run).join("downloads");
                    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
                    s.cdp.call("","Browser.setDownloadBehavior",json!({"behavior":"allowAndName","downloadPath":dir,"eventsEnabled":true})).await?;
                }
                let v = s
                    .cdp
                    .call(&s.tab, "Page.navigate", json!({"url":dest}))
                    .await?;
                if let Some(error) = v["errorText"].as_str()
                    && !(action == "download" && error == "net::ERR_ABORTED")
                {
                    return Err(error.into());
                }
            }
            "click" | "click_at" => {
                let (x, y) = point()?;
                mouse(&s.cdp, &s.tab, "mousePressed", x, y, "left", 1).await?;
                mouse(&s.cdp, &s.tab, "mouseReleased", x, y, "left", 1).await?;
            }
            "type" => {
                let e = element()?;
                s.cdp.eval(&s.tab,format!("(()=>{{const e={e};if(e.type==='password')throw Error('secret');e.focus();return true}})() ")).await?;
                s.cdp
                    .call(&s.tab, "Input.insertText", json!({"text":text(a,"text")?}))
                    .await?;
            }
            "keys" => {
                let key = text(a, "text")?;
                let code = match key {
                    "Enter" => 13,
                    "Tab" => 9,
                    "Escape" => 27,
                    "Backspace" => 8,
                    "Delete" => 46,
                    "ArrowLeft" => 37,
                    "ArrowUp" => 38,
                    "ArrowRight" => 39,
                    "ArrowDown" => 40,
                    "Home" => 36,
                    "End" => 35,
                    _ => return Err(
                        "Supported keys: Enter, Tab, Escape, Backspace, Delete, arrows, Home, End"
                            .into(),
                    ),
                };
                for event in ["keyDown", "keyUp"] {
                    s.cdp
                        .call(
                            &s.tab,
                            "Input.dispatchKeyEvent",
                            json!({"type":event,"key":key,"windowsVirtualKeyCode":code}),
                        )
                        .await?;
                }
            }
            "scroll" => {
                s.cdp.call(&s.tab,"Input.dispatchMouseEvent",json!({"type":"mouseWheel","x":number(&before["viewport"],"width")?/2.,"y":number(&before["viewport"],"height")?/2.,"deltaX":a["x"].as_f64().unwrap_or(0.),"deltaY":number(a,"y")?})).await?;
            }
            "drag" => {
                let (x, y) = point()?;
                let (tx, ty) = (number(a, "to_x")?, number(a, "to_y")?);
                mouse(&s.cdp, &s.tab, "mouseMoved", x, y, "none", 0).await?;
                mouse(&s.cdp, &s.tab, "mousePressed", x, y, "left", 1).await?;
                for i in 1..=10 {
                    mouse(
                        &s.cdp,
                        &s.tab,
                        "mouseMoved",
                        x + (tx - x) * i as f64 / 10.,
                        y + (ty - y) * i as f64 / 10.,
                        "left",
                        0,
                    )
                    .await?;
                }
                mouse(&s.cdp, &s.tab, "mouseReleased", tx, ty, "left", 1).await?;
            }
            "select" => {
                let e = element()?;
                let value = serde_json::to_string(text(a, "text")?).unwrap();
                s.cdp.eval(&s.tab,format!("(()=>{{const e={e};if(e.tagName!=='SELECT')throw Error('not select');e.value={value};e.dispatchEvent(new Event('input',{{bubbles:true}}));e.dispatchEvent(new Event('change',{{bubbles:true}}));return e.value;}})()")).await?;
            }
            "upload" => {
                let e = element()?;
                let artifact = text(a, "artifact")?;
                let file = self.browser_file(&self.db.run(run)?.agent_id, artifact)?;
                let node = s
                    .cdp
                    .call(
                        &s.tab,
                        "Runtime.evaluate",
                        json!({"expression":e,"returnByValue":false}),
                    )
                    .await?;
                s.cdp
                    .call(
                        &s.tab,
                        "DOM.setFileInputFiles",
                        json!({"objectId":node["result"]["objectId"],"files":[file]}),
                    )
                    .await?;
            }
            "dialog" => {
                s.cdp.call(&s.tab,"Page.handleJavaScriptDialog",json!({"accept":a["accept"].as_bool().unwrap_or(false),"promptText":a["text"].as_str().unwrap_or("")})).await?;
            }
            "back" => {
                let h = s
                    .cdp
                    .call(&s.tab, "Page.getNavigationHistory", json!({}))
                    .await?;
                let i = h["currentIndex"].as_u64().ok_or("No history")?;
                if i == 0 {
                    return Err("No earlier page".into());
                }
                s.cdp
                    .call(
                        &s.tab,
                        "Page.navigateToHistoryEntry",
                        json!({"entryId":h["entries"][(i-1) as usize]["id"]}),
                    )
                    .await?;
            }
            "new_tab" | "switch_tab" | "close_tab" => {
                let target = if action == "new_tab" {
                    let dest = a["url"].as_str().unwrap_or("https://example.com");
                    url(dest)?;
                    let t = s
                        .cdp
                        .call("", "Target.createTarget", json!({"url":dest}))
                        .await?;
                    text(&t, "targetId")?.to_owned()
                } else {
                    text(a, "tab")?.to_owned()
                };
                if action == "close_tab" {
                    if target == s.target {
                        return Err("Switch to another tab before closing the current tab".into());
                    }
                    s.cdp
                        .call("", "Target.closeTarget", json!({"targetId":target}))
                        .await?;
                } else {
                    if let Some(c) = s.capture.take() {
                        c.finish().await;
                    }
                    let attached = s
                        .cdp
                        .call(
                            "",
                            "Target.attachToTarget",
                            json!({"targetId":target,"flatten":true}),
                        )
                        .await?;
                    s.tab = text(&attached, "sessionId")?.into();
                    s.target = target;
                    s.cdp.call(&s.tab, "Page.enable", json!({})).await?;
                    s.cdp
                        .call("", "Target.activateTarget", json!({"targetId":s.target}))
                        .await?;
                    s.capture = Some(
                        Capture::start(s.cdp.clone(), s.tab.clone(), self.browser_dir(run)).await?,
                    );
                }
            }
            _ => return Err("Unsupported action".into()),
        }
        Ok(
            json!({"dispatched":true,"action":action,"next":"Observe again before another interaction; dispatched does not prove website success"}),
        )
    }
}
async fn mouse(
    cdp: &Cdp,
    tab: &str,
    event: &str,
    x: f64,
    y: f64,
    button: &str,
    count: u32,
) -> Result<()> {
    cdp.call(tab,"Input.dispatchMouseEvent",json!({"type":event,"x":x,"y":y,"button":button,"buttons":if button=="left" && event!="mouseReleased"{1}else{0},"clickCount":count})).await?;
    Ok(())
}

fn image_hash(data: &str) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    data.hash(&mut hasher);
    hasher.finish()
}
