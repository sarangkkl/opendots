use crate::models::Result;

use serde_json::{Value, json};

use std::{
    path::Path,
    process::Stdio,
    sync::{Arc, Mutex, OnceLock},
};

use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader, Lines},
    process::{Child, ChildStdin, ChildStdout, Command},
    time::{Duration, timeout},
};

static OWNED: OnceLock<Mutex<std::collections::HashSet<u32>>> = OnceLock::new();

fn owned() -> &'static Mutex<std::collections::HashSet<u32>> {
    OWNED.get_or_init(|| Mutex::new(std::collections::HashSet::new()))
}

pub fn shutdown_all() {
    let pids: Vec<_> = owned().lock().unwrap().iter().copied().collect();

    for pid in pids {
        #[cfg(unix)]
        unsafe {
            libc::kill(-(pid as i32), libc::SIGKILL);
        }

        #[cfg(windows)]
        if pid > 0 {
            let _ = std::process::Command::new("taskkill")
                .args(["/PID", &pid.to_string(), "/T", "/F"])
                .status();
        }
    }
}

pub struct Process {
    pub child: Child,
    pub input: Option<ChildStdin>,
    pub lines: Option<Lines<BufReader<ChildStdout>>>,
    stdout_drain: Option<tokio::task::JoinHandle<()>>,
    stderr: tokio::task::JoinHandle<()>,
    pid: u32,
    errors: Arc<Mutex<Vec<String>>>,
}

impl Process {
    pub fn spawn(executable: &str, args: &[String], cwd: &Path) -> Result<Self> {
        let mut cmd = Command::new(executable);

        cmd.args(args)
            .current_dir(cwd)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);

        #[cfg(unix)]
        cmd.process_group(0);

        #[cfg(windows)]
        cmd.creation_flags(0x08000000);

        let mut child = cmd
            .spawn()
            .map_err(|e| format!("Cannot start {executable}: {e}"))?;

        let pid = child.id().unwrap_or(0);

        owned().lock().unwrap().insert(pid);

        let input = child.stdin.take().unwrap();

        let lines = BufReader::new(child.stdout.take().unwrap()).lines();

        let stderr = child.stderr.take().unwrap();

        let errors = Arc::new(Mutex::new(Vec::new()));

        let collected = errors.clone();

        let stderr = tokio::spawn(async move {
            let mut lines = BufReader::new(stderr).lines();

            while let Ok(Some(line)) = lines.next_line().await {
                if line.starts_with("Error:") && !line.contains("http") && !line.contains("sk-") {
                    let mut errors = collected.lock().unwrap();

                    if errors.len() < 5 {
                        errors.push(line.chars().take(1000).collect());
                    }
                }
            }
        });

        Ok(Self {
            child,
            input: Some(input),
            lines: Some(lines),
            stdout_drain: None,
            stderr,
            pid,
            errors,
        })
    }

    pub fn drain_stdout(&mut self) {
        if let Some(mut lines) = self.lines.take() {
            self.stdout_drain = Some(tokio::spawn(async move {
                while let Ok(Some(_)) = lines.next_line().await {}
            }));
        }
    }
    pub async fn send(&mut self, v: Value) -> Result<()> {
        self.input
            .as_mut()
            .ok_or("Process input is closed")?
            .write_all(format!("{v}\n").as_bytes())
            .await
            .map_err(|e| e.to_string())
    }

    pub async fn next(&mut self) -> Result<Option<Value>> {
        match self
            .lines
            .as_mut()
            .ok_or("Process stdout is being drained")?
            .next_line()
            .await
            .map_err(|e| e.to_string())?
        {
            Some(s) => serde_json::from_str(&s)
                .map(Some)
                .map_err(|_| "Provider returned malformed JSON".into()),
            None => Ok(None),
        }
    }

    pub async fn request(&mut self, id: u64, method: &str, params: Value) -> Result<Value> {
        self.send(json!({
        "jsonrpc":"2.0","id":id,"method":method,"params":params}
        ))
        .await?;

        timeout(Duration::from_secs(60), async {
            loop {
                let v = self.next().await?.ok_or_else(|| {
                    format!(
                        "Process exited during initialization: {}",
                        self.errors.lock().unwrap().join("; ")
                    )
                })?;
                if v["id"] == id {
                    if !v["error"].is_null() {
                        return Err(v["error"].to_string());
                    }
                    return Ok(v["result"].clone());
                }

                if v.get("id").is_some() && v.get("method").is_some() {
                    self.send(json!({
                    "jsonrpc":"2.0","id":v["id"],"error":{
                    "code":-32601,"message":"Client operation not available during initialization"}
                    }
                    ))
                    .await?;
                }
            }
        })
        .await
        .map_err(|_| "Initialization timed out".to_string())?
    }

    pub async fn stop(&mut self) {
        #[cfg(unix)]
        if self.pid > 0 {
            unsafe {
                libc::kill(-(self.pid as i32), libc::SIGTERM);
            }
        }

        #[cfg(windows)]
        {
            let _ = Command::new("taskkill")
                .args(["/PID", &self.pid.to_string(), "/T", "/F"])
                .output()
                .await;
        }

        let _ = self.child.kill().await;

        let _ = self.child.wait().await;

        #[cfg(unix)]
        if self.pid > 0 {
            unsafe {
                libc::kill(-(self.pid as i32), libc::SIGKILL);
            }
        }

        owned().lock().unwrap().remove(&self.pid);

        self.pid = 0;

        self.stderr.abort();
        if let Some(drain) = self.stdout_drain.take() {
            drain.abort();
        }
    }
}

impl Drop for Process {
    fn drop(&mut self) {
        #[cfg(unix)]
        if self.pid > 0 {
            unsafe {
                libc::kill(-(self.pid as i32), libc::SIGKILL);
            }
        }

        #[cfg(windows)]
        if self.pid > 0 {
            let _ = std::process::Command::new("taskkill")
                .args(["/PID", &self.pid.to_string(), "/T", "/F"])
                .spawn();
        }

        owned().lock().unwrap().remove(&self.pid);

        self.stderr.abort();
        if let Some(drain) = self.stdout_drain.take() {
            drain.abort();
        }
    }
}
