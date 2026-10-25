use crate::{models::Result, process::Process};
use futures_util::{SinkExt, StreamExt};
use serde_json::{Value, json};
use std::{
    collections::HashMap,
    path::Path,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
};
use tokio::{
    sync::{broadcast, mpsc, oneshot},
    time::{Duration, timeout},
};
use tokio_tungstenite::tungstenite::Message;

#[derive(Clone)]
pub struct Cdp {
    tx: mpsc::Sender<(u64, Value, oneshot::Sender<Result<Value>>)>,
    ids: Arc<AtomicU64>,
    pub events: broadcast::Sender<Value>,
    pub dialogs: Arc<std::sync::Mutex<HashMap<String, Value>>>,
}
impl Cdp {
    pub async fn connect(url: &str) -> Result<Self> {
        let (ws, _) = tokio_tungstenite::connect_async(url)
            .await
            .map_err(|e| e.to_string())?;
        let (mut sink, mut stream) = ws.split();
        let (tx, mut rx) = mpsc::channel::<(u64, Value, oneshot::Sender<Result<Value>>)>(64);
        let (events, _) = broadcast::channel(64);
        let emit = events.clone();
        let dialogs = Arc::new(std::sync::Mutex::new(HashMap::new()));
        let dialog_state = dialogs.clone();
        tokio::spawn(async move {
            let mut pending = HashMap::new();
            loop {
                tokio::select! {
                    request = rx.recv() => match request {
                        Some((id, value, reply)) => {
                            pending.insert(id, reply);
                            if sink.send(Message::Text(value.to_string().into())).await.is_err() { break; }
                        }
                        None => break,
                    },
                    incoming = stream.next() => match incoming {
                        Some(Ok(Message::Text(text))) => {
                            let Ok(v) = serde_json::from_str::<Value>(&text) else { continue };
                            if let Some(id) = v["id"].as_u64() {
                                if let Some(reply) = pending.remove(&id) {
                                    let result = if v.get("error").is_some() { Err(v["error"].to_string()) } else { Ok(v["result"].clone()) };
                                    let _ = reply.send(result);
                                }
                            } else {
                                if let Some(session)=v["sessionId"].as_str() {
                                    if v["method"]=="Page.javascriptDialogOpening" { dialog_state.lock().unwrap().insert(session.into(),v["params"].clone()); }
                                    if v["method"]=="Page.javascriptDialogClosed" { dialog_state.lock().unwrap().remove(session); }
                                }
                                let _ = emit.send(v);
                            }
                        }
                        Some(Ok(Message::Ping(p))) => { if sink.send(Message::Pong(p)).await.is_err() { break; } }
                        Some(Ok(Message::Close(_))) | Some(Err(_)) | None => break,
                        _ => {}
                    }
                }
                pending.retain(|_, sender| !sender.is_closed());
            }
            for (_, reply) in pending {
                let _ = reply.send(Err("Browser connection closed".into()));
            }
        });
        Ok(Self {
            tx,
            ids: Arc::new(AtomicU64::new(1)),
            events,
            dialogs,
        })
    }
    pub async fn call(&self, session: &str, method: &str, params: Value) -> Result<Value> {
        let id = self.ids.fetch_add(1, Ordering::Relaxed);
        let mut v = json!({"id":id,"method":method,"params":params});
        if !session.is_empty() {
            v["sessionId"] = json!(session);
        }
        let (tx, rx) = oneshot::channel();
        self.tx
            .send((id, v, tx))
            .await
            .map_err(|_| "Browser connection closed")?;
        timeout(Duration::from_secs(20), rx).await.map_err(|_| "Browser command timed out; outcome may be unknown. Do not retry submissions automatically.")?.map_err(|_| "Browser connection closed")?
    }
    pub async fn eval(&self, session: &str, expression: String) -> Result<Value> {
        let v = self
            .call(
                session,
                "Runtime.evaluate",
                json!({"expression":expression,"returnByValue":true,"awaitPromise":true}),
            )
            .await?;
        if v.get("exceptionDetails").is_some() {
            return Err("Page evaluation failed; refresh browser observation".into());
        }
        Ok(v["result"]["value"].clone())
    }
}

pub fn executable(explicit: &str) -> Result<String> {
    if !explicit.is_empty() {
        if Path::new(explicit).is_file() {
            return Ok(explicit.into());
        }
        return Err("Configured browser executable does not exist".into());
    }
    let mut candidates = vec![
        "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome".to_owned(),
        "/Applications/Chromium.app/Contents/MacOS/Chromium".to_owned(),
        "/usr/bin/google-chrome".to_owned(),
        "/usr/bin/chromium".to_owned(),
        "/usr/bin/chromium-browser".to_owned(),
    ];
    for var in ["PROGRAMFILES", "PROGRAMFILES(X86)", "LOCALAPPDATA"] {
        if let Ok(root) = std::env::var(var) {
            candidates.push(format!("{root}/Google/Chrome/Application/chrome.exe"));
        }
    }
    candidates
        .into_iter()
        .find(|p| Path::new(p).is_file())
        .ok_or("Install Chrome/Chromium or set its executable path".into())
}

pub async fn launch(exe: &str, dir: &Path, headless: bool) -> Result<(Process, Cdp)> {
    std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    #[cfg(unix)]
    if let Ok(lock) = std::fs::read_link(dir.join("SingletonLock"))
        && let Some(pid) = lock
            .to_string_lossy()
            .rsplit('-')
            .next()
            .and_then(|p| p.parse::<i32>().ok())
        && pid > 0
        && (unsafe { libc::kill(pid, 0) } == 0
            || std::io::Error::last_os_error().raw_os_error() == Some(libc::EPERM))
    {
        return Err(
            "Saved browser profile is locked by another process. Close that browser first.".into(),
        );
    }
    // A browser can leave a stale endpoint behind. It must never be used to attach to another process.
    let endpoint = dir.join("DevToolsActivePort");
    let _ = std::fs::remove_file(&endpoint);
    let mut args = vec![
        format!("--user-data-dir={}", dir.display()),
        "--remote-debugging-port=0".into(),
        "--remote-debugging-address=127.0.0.1".into(),
        "--no-first-run".into(),
        "--no-default-browser-check".into(),
        "about:blank".into(),
    ];
    if headless {
        args.push("--headless=new".into());
    }
    let mut process = Process::spawn(exe, &args, dir)?;
    process.drain_stdout();
    for _ in 0..150 {
        if process
            .child
            .try_wait()
            .map_err(|e| e.to_string())?
            .is_some()
        {
            return Err("Browser exited. The saved profile may be locked; close its other browser and try again.".into());
        }
        if let Ok(value) = std::fs::read_to_string(&endpoint) {
            let mut lines = value.lines();
            if let (Some(port), Some(path)) = (lines.next(), lines.next())
                && let Ok(port) = port.parse::<u16>()
            {
                let cdp = Cdp::connect(&format!("ws://127.0.0.1:{port}{path}")).await?;
                return Ok((process, cdp));
            }
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    process.stop().await;
    Err("Browser startup timed out".into())
}
