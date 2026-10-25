use crate::{models::*, process::Process, runtime::Runtime};

use axum::{
    Json, Router,
    extract::{DefaultBodyLimit, Path, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::post,
};

use serde_json::{Value, json};

use std::{path::Path as FsPath, sync::Arc};

use tokio::time::{Duration, timeout};

pub fn secret(id: &str) -> Result<Option<String>> {
    match keyring::Entry::new("opendots", id)
        .map_err(|e| e.to_string())?
        .get_password()
    {
        Ok(s) => Ok(Some(s)),
        Err(keyring::Error::NoEntry) => Ok(None),
        Err(e) => Err(e.to_string()),
    }
}

pub enum Client {
    Http {
        client: reqwest::Client,
        url: String,
        session: Option<String>,
        bearer: Option<String>,
    },
    Stdio(Box<Process>),
}

impl Client {
    pub async fn connect(i: &Integration) -> Result<Self> {
        let mut c = if !i.command.is_empty() {
            Self::Stdio(Box::new(Process::spawn(
                &i.command,
                &i.args,
                FsPath::new("."),
            )?))
        } else {
            let url = reqwest::Url::parse(&i.url).map_err(|_| "Invalid MCP URL")?;

            if !matches!(url.scheme(), "http" | "https") {
                return Err("MCP URL must use HTTP or HTTPS".into());
            }

            Self::Http {
                client: reqwest::Client::builder()
                    .timeout(Duration::from_secs(120))
                    .redirect(reqwest::redirect::Policy::none())
                    .build()
                    .map_err(|e| e.to_string())?,
                url: i.url.clone(),
                session: None,
                bearer: if i.has_secret {
                    Some(secret(&i.id)?.ok_or("Saved credential is missing from the OS store")?)
                } else {
                    None
                },
            }
        };

        c.request(
            1,
            "initialize",
            json!({
            "protocolVersion":"2025-03-26","capabilities":{
            }
            ,"clientInfo":{
            "name":"opendots","version":"0.1.0"}
            }
            ),
        )
        .await?;

        c.send(json!({
        "jsonrpc":"2.0","method":"notifications/initialized"}
        ))
        .await?;

        Ok(c)
    }

    async fn send(&mut self, v: Value) -> Result<Value> {
        match self {
            Self::Stdio(p) => {
                p.send(v.clone()).await?;

                if v.get("id").is_none() {
                    return Ok(Value::Null);
                }

                timeout(Duration::from_secs(120), async {
                    loop {
                        let r = p.next().await?.ok_or("MCP server exited")?;
                        if r["id"] == v["id"] {
                            return Ok(r);
                        }

                        if r.get("method").is_some() && r.get("id").is_some() {
                            p.send(json!({
                            "jsonrpc":"2.0","id":r["id"],"error":{
                            "code":-32601,"message":"Sampling and elicitation are not enabled"}
                            }
                            ))
                            .await?;
                        }
                    }
                })
                .await
                .map_err(|_| "MCP request timed out".to_string())?
            }

            Self::Http {
                client,
                url,
                session,
                bearer,
            } => {
                let mut r = client
                    .post(url.as_str())
                    .header("Accept", "application/json, text/event-stream")
                    .header("MCP-Protocol-Version", "2025-03-26")
                    .json(&v);

                if let Some(s) = session.as_ref() {
                    r = r.header("Mcp-Session-Id", s)
                }

                if let Some(s) = bearer.as_ref() {
                    r = r.bearer_auth(s)
                }

                let mut response = r
                    .send()
                    .await
                    .map_err(|e| e.to_string())?
                    .error_for_status()
                    .map_err(|e| e.to_string())?;

                if let Some(s) = response
                    .headers()
                    .get("Mcp-Session-Id")
                    .and_then(|s| s.to_str().ok())
                {
                    *session = Some(s.into());
                }

                if v.get("id").is_none() {
                    return Ok(Value::Null);
                }

                let is_sse = response
                    .headers()
                    .get("content-type")
                    .and_then(|v| v.to_str().ok())
                    .is_some_and(|v| v.contains("text/event-stream"));

                if !is_sse {
                    return response.json().await.map_err(|e| e.to_string());
                }

                let mut bytes = Vec::new();

                while let Some(chunk) = response.chunk().await.map_err(|e| e.to_string())? {
                    bytes.extend_from_slice(&chunk);

                    if bytes.len() > 4_000_000 {
                        return Err("MCP response exceeds 4 MB".into());
                    }

                    let text = String::from_utf8_lossy(&bytes);

                    for line in text.lines().filter_map(|l| l.strip_prefix("data:")) {
                        if let Ok(r) = serde_json::from_str::<Value>(line.trim())
                            && r["id"] == v["id"]
                        {
                            return Ok(r);
                        }
                    }
                }

                Err("MCP stream ended without a response".into())
            }
        }
    }

    pub async fn request(&mut self, id: u64, method: &str, params: Value) -> Result<Value> {
        let r = self
            .send(json!({
            "jsonrpc":"2.0","id":id,"method":method,"params":params}
            ))
            .await?;

        if !r["error"].is_null() {
            Err(r["error"].to_string())
        } else {
            Ok(r["result"].clone())
        }
    }

    pub async fn close(&mut self) {
        if let Self::Stdio(p) = self {
            p.stop().await
        }
    }
}

fn tool(name: &str, description: &str, properties: Value, required: Value) -> Value {
    json!({
    "name":name,"description":description,"inputSchema":{
    "type":"object","properties":properties,"required":required,"additionalProperties":false}
    }
    )
}

pub fn builtin_tools() -> Vec<Value> {
    let mut tools = vec![
        tool(
            "send_to_agent",
            "Hand off a task or ask a group member a question. Use a stable request_id to avoid duplicate handoffs.",
            json!({
            "agent_id":{
            "type":"string"}
            ,"message":{
            "type":"string"}
            ,"request_id":{
            "type":"string"}
            ,"artifacts":{
            "type":"array","items":{
            "type":"string"}
            }
            }
            ),
            json!(["agent_id", "message", "request_id"]),
        ),
        tool(
            "propose_memory",
            "Propose a lasting lesson for the user to approve.",
            json!({
            "content":{
            "type":"string"}
            }
            ),
            json!(["content"]),
        ),
        tool(
            "list_integrations",
            "List assigned integrations and their IDs.",
            json!({}),
            json!([]),
        ),
        tool(
            "list_tools",
            "Discover tools in one assigned MCP integration.",
            json!({
            "integration_id":{
            "type":"string"}
            }
            ),
            json!(["integration_id"]),
        ),
        tool(
            "call_tool",
            "Call an assigned MCP tool. OpenDots enforces its allow/ask/deny policy before executing.",
            json!({
            "integration_id":{
            "type":"string"}
            ,"tool":{
            "type":"string"}
            ,"arguments":{
            "type":"object"}
            }
            ),
            json!(["integration_id", "tool", "arguments"]),
        ),
    ];
    tools.extend(crate::browser::tools());
    tools
}

fn required<'a>(v: &'a Value, key: &str) -> Result<&'a str> {
    v[key]
        .as_str()
        .filter(|v| !v.trim().is_empty())
        .ok_or_else(|| format!("Missing {key}"))
}

impl Runtime {
    pub async fn tool_output(
        &self,
        run: &str,
        name: &str,
        args: Value,
    ) -> Result<crate::browser::ToolOutput> {
        if name.starts_with("browser_") {
            self.browser_tool(run, name, args).await
        } else {
            self.tool(run, name, args)
                .await
                .map(crate::browser::ToolOutput::text)
        }
    }
    pub async fn tool(&self, run_id: &str, name: &str, args: Value) -> Result<Value> {
        let run = self.db.run(run_id)?;

        if !matches!(run.status.as_str(), "running" | "waiting_approval") {
            return Err("Execution is not active".into());
        }

        if self.cancelled(run_id) {
            return Err("Execution cancelled".into());
        }

        let agent: Agent = self.db.get("agent", &run.agent_id)?;

        match name {
            "propose_memory" => {
                let content = required(&args, "content")?;

                if content.len() > 20_000 {
                    return Err("Memory exceeds 20 KB".into());
                }

                let m = Memory {
                    id: id(),
                    agent_id: agent.id,
                    content: content.into(),
                    approved: false,
                };

                self.db.put("memory", &m.id, &m)?;

                self.notify();

                Ok(json!({
                "memory_id":m.id,"status":"awaiting_user_review"}
                ))
            }

            "send_to_agent" => {
                let recipient = required(&args, "agent_id")?;

                let request = required(&args, "request_id")?;

                let message = required(&args, "message")?;

                let group_id = run
                    .group_id
                    .as_ref()
                    .ok_or("Handoffs require a group conversation")?;

                let mut group: Group = self.db.get("group", group_id)?;

                if !group.members.contains(&recipient.to_string())
                    || !group.members.contains(&agent.id)
                {
                    return Err("Sender and recipient must be group members".into());
                }

                if group.paused {
                    return Err("Group is paused".into());
                }

                let key = format!("{}:{request}", run.id);

                if let Ok(v) = self.db.get::<Value>("handoff", &key) {
                    return Ok(v);
                }

                let count = self
                    .db
                    .runs()?
                    .iter()
                    .filter(|r| r.chain_id == run.chain_id && r.depth > 0)
                    .count();

                if count >= group.handoff_limit as usize {
                    group.paused = true;

                    self.db.put("group", &group.id, &group)?;

                    self.notify();

                    return Err("Handoff limit reached; group paused for review. Increase the limit and resume to continue.".into());
                }

                if !group.automatic {
                    self.approve(
                        run_id,
                        "handoff",
                        json!({
                        "to":recipient,"message":message}
                        ),
                    )
                    .await?;
                }

                let _guard = self.mutations.lock().unwrap();

                if self.cancelled(run_id) {
                    return Err("Execution cancelled".into());
                }

                let current: Group = self.db.get("group", group_id)?;

                if current.paused || !current.members.contains(&recipient.to_string()) {
                    return Err("Group changed or paused before handoff".into());
                }

                if let Ok(v) = self.db.get::<Value>("handoff", &key) {
                    return Ok(v);
                }

                let prompt = if let Some(artifacts) = args.get("artifacts") {
                    format!("{message}\nArtifact references: {artifacts}")
                } else {
                    message.into()
                };

                let count = self
                    .db
                    .runs()?
                    .iter()
                    .filter(|r| r.chain_id == run.chain_id && r.depth > 0)
                    .count();

                if count >= current.handoff_limit as usize {
                    return Err("Handoff limit reached".into());
                }

                let child = Execution {
                    id: id(),
                    agent_id: recipient.into(),
                    conversation_id: run.conversation_id.clone(),
                    group_id: run.group_id.clone(),
                    job_id: run.job_id.clone(),
                    chain_id: run.chain_id.clone(),
                    depth: run.depth + 1,
                    prompt: prompt.clone(),
                    status: "queued".into(),
                    created_at: now(),
                    updated_at: now(),
                    error: None,
                };

                let result = json!({
                "execution_id":child.id,"recipient":recipient}
                );

                let target: Agent = self.db.get("agent", recipient)?;

                let msg = Message {
                    id: id(),
                    conversation_id: run.conversation_id.clone(),
                    agent_id: Some(agent.id.clone()),
                    execution_id: Some(child.id.clone()),
                    role: "handoff".into(),
                    content: format!("@{}: {}", target.name, prompt),
                    created_at: now(),
                };

                let shared: Vec<_> = args["artifacts"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(Value::as_str)
                    .filter(|a| a.starts_with("browser:"))
                    .collect();
                for reference in &shared {
                    self.browser_file(&agent.id, reference)?;
                }
                self.db.insert_handoff(&child, &msg, &key, &result)?;
                for reference in shared {
                    self.db
                        .put("browser_share", &format!("{recipient}:{reference}"), &true)?;
                }

                self.event(run_id, "handoff", result.clone());

                Ok(result)
            }

            "list_integrations" => {
                let integrations: Vec<Value> = agent
                    .integrations
                    .iter()
                    .filter_map(|i| self.db.get::<Integration>("integration", i).ok())
                    .map(|i| {
                        json!({
                        "id":i.id,"name":i.name}
                        )
                    })
                    .collect();

                Ok(json!(integrations))
            }

            "list_tools" | "call_tool" => {
                let integration_id = required(&args, "integration_id")?;

                if !agent.integrations.contains(&integration_id.into()) {
                    return Err("Integration not assigned to this agent".into());
                }

                let integration: Integration = self.db.get("integration", integration_id)?;

                if name == "call_tool" {
                    let tool = required(&args, "tool")?;

                    let policy_key = format!("{integration_id}:{tool}");

                    match agent.policies.get(&policy_key).map(String::as_str).unwrap_or("ask"){
"deny"=>return Err("Tool denied by agent policy".into()),"allow"=>{
}
,_=>self.approve(run_id,"tool",json!({
"integration_id":integration_id,"tool":tool,"arguments":args["arguments"]}
)).await?}

                    let current: Agent = self.db.get("agent", &agent.id)?;

                    if !current.integrations.contains(&integration_id.to_string())
                        || current
                            .policies
                            .get(&policy_key)
                            .is_some_and(|p| p == "deny")
                    {
                        return Err("Tool access was revoked".into());
                    }
                }

                if self.cancelled(run_id) {
                    return Err("Execution cancelled".into());
                }

                self.event(
                    run_id,
                    "tool_call",
                    json!({
                    "name":name,"arguments":args}
                    ),
                );

                let work = async {
                    let mut client = Client::connect(&integration).await?;
                    let result = if name == "list_tools" {
                        client.request(2, "tools/list", json!({})).await
                    } else {
                        client
                            .request(
                                2,
                                "tools/call",
                                json!({"name":args["tool"],"arguments":args["arguments"]}),
                            )
                            .await
                    };
                    client.close().await;
                    result
                };
                let result = tokio::select! {
                    result=work=>result,
                    _=async {while !self.cancelled(run_id){tokio::time::sleep(Duration::from_millis(100)).await}} => Err("Execution cancelled; an external server may already have accepted the action".into())
                };

                self.event(
                    run_id,
                    "tool_result",
                    json!({
                    "result":result.as_ref().ok(),"error":result.as_ref().err()}
                    ),
                );

                result
            }

            _ => Err("Unknown OpenDots tool".into()),
        }
    }
}

async fn handle(
    State(rt): State<Arc<Runtime>>,
    Path(token): Path<String>,
    headers: HeaderMap,
    Json(v): Json<Value>,
) -> Response {
    if headers.contains_key("origin") {
        return StatusCode::FORBIDDEN.into_response();
    }

    let run = rt.tokens.lock().unwrap().get(&token).cloned();

    let Some(run) = run else {
        return StatusCode::UNAUTHORIZED.into_response();
    };

    if v.get("id").is_none() {
        return StatusCode::ACCEPTED.into_response();
    }

    let result = match v["method"].as_str().unwrap_or("") {
        "initialize" => Ok(json!({
        "protocolVersion":"2025-03-26","capabilities":{
        "tools":{
        }
        }
        ,"serverInfo":{
        "name":"opendots","version":"0.1.0"}
        }
        )),
        "ping" => Ok(json!({})),
        "tools/list" => Ok(json!({
        "tools":builtin_tools()}
        )),
        "tools/call" => {
            let name = v["params"]["name"].as_str().unwrap_or("");

            match rt
                .tool_output(&run, name, v["params"]["arguments"].clone())
                .await
            {
                Ok(data) => Ok(json!({
                "content":data.content,"isError":false}
                )),

                Err(e) => Ok(json!({
                "content":[{
                "type":"text","text":e}
                ],"isError":true}
                )),
            }
        }

        _ => Err("Method not found"),
    };

    Json(match result {
        Ok(r) => json!({
        "jsonrpc":"2.0","id":v["id"],"result":r}
        ),
        Err(e) => json!({
        "jsonrpc":"2.0","id":v["id"],"error":{
        "code":-32601,"message":e}
        }
        ),
    })
    .into_response()
}

pub async fn serve(rt: Arc<Runtime>) -> Result<()> {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .map_err(|e| e.to_string())?;

    *rt.endpoint.lock().unwrap() = format!(
        "http://{}",
        listener.local_addr().map_err(|e| e.to_string())?
    );

    let router = Router::new()
        .route("/mcp/{token}", post(handle))
        .layer(DefaultBodyLimit::max(1_000_000))
        .with_state(rt.clone());

    axum::serve(listener, router)
        .with_graceful_shutdown(async move {
            while !rt.stopping.load(std::sync::atomic::Ordering::SeqCst) {
                tokio::time::sleep(Duration::from_millis(200)).await
            }
        })
        .await
        .map_err(|e| e.to_string())
}
