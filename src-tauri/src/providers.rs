use crate::{mcp, models::*, process::Process, runtime::Runtime};

use serde_json::{Value, json};

use std::{
    path::{Path, PathBuf},
    sync::Arc,
};

use tokio::{
    process::Command,
    time::{Duration, sleep, timeout},
};

pub fn executable(provider: &str, explicit: &str) -> Result<String> {
    if !explicit.is_empty() {
        return Ok(explicit.into());
    }

    let name = match provider {
        "claude" => "claude",
        "codex" => "codex",
        "cursor" => "agent",
        _ => return Err("Unsupported provider".into()),
    };

    let mut dirs: Vec<PathBuf> = std::env::var_os("PATH")
        .map(|v| std::env::split_paths(&v).collect())
        .unwrap_or_default();

    if let Some(home) = std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE")) {
        let home = PathBuf::from(home);

        dirs.extend([
            home.join(".local/bin"),
            home.join(".cargo/bin"),
            PathBuf::from("/opt/homebrew/bin"),
            PathBuf::from("/usr/local/bin"),
        ]);

        if let Ok(versions) = std::fs::read_dir(home.join(".nvm/versions/node")) {
            for entry in versions.flatten() {
                dirs.push(entry.path().join("bin"));
            }
        }
    }

    for dir in dirs {
        for suffix in if cfg!(windows) {
            vec![".exe", ".cmd", ""]
        } else {
            vec![""]
        } {
            let path = dir.join(format!("{name}{suffix}"));

            if path.is_file() {
                return Ok(path.to_string_lossy().into());
            }
        }
    }

    Err(format!(
        "{name} not found. Install and sign in, or set its executable path."
    ))
}

pub async fn discover(provider: &str, path: &str) -> Value {
    match executable(provider, path) {
        Ok(path) => {
            let mut cmd = Command::new(&path);

            cmd.arg("--version").kill_on_drop(true);

            match timeout(Duration::from_secs(10), cmd.output()).await {
                Ok(Ok(o)) if o.status.success() => {
                    json!({
                    "provider":provider,"path":path,"available":true,"version":String::from_utf8_lossy(&o.stdout).trim(),"native_tools":"disabled; use assigned MCP tools","plugins":"Provider-native plugins are not loaded in managed sessions"}
                    )
                }

                _ => {
                    json!({
                    "provider":provider,"path":path,"available":false,"error":"Version check failed or timed out"}
                    )
                }
            }
        }

        Err(e) => json!({
        "provider":provider,"available":false,"error":e}
        ),
    }
}

async fn next(rt: &Runtime, run: &str, p: &mut Process) -> Result<Option<Value>> {
    loop {
        if rt.cancelled(run) {
            return Err("Execution cancelled".into());
        }

        tokio::select! {
        v=p.next()=>return v,_=sleep(Duration::from_millis(150))=>{
        }
        }
    }
}

pub async fn execute(rt: Arc<Runtime>, run: &Execution, token: &str) -> Result<String> {
    let a: Agent = rt.db.get("agent", &run.agent_id)?;

    let exe = executable(&a.provider, &a.executable)?;

    // Provider-owned settings are confined to an application workspace, not written into user projects.
    let cwd = rt.root.join("workspaces").join(&a.id);

    std::fs::create_dir_all(&cwd).map_err(|e| e.to_string())?;

    let mut prompt = rt.context(run, &a)?;

    if !a.workspace.is_empty() {
        prompt.push_str(&format!(
            "\nUser workspace reference: {}. Access it only through assigned tools.",
            a.workspace
        ));
    }

    let url = format!("{}/mcp/{token}", rt.endpoint.lock().unwrap());

    let work = async {
        match a.provider.as_str() {
            "claude" => claude(rt.clone(), run, &a, &exe, &cwd, &prompt, &url).await,
            "codex" => codex(rt.clone(), run, &a, &exe, &cwd, &prompt).await,
            "cursor" => cursor(rt.clone(), run, &a, &exe, &cwd, &prompt, &url).await,
            _ => Err("Unsupported provider".into()),
        }
    };

    tokio::select! {
    result=work=>result,_=async{
    while !rt.cancelled(&run.id){
    sleep(Duration::from_millis(100)).await}
    }
    =>Err("Execution cancelled".into())}
}

async fn claude(
    rt: Arc<Runtime>,
    run: &Execution,
    a: &Agent,
    exe: &str,
    cwd: &Path,
    prompt: &str,
    url: &str,
) -> Result<String> {
    let config = json!({
    "mcpServers":{
    "opendots":{
    "type":"http","url":url}
    }
    }
    );

    let mut args = vec![
        "-p".into(),
        "--output-format".into(),
        "stream-json".into(),
        "--verbose".into(),
        "--include-partial-messages".into(),
        "--tools".into(),
        "".into(),
        "--strict-mcp-config".into(),
        "--mcp-config".into(),
        config.to_string(),
        "--setting-sources".into(),
        "".into(),
        "--settings".into(),
        json!({
        "disableAllHooks":true,"enabledPlugins":{
        }
        }
        )
        .to_string(),
        "--disable-slash-commands".into(),
        "--allowedTools".into(),
        "mcp__opendots".into(),
    ];

    if !a.model.is_empty() {
        args.extend(["--model".into(), a.model.clone()]);
    }

    if let Some(s) = rt.db.session(&a.id, &run.conversation_id, "claude") {
        args.extend(["--resume".into(), s]);
    }

    let mut p = Process::spawn(exe, &args, cwd)?;

    use tokio::io::AsyncWriteExt;

    let mut stdin = p.input.take().ok_or("Missing provider stdin")?;

    stdin
        .write_all(prompt.as_bytes())
        .await
        .map_err(|e| e.to_string())?;

    drop(stdin);

    let outcome = async {
        let mut final_text = String::new();

        let mut completed = false;

        while let Some(v) = next(&rt, &run.id, &mut p).await? {
            if let Some(s) = v["session_id"].as_str() {
                rt.db
                    .set_session(&a.id, &run.conversation_id, "claude", s)?;
            }

            match v["type"].as_str().unwrap_or("") {
                "stream_event" => {
                    if v["event"]["delta"]["type"] == "text_delta" {
                        rt.event(
                            &run.id,
                            "text",
                            json!({
                            "text":v["event"]["delta"]["text"]}
                            ),
                        );
                    }
                }

                "assistant" => {
                    if let Some(blocks) = v["message"]["content"].as_array() {
                        for b in blocks {
                            match b["type"].as_str() {
                                Some("tool_use") => rt.event(
                                    &run.id,
                                    "tool_call",
                                    json!({
                                    "name":b["name"],"arguments":b["input"]}
                                    ),
                                ),
                                Some("text") => {}

                                _ => {}
                            }
                        }
                    }
                }

                "result" => {
                    if v["is_error"].as_bool() == Some(true) {
                        return Err(v["result"]
                            .as_str()
                            .map(str::to_string)
                            .unwrap_or_else(|| v["errors"].to_string()));
                    }

                    final_text = v["result"].as_str().unwrap_or("").into();

                    completed = true;

                    rt.event(
                        &run.id,
                        "usage",
                        json!({
                        "usage":v["usage"],"cost_usd":v["total_cost_usd"]}
                        ),
                    );
                }

                _ => {}
            }
        }

        let status = p.child.wait().await.map_err(|e| e.to_string())?;

        if !completed || !status.success() {
            return Err(format!(
                "Claude exited without successful completion ({status}). Check provider login."
            ));
        }

        Ok(final_text)
    }
    .await;

    p.stop().await;

    outcome
}

async fn codex(
    rt: Arc<Runtime>,
    run: &Execution,
    a: &Agent,
    exe: &str,
    cwd: &Path,
    prompt: &str,
) -> Result<String> {
    let mut args = vec![
        "app-server",
        "-c",
        "apps._default.enabled=false",
        "-c",
        "features.shell_tool=false",
        "-c",
        "features.shell_snapshot=false",
        "-c",
        "features.multi_agent=false",
        "-c",
        "features.js_repl=false",
        "-c",
        "web_search=\"disabled\"",
        "-c",
        "sandbox_mode=\"read-only\"",
        "-c",
        "approval_policy=\"on-request\"",
    ]
    .into_iter()
    .map(str::to_string)
    .collect::<Vec<_>>();

    let mut p = Process::spawn(exe, &args, cwd)?;

    let result=async {
p.request(1,"initialize",json!({
"clientInfo":{
"name":"opendots","version":"0.1.0"}
,"capabilities":{
"experimentalApi":true}
}
)).await?;
p.send(json!({
"method":"initialized"}
)).await?;

 // Codex merges config tables. Disable inherited entries individually.
 let config=p.request(2,"config/read",json!({
"includeLayers":false}
)).await?;

 let mut overrides=Vec::new();

 for table in ["mcp_servers","plugins","apps"] {

 if let Some(entries)=config["config"][table].as_object() {

 for key in entries.keys(){
if key.contains('.'){
return Err("Codex integration names containing dots cannot be isolated by this CLI version".into())}
overrides.extend(["-c".to_string(),format!("{table}.{key}.enabled=false")]);
}

 }
}

 if !overrides.is_empty(){
p.stop().await;
args.extend(overrides);
p=Process::spawn(exe,&args,cwd)?;

 p.request(1,"initialize",json!({
"clientInfo":{
"name":"opendots","version":"0.1.0"}
,"capabilities":{
"experimentalApi":true}
}
)).await?;

 p.send(json!({
"method":"initialized"}
)).await?;

 let effective=p.request(2,"config/read",json!({
"includeLayers":false}
)).await?;

 for table in ["mcp_servers","plugins"]{
if effective["config"][table].as_object().is_some_and(|m|m.values().any(|v|v["enabled"]!=false)){
return Err("Codex configuration could not disable inherited integrations".into());
}
}

 }

 let saved=rt.db.session(&a.id,&run.conversation_id,"codex");
let mut params=json!({
"cwd":cwd,"approvalPolicy":"on-request","sandbox":"read-only","baseInstructions":"Use OpenDots tools for all external actions. Native execution is disabled.","persistExtendedHistory":true}
);
if !a.model.is_empty(){
params["model"]=json!(a.model);
}

 let method=if let Some(s)=saved{
params["threadId"]=json!(s);
"thread/resume"}
else{
params["dynamicTools"]=json!(mcp::builtin_tools().into_iter().map(|t|json!({
"name":t["name"],"description":t["description"],"inputSchema":t["inputSchema"]}
)).collect::<Vec<_>>());
"thread/start"}
;

 let response=p.request(3,method,params).await?;
let thread=response["thread"]["id"].as_str().ok_or("Codex did not return a thread ID")?.to_string();
rt.db.set_session(&a.id,&run.conversation_id,"codex",&thread)?;

 p.send(json!({
"id":4,"method":"turn/start","params":{
"threadId":thread,"input":[{
"type":"text","text":prompt}
]}
}
)).await?;

 let mut text=String::new();
while let Some(v)=next(&rt,&run.id,&mut p).await?{
let method=v["method"].as_str().unwrap_or("");
let params=&v["params"];
if v.get("id").is_some()&&!method.is_empty(){
let answer=if method=="item/tool/call"{
let tool=params["tool"].as_str().unwrap_or("");
let r=rt.tool_output(&run.id,tool,params["arguments"].clone()).await;
json!({"contentItems":match &r { Ok(v)=>v.codex(), Err(e)=>vec![json!({"type":"inputText","text":e})]},"success":r.is_ok()})
}
else if method=="item/permissions/requestApproval" {
    json!({"permissions":{},"scope":"turn"})
} else if method.ends_with("requestApproval") {
    json!({"decision":"decline"})
}
else{
p.send(json!({
"id":v["id"],"error":{
"code":-32601,"message":"Unsupported client operation"}
}
)).await?;
continue}
;
p.send(json!({
"id":v["id"],"result":answer}
)).await?;
continue}

 if !v["error"].is_null(){
return Err(v["error"].to_string())}
match method{
"item/agentMessage/delta"=>{
let delta=params["delta"].as_str().unwrap_or("");
text.push_str(delta);
rt.event(&run.id,"text",json!({
"text":delta}
));
}
,"item/started"|"item/completed"=>{
let item=&params["item"];
if matches!(item["type"].as_str(),Some("dynamicToolCall"|"mcpToolCall"|"commandExecution"|"fileChange")){
rt.event(&run.id,if method.ends_with("started"){
"tool_call"}
else{
"tool_result"}
,item.clone());
}
}
,"turn/completed"=>{
if params["turn"]["status"]!="completed"{
return Err(format!("Codex turn failed: {}",params["turn"]["error"]))}
return Ok(text)}
,"error"=>return Err(params["error"].to_string()),_=>{
}
}

 }
Err("Codex exited before completing the turn".into())}
.await;

    p.stop().await;

    result
}

async fn cursor(
    rt: Arc<Runtime>,
    run: &Execution,
    a: &Agent,
    exe: &str,
    cwd: &Path,
    prompt: &str,
    url: &str,
) -> Result<String> {
    let dir = cwd.join(".cursor");

    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;

    std::fs::write(
        dir.join("cli.json"),
        json!({
        "permissions":{
        "allow":[],"deny":["Shell(*)","Read(*)","Write(*)"]}
        }
        )
        .to_string(),
    )
    .map_err(|e| e.to_string())?;

    // Global MCP servers would bypass OpenDots. Require users to remove them from the managed CLI profile.
    if let Some(home) = std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE")) {
        let global = PathBuf::from(home).join(".cursor/mcp.json");

        if let Ok(s) = std::fs::read_to_string(global) {
            let v: Value = serde_json::from_str(&s)
                .map_err(|_| "Cursor global MCP configuration is invalid")?;

            if v["mcpServers"].as_object().is_some_and(|m| !m.is_empty()) {
                return Err("Cursor has global MCP servers. Managed sessions require an empty global MCP configuration; assign servers in OpenDots instead.".into());
            }
        }
    }

    std::fs::write(
        dir.join("mcp.json"),
        json!({
        "mcpServers":{
        "opendots":{
        "url":url}
        }
        }
        )
        .to_string(),
    )
    .map_err(|e| e.to_string())?;

    let mut args = vec!["acp".into()];

    if !a.model.is_empty() {
        args = vec!["--model".into(), a.model.clone(), "acp".into()];
    }

    let mut p = Process::spawn(exe, &args, cwd)?;

    let result=async{
let init=p.request(1,"initialize",json!({
"protocolVersion":1,"clientCapabilities":{
"fs":{
"readTextFile":false,"writeTextFile":false}
,"terminal":false}
,"clientInfo":{
"name":"opendots","version":"0.1.0"}
}
)).await?;

 let saved=rt.db.session(&a.id,&run.conversation_id,"cursor");
let mut params=json!({
"cwd":cwd,"mcpServers":[{
"type":"http","name":"opendots","url":url,"headers":[]}
]}
);
let method=if let Some(s)=saved{
if init["agentCapabilities"]["loadSession"]!=true{
return Err("This Cursor version cannot resume sessions. Upgrade Cursor or create a new agent.".into())}
params["sessionId"]=json!(s);
"session/load"}
else{
"session/new"}
;
let response=p.request(2,method,params.clone()).await?;
let session=response["sessionId"].as_str().or(params["sessionId"].as_str()).ok_or("Cursor did not return a session ID")?.to_string();
rt.db.set_session(&a.id,&run.conversation_id,"cursor",&session)?;
p.send(json!({
"jsonrpc":"2.0","id":3,"method":"session/prompt","params":{
"sessionId":session,"prompt":[{
"type":"text","text":prompt}
]}
}
)).await?;

 let mut text=String::new();
while let Some(v)=next(&rt,&run.id,&mut p).await?{
if v["method"]=="session/request_permission"{
let params=&v["params"];
let title=params["toolCall"]["title"].as_str().unwrap_or("");
let is_bridge=title.contains("opendots");
let options=params["options"].as_array().cloned().unwrap_or_default();
let desired=if is_bridge{
"allow_once"}
else{
"reject_once"}
;
let option=options.iter().find(|o|o["kind"]==desired).and_then(|o|o["optionId"].as_str());
let outcome=option.map(|id|json!({
"outcome":"selected","optionId":id}
)).unwrap_or(json!({
"outcome":"cancelled"}
));
p.send(json!({
"jsonrpc":"2.0","id":v["id"],"result":{
"outcome":outcome}
}
)).await?;
continue}

 if v.get("id").is_some()&&v.get("method").is_some(){
p.send(json!({
"jsonrpc":"2.0","id":v["id"],"error":{
"code":-32601,"message":"Native client tools disabled; use OpenDots MCP"}
}
)).await?;
continue}

 if v["id"]==3 {
if !v["error"].is_null(){
return Err(v["error"].to_string())}

if v["result"]["stopReason"]=="cancelled"{
return Err("Cursor cancelled the turn".into())}
return Ok(text)}

 if v["method"]=="session/update"{
let update=&v["params"]["update"];
match update["sessionUpdate"].as_str().unwrap_or(""){
"agent_message_chunk"=>{
if update["content"]["type"]=="text"{
let s=update["content"]["text"].as_str().unwrap_or("");
text.push_str(s);
rt.event(&run.id,"text",json!({
"text":s}
));
}
}
,"tool_call"|"tool_call_update"=>rt.event(&run.id,"tool_call",update.clone()),_=>{
}
}
}
}

 Err("Cursor exited before completing the turn".into())}
.await;

    p.stop().await;

    let _ = std::fs::remove_file(dir.join("mcp.json"));

    result
}
