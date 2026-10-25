use crate::{db::Store, mcp, models::*, runtime::Runtime, schedule};
use serde_json::{Value, json};
use std::{
    collections::HashMap,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
};
use tokio::time::{Duration, sleep};

pub fn runtime() -> Arc<Runtime> {
    let root = std::env::temp_dir().join(format!("opendots-test-{}", id()));
    std::fs::create_dir_all(&root).unwrap();
    Arc::new(Runtime {
        browsers: Default::default(),
        db: Store::open(&root.join("test.sqlite")).unwrap(),
        app: None,
        root,
        controls: Mutex::new(HashMap::new()),
        tokens: Mutex::new(HashMap::new()),
        mutations: Mutex::new(()),
        endpoint: Mutex::new(String::new()),
        stopping: AtomicBool::new(false),
    })
}
fn agent(rt: &Runtime, name: &str) -> Agent {
    let a = Agent {
        name: name.into(),
        ..Default::default()
    };
    rt.db.put("agent", &a.id, &a).unwrap();
    a
}
fn run(rt: &Runtime, a: &Agent, g: Option<String>) -> Execution {
    let r = rt
        .enqueue(
            SendInput {
                agent_id: a.id.clone(),
                group_id: g,
                prompt: "Task".into(),
                job_id: None,
            },
            None,
        )
        .unwrap();
    rt.db.status(&r.id, "running", None).unwrap();
    r
}
#[tokio::test]
async fn group_handoffs_clarification_dedup_limit_and_memory() {
    let rt = runtime();
    let researcher = agent(&rt, "Researcher");
    let creator = agent(&rt, "Creator");
    let publisher = agent(&rt, "Publisher");
    let g = Group {
        name: "Sira".into(),
        members: vec![
            researcher.id.clone(),
            creator.id.clone(),
            publisher.id.clone(),
        ],
        handoff_limit: 4,
        ..Default::default()
    };
    rt.db.put("group", &g.id, &g).unwrap();
    let first = run(&rt, &researcher, Some(g.id.clone()));
    let handoff = |agent: &Agent, message: &str, key: &str| json!({"agent_id":agent.id,"message":message,"request_id":key});
    let a = rt
        .tool(
            &first.id,
            "send_to_agent",
            handoff(&creator, "Generate today's image", "research-1"),
        )
        .await
        .unwrap();
    let duplicate = rt
        .tool(
            &first.id,
            "send_to_agent",
            handoff(&creator, "Generate today's image", "research-1"),
        )
        .await
        .unwrap();
    assert_eq!(a, duplicate);
    assert_eq!(rt.db.runs().unwrap().len(), 2);
    let second = a["execution_id"].as_str().unwrap();
    rt.db.status(second, "running", None).unwrap();
    let b = rt
        .tool(
            second,
            "send_to_agent",
            handoff(&researcher, "Which aspect ratio?", "clarify-1"),
        )
        .await
        .unwrap();
    let third = b["execution_id"].as_str().unwrap();
    rt.db.status(third, "running", None).unwrap();
    let c = rt
        .tool(
            third,
            "send_to_agent",
            handoff(&creator, "Use 4:5", "answer-1"),
        )
        .await
        .unwrap();
    let fourth = c["execution_id"].as_str().unwrap();
    rt.db.status(fourth, "running", None).unwrap();
    let d=rt.tool(fourth,"send_to_agent",json!({"agent_id":publisher.id,"message":"Review and publish","request_id":"publish-1","artifacts":["fixture://image-1"]})).await.unwrap();
    let fifth = d["execution_id"].as_str().unwrap();
    rt.db.status(fifth, "running", None).unwrap();
    assert!(
        rt.tool(
            fifth,
            "send_to_agent",
            handoff(&researcher, "One more", "limit")
        )
        .await
        .is_err()
    );
    assert!(rt.db.get::<Group>("group", &g.id).unwrap().paused);
    rt.tool(
        &first.id,
        "propose_memory",
        json!({"content":"Use 4:5 for feed images"}),
    )
    .await
    .unwrap();
    let mut memory = rt.db.list::<Memory>("memory").unwrap().remove(0);
    assert!(!memory.approved);
    assert!(
        !rt.context(&first, &researcher)
            .unwrap()
            .contains("Use 4:5 for feed images")
    );
    memory.approved = true;
    rt.db.put("memory", &memory.id, &memory).unwrap();
    assert!(
        rt.context(&first, &researcher)
            .unwrap()
            .contains("Use 4:5 for feed images")
    );
    let private = run(&rt, &publisher, None);
    assert!(
        rt.tool(
            &private.id,
            "send_to_agent",
            handoff(&creator, "No group", "bad")
        )
        .await
        .is_err()
    );
    assert!(
        !rt.context(&private, &publisher)
            .unwrap()
            .contains("Which aspect ratio?")
    );
    let reopened = Store::open(&rt.root.join("test.sqlite")).unwrap();
    assert_eq!(reopened.list::<Group>("group").unwrap().len(), 1);
    assert_eq!(reopened.messages(&first.conversation_id).unwrap().len(), 5);
    reopened.recover().unwrap();
    assert!(
        reopened
            .runs()
            .unwrap()
            .iter()
            .all(|r| r.status == "interrupted")
    );
}
async fn wait_pending(rt: &Runtime) -> Approval {
    for _ in 0..100 {
        if let Some(a) = rt
            .db
            .list::<Approval>("approval")
            .unwrap()
            .into_iter()
            .find(|a| a.status == "pending")
        {
            return a;
        }
        sleep(Duration::from_millis(10)).await
    }
    panic!("No approval appeared")
}
#[tokio::test]
async fn fixture_mcp_approval_rejection_and_revocation() {
    use axum::{Json, Router, extract::State, routing::post};
    async fn fixture(State(count): State<Arc<AtomicUsize>>, Json(v): Json<Value>) -> Json<Value> {
        let r = match v["method"].as_str().unwrap_or("") {
            "initialize" => {
                json!({"protocolVersion":"2025-03-26","capabilities":{"tools":{}},"serverInfo":{"name":"fixture","version":"1"}})
            }
            "tools/list" => {
                json!({"tools":[{"name":"publish","description":"Fixture only","inputSchema":{"type":"object"}}]})
            }
            "tools/call" => {
                count.fetch_add(1, Ordering::SeqCst);
                json!({"content":[{"type":"text","text":"fixture publication complete"}]})
            }
            _ => json!({}),
        };
        Json(json!({"jsonrpc":"2.0","id":v["id"],"result":r}))
    }
    let count = Arc::new(AtomicUsize::new(0));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}/mcp", listener.local_addr().unwrap());
    let server = tokio::spawn(
        axum::serve(
            listener,
            Router::new()
                .route("/mcp", post(fixture))
                .with_state(count.clone()),
        )
        .into_future(),
    );
    let rt = runtime();
    let i = Integration {
        name: "Studio".into(),
        url,
        ..Default::default()
    };
    rt.db.put("integration", &i.id, &i).unwrap();
    let mut a = agent(&rt, "Publisher");
    a.integrations.push(i.id.clone());
    rt.db.put("agent", &a.id, &a).unwrap();
    let r = run(&rt, &a, None);
    let args =
        json!({"integration_id":i.id,"tool":"publish","arguments":{"media":"fixture://image"}});
    let task = {
        let rt = rt.clone();
        let id = r.id.clone();
        let args = args.clone();
        tokio::spawn(async move { rt.tool(&id, "call_tool", args).await })
    };
    let approval = wait_pending(&rt).await;
    assert_eq!(count.load(Ordering::SeqCst), 0);
    rt.decide(&approval.id, false).unwrap();
    assert!(task.await.unwrap().is_err());
    assert_eq!(count.load(Ordering::SeqCst), 0);
    let task = {
        let rt = rt.clone();
        let id = r.id.clone();
        let args = args.clone();
        tokio::spawn(async move { rt.tool(&id, "call_tool", args).await })
    };
    let approval = wait_pending(&rt).await;
    rt.decide(&approval.id, true).unwrap();
    let result = task.await.unwrap();
    assert!(result.is_ok(), "{result:?}");
    assert_eq!(count.load(Ordering::SeqCst), 1);
    a.policies
        .insert(format!("{}:publish", i.id), "deny".into());
    rt.db.put("agent", &a.id, &a).unwrap();
    assert!(rt.tool(&r.id, "call_tool", args).await.is_err());
    assert_eq!(count.load(Ordering::SeqCst), 1);
    server.abort();
}
#[tokio::test]
async fn manual_handoff_and_cancel() {
    let rt = runtime();
    let a = agent(&rt, "A");
    let b = agent(&rt, "B");
    let g = Group {
        name: "G".into(),
        members: vec![a.id.clone(), b.id.clone()],
        automatic: false,
        ..Default::default()
    };
    rt.db.put("group", &g.id, &g).unwrap();
    let r = run(&rt, &a, Some(g.id));
    let task = {
        let rt = rt.clone();
        let id = r.id.clone();
        tokio::spawn(async move {
            rt.tool(
                &id,
                "send_to_agent",
                json!({"agent_id":b.id,"message":"Work","request_id":"1"}),
            )
            .await
        })
    };
    wait_pending(&rt).await;
    rt.cancel(&r.id).unwrap();
    assert!(task.await.unwrap().is_err());
    assert_eq!(rt.db.runs().unwrap().len(), 1);
    assert_eq!(
        rt.db.list::<Approval>("approval").unwrap()[0].status,
        "expired"
    );
}
#[test]
fn schedule_restart_sleep_overlap_and_no_replay() {
    let rt = runtime();
    let a = agent(&rt, "Scheduled");
    let local = (chrono::Utc::now() - chrono::Duration::days(2))
        .format("%Y-%m-%dT%H:%M")
        .to_string();
    let mut j = Job {
        name: "Daily".into(),
        agent_id: a.id.clone(),
        prompt: "Work".into(),
        local_time: local,
        recurrence: "daily".into(),
        ..Default::default()
    };
    schedule::validate(&mut j).unwrap();
    rt.db.put("job", &j.id, &j).unwrap();
    rt.tick(true).unwrap();
    assert!(rt.db.runs().unwrap().is_empty());
    assert_eq!(
        rt.db.list::<Value>("occurrence").unwrap()[0]["status"],
        "missed"
    );
    j = rt.db.get("job", &j.id).unwrap();
    assert!(j.next_at.as_ref().unwrap() > &now());
    j.next_at = Some((chrono::Utc::now() - chrono::Duration::seconds(1)).to_rfc3339());
    rt.db.put("job", &j.id, &j).unwrap();
    rt.tick(false).unwrap();
    assert_eq!(rt.db.runs().unwrap().len(), 1);
    rt.tick(false).unwrap();
    assert_eq!(rt.db.runs().unwrap().len(), 1);
    j.next_at = Some((chrono::Utc::now() - chrono::Duration::seconds(2)).to_rfc3339());
    rt.db.put("job", &j.id, &j).unwrap();
    rt.tick(false).unwrap();
    assert!(
        rt.db
            .list::<Value>("occurrence")
            .unwrap()
            .iter()
            .any(|o| o["status"] == "skipped")
    );
}
#[tokio::test]
async fn bridge_rejects_unauthenticated_and_browser_calls() {
    let rt = runtime();
    let server = tokio::spawn(mcp::serve(rt.clone()));
    for _ in 0..100 {
        if !rt.endpoint.lock().unwrap().is_empty() {
            break;
        }
        sleep(Duration::from_millis(10)).await
    }
    let url = format!("{}/mcp/wrong", rt.endpoint.lock().unwrap());
    let r = reqwest::Client::new()
        .post(&url)
        .json(&json!({"id":1,"method":"tools/list"}))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 401);
    rt.tokens
        .lock()
        .unwrap()
        .insert("test".into(), "none".into());
    let url = url.replace("wrong", "test");
    let r = reqwest::Client::new()
        .post(&url)
        .header("Origin", "https://untrusted.example")
        .json(&json!({"id":1,"method":"tools/list"}))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 403);
    rt.stopping.store(true, Ordering::SeqCst);
    server.await.unwrap().unwrap();
}
#[tokio::test]
#[ignore = "Uses an installed provider and its existing account; set OPENDOTS_TEST_PROVIDER"]
async fn live_provider_smoke() {
    let provider = std::env::var("OPENDOTS_TEST_PROVIDER").unwrap();
    let rt = runtime();
    let mut a = agent(&rt, "Smoke test");
    a.provider = provider;
    a.executable = std::env::var("OPENDOTS_TEST_EXECUTABLE").unwrap_or_default();
    rt.db.put("agent", &a.id, &a).unwrap();
    let mut r = run(&rt, &a, None);
    r.prompt = "Call propose_memory exactly once with content Smoke-test lesson, then reply OPENDOTS_OK. Use only the OpenDots tool.".into();
    let token = id();
    rt.tokens
        .lock()
        .unwrap()
        .insert(token.clone(), r.id.clone());
    let server = tokio::spawn(mcp::serve(rt.clone()));
    while rt.endpoint.lock().unwrap().is_empty() {
        sleep(Duration::from_millis(10)).await
    }
    let result = tokio::time::timeout(
        Duration::from_secs(90),
        crate::providers::execute(rt.clone(), &r, &token),
    )
    .await;
    let saved = rt.db.session(&a.id, &r.conversation_id, &a.provider);
    let mut followup = r.clone();
    followup.prompt = "Reply exactly OPENDOTS_RESUMED. Do not use tools.".into();
    let resumed = tokio::time::timeout(
        Duration::from_secs(90),
        crate::providers::execute(rt.clone(), &followup, &token),
    )
    .await;
    rt.stopping.store(true, Ordering::SeqCst);
    server.await.unwrap().unwrap();
    assert_eq!(saved, rt.db.session(&a.id, &r.conversation_id, &a.provider));
    assert!(
        resumed
            .expect("resume timed out")
            .expect("resume failed")
            .contains("OPENDOTS_RESUMED")
    );
    let memories = rt.db.list::<Memory>("memory").unwrap();
    assert_eq!(
        memories.len(),
        1,
        "Provider did not call its managed memory tool"
    );
    assert!(!memories[0].approved);
    let text = result
        .expect("provider timed out")
        .expect("provider failed");
    assert!(text.contains("OPENDOTS_OK"), "{text}");
    assert!(
        rt.db
            .session(&a.id, &r.conversation_id, &a.provider)
            .is_some()
    );
    assert!(
        rt.db
            .events(&r.id)
            .unwrap()
            .iter()
            .any(|e| e.kind == "text")
    );
}

#[cfg(unix)]
#[tokio::test]
async fn queue_limits_and_cancellation_release_owned_processes() {
    use std::os::unix::fs::PermissionsExt;
    let rt = runtime();
    let script = rt.root.join("fixture-provider");
    std::fs::write(&script,"#!/bin/sh\ncat >/dev/null\nprintf '%s\\n' '{\"type\":\"system\",\"session_id\":\"fixture\"}'\nsleep 30\n").unwrap();
    std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o700)).unwrap();
    let mut agents = Vec::new();
    for n in 0..5 {
        let mut a = agent(&rt, &format!("Agent {n}"));
        a.executable = script.to_string_lossy().into();
        rt.db.put("agent", &a.id, &a).unwrap();
        agents.push(a);
    }
    for a in &agents {
        for _ in 0..2 {
            rt.enqueue(
                SendInput {
                    agent_id: a.id.clone(),
                    group_id: None,
                    prompt: "Wait".into(),
                    job_id: None,
                },
                None,
            )
            .unwrap();
        }
    }
    *rt.endpoint.lock().unwrap() = "http://127.0.0.1:1".into();
    let dispatcher = tokio::spawn(rt.clone().dispatcher());
    for _ in 0..100 {
        if rt.controls.lock().unwrap().len() == 4 {
            break;
        }
        sleep(Duration::from_millis(10)).await;
    }
    assert_eq!(rt.controls.lock().unwrap().len(), 4);
    let running: Vec<_> = rt
        .db
        .runs()
        .unwrap()
        .into_iter()
        .filter(|r| r.status == "running")
        .collect();
    let unique: std::collections::HashSet<_> = running.iter().map(|r| &r.agent_id).collect();
    assert_eq!(unique.len(), 4);
    let cancelled = running[0].id.clone();
    rt.cancel(&cancelled).unwrap();
    for _ in 0..100 {
        if !rt.controls.lock().unwrap().contains_key(&cancelled) {
            break;
        }
        sleep(Duration::from_millis(10)).await;
    }
    assert!(!rt.controls.lock().unwrap().contains_key(&cancelled));
    rt.stopping.store(true, Ordering::SeqCst);
    dispatcher.await.unwrap();
    for _ in 0..100 {
        if rt.controls.lock().unwrap().is_empty() {
            break;
        }
        sleep(Duration::from_millis(10)).await;
    }
    assert!(rt.controls.lock().unwrap().is_empty());
}

#[cfg(unix)]
#[tokio::test]
async fn malformed_provider_output_and_early_exit_are_errors() {
    let mut p = crate::process::Process::spawn(
        "/bin/sh",
        &["-c".into(), "printf 'not-json\\n'".into()],
        std::path::Path::new("/tmp"),
    )
    .unwrap();
    assert!(p.next().await.unwrap_err().contains("malformed JSON"));
    p.stop().await;
    let mut p = crate::process::Process::spawn(
        "/bin/sh",
        &["-c".into(), "exit 1".into()],
        std::path::Path::new("/tmp"),
    )
    .unwrap();
    assert!(p.request(1, "initialize", json!({})).await.is_err());
    p.stop().await;
}

#[cfg(unix)]
#[tokio::test]
async fn stdio_mcp_initialization_and_tool_discovery() {
    let script = r#"read first
printf '%s\n' '{"jsonrpc":"2.0","id":1,"result":{"protocolVersion":"2025-03-26","capabilities":{"tools":{}},"serverInfo":{"name":"fixture","version":"1"}}}'
read initialized
read list
printf '%s\n' '{"jsonrpc":"2.0","id":2,"result":{"tools":[{"name":"generate","inputSchema":{"type":"object"}}]}}'
sleep 30
"#;
    let i = Integration {
        name: "Fixture stdio".into(),
        command: "/bin/sh".into(),
        args: vec!["-c".into(), script.into()],
        ..Default::default()
    };
    let mut client = mcp::Client::connect(&i).await.unwrap();
    let tools = client.request(2, "tools/list", json!({})).await.unwrap();
    assert_eq!(tools["tools"][0]["name"], "generate");
    client.close().await;
}

#[cfg(unix)]
#[tokio::test]
async fn cancellation_interrupts_pending_stdio_mcp_call() {
    let rt = runtime();
    let mut a = agent(&rt, "Tool runner");
    let script = r#"read first
printf '%s\n' '{"jsonrpc":"2.0","id":1,"result":{"protocolVersion":"2025-03-26","capabilities":{"tools":{}},"serverInfo":{"name":"fixture","version":"1"}}}'
read initialized
read call
sleep 30
"#;
    let i = Integration {
        name: "Waiting fixture".into(),
        command: "/bin/sh".into(),
        args: vec!["-c".into(), script.into()],
        ..Default::default()
    };
    rt.db.put("integration", &i.id, &i).unwrap();
    a.integrations.push(i.id.clone());
    a.policies.insert(format!("{}:wait", i.id), "allow".into());
    rt.db.put("agent", &a.id, &a).unwrap();
    let r = run(&rt, &a, None);
    let task = {
        let rt = rt.clone();
        let id = r.id.clone();
        tokio::spawn(async move {
            rt.tool(
                &id,
                "call_tool",
                json!({"integration_id":i.id,"tool":"wait","arguments":{}}),
            )
            .await
        })
    };
    sleep(Duration::from_millis(150)).await;
    rt.cancel(&r.id).unwrap();
    let result = tokio::time::timeout(Duration::from_secs(2), task)
        .await
        .expect("Cancellation did not interrupt the MCP request")
        .unwrap();
    assert!(result.unwrap_err().contains("cancelled"));
}
