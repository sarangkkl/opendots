use super::*;
use crate::tests::runtime;
use tokio::time::{Duration, sleep};

#[test]
fn multimodal_tool_results_preserve_images() {
    let output = ToolOutput {
        content: vec![
            Content::Text {
                text: "snapshot".into(),
            },
            Content::Image {
                data: "YWJj".into(),
                mime_type: "image/png".into(),
            },
        ],
    };
    let mcp = serde_json::to_value(&output).unwrap();
    assert_eq!(mcp["content"][1]["type"], "image");
    assert_eq!(mcp["content"][1]["mimeType"], "image/png");
    let codex = output.codex();
    assert_eq!(codex[1]["type"], "inputImage");
    assert_eq!(codex[1]["imageUrl"], "data:image/png;base64,YWJj");
}
fn setup(rt: &Runtime, name: &str) -> (Agent, Profile, Execution) {
    let a = Agent {
        name: name.into(),
        ..Default::default()
    };
    rt.db.put("agent", &a.id, &a).unwrap();
    let profile = Profile {
        id: id(),
        agent_id: a.id.clone(),
        name: name.into(),
        executable: std::env::var("OPENDOTS_TEST_CHROME").unwrap_or_default(),
    };
    rt.db.put("browser_profile", &profile.id, &profile).unwrap();
    let run = rt
        .enqueue(
            SendInput {
                agent_id: a.id.clone(),
                group_id: None,
                job_id: None,
                prompt: "Browser fixture".into(),
            },
            None,
        )
        .unwrap();
    rt.status(&run.id, "running", None);
    (a, profile, run)
}
async fn observe(rt: &Runtime, run: &str) -> Value {
    let result = rt
        .browser_tool(run, "browser_observe", json!({"screenshot":true}))
        .await
        .unwrap();
    assert!(matches!(result.content[1], Content::Image { .. }));
    let Content::Text { text } = &result.content[0] else {
        panic!("no state")
    };
    serde_json::from_str(text).unwrap()
}
async fn approved(rt: Arc<Runtime>, run: &str, args: Value, allow: bool) -> Result<ToolOutput> {
    let worker = rt.clone();
    let run_id = run.to_owned();
    let task =
        tokio::spawn(async move { worker.browser_tool(&run_id, "browser_action", args).await });
    for _ in 0..100 {
        if let Some(a) = rt
            .db
            .list::<Approval>("approval")
            .unwrap()
            .into_iter()
            .find(|a| a.execution_id == run && a.status == "pending")
        {
            rt.decide(&a.id, allow).unwrap();
            return task.await.unwrap();
        }
        sleep(Duration::from_millis(50)).await;
    }
    panic!("Expected approval; task finished: {}", task.is_finished())
}
#[tokio::test]
#[ignore = "Launches installed Chrome against a local fixture only"]
async fn browser_fixture_approvals_profiles_recording_replay() {
    use axum::{Router, response::Html, routing::get};
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        axum::serve(listener,Router::new().route("/",get(||async{Html(r#"<!doctype html><title>Browser fixture</title><button id="publish" onclick="document.querySelector('#result').textContent='Published fixture';document.cookie='login=sira;max-age=3600;path=/'">Publish fixture</button><p id="result">Draft</p><input aria-label="Caption"><input type="file" aria-label="Media"><select aria-label="Audience"><option value="private">Private</option><option value="public">Public</option></select><div id="shadow"></div><iframe src="/frame"></iframe><script>const root=document.querySelector('#shadow').attachShadow({mode:'open'});root.innerHTML='<button>Shadow action</button>';</script>"#)})).route("/frame",get(||async{Html("<button>Frame action</button>")}))).await.unwrap()
    });
    let rt = runtime();
    let (a, profile, run) = setup(&rt, "Publisher");
    let state = observe(&rt, &run.id).await;
    approved(rt.clone(),&run.id,json!({"action":"navigate","snapshot":state["snapshot"],"url":format!("http://{address}/")}),true).await.unwrap();
    sleep(Duration::from_millis(500)).await;
    let state = observe(&rt, &run.id).await;
    assert!(
        state["elements"]
            .as_array()
            .unwrap()
            .iter()
            .any(|e| e["name"] == "Shadow action")
    );
    assert!(
        state["elements"]
            .as_array()
            .unwrap()
            .iter()
            .any(|e| e["name"] == "Frame action")
    );
    let index = state["elements"]
        .as_array()
        .unwrap()
        .iter()
        .find(|e| e["name"] == "Publish fixture")
        .unwrap()["index"]
        .clone();
    let action = json!({"action":"click","snapshot":state["snapshot"],"index":index});
    assert!(
        approved(rt.clone(), &run.id, action.clone(), false)
            .await
            .is_err()
    );
    let session = rt.acquire_browser(&run).await.unwrap();
    {
        let s = session.lock().await;
        assert_eq!(
            s.cdp
                .eval(
                    &s.tab,
                    "document.querySelector('#result').textContent".into()
                )
                .await
                .unwrap(),
            "Draft"
        );
    }
    approved(rt.clone(), &run.id, action.clone(), true)
        .await
        .unwrap();
    sleep(Duration::from_millis(300)).await;
    assert!(
        rt.browser_tool(&run.id, "browser_action", action)
            .await
            .unwrap_err()
            .contains("Observe")
    );
    // Approve after a page mutation: no click may be sent.
    let state = observe(&rt, &run.id).await;
    let worker = rt.clone();
    let run_id = run.id.clone();
    let task = tokio::spawn(async move {
        worker
            .browser_tool(
                &run_id,
                "browser_action",
                json!({"action":"click","snapshot":state["snapshot"],"index":index}),
            )
            .await
    });
    for _ in 0..100 {
        if let Some(approval) = rt
            .db
            .list::<Approval>("approval")
            .unwrap()
            .into_iter()
            .find(|a| a.status == "pending")
        {
            let s = session.lock().await;
            s.cdp
                .eval(
                    &s.tab,
                    "document.querySelector('#result').textContent='Changed during approval'"
                        .into(),
                )
                .await
                .unwrap();
            drop(s);
            sleep(Duration::from_millis(50)).await;
            rt.decide(&approval.id, true).unwrap();
            break;
        }
        sleep(Duration::from_millis(50)).await;
    }
    assert!(task.await.unwrap().unwrap_err().contains("changed"));
    sleep(Duration::from_millis(300)).await;
    rt.browser_release(&run.id).await;
    let videos: Vec<_> = std::fs::read_dir(rt.browser_dir(&run.id))
        .unwrap()
        .flatten()
        .filter(|e| e.path().extension().is_some_and(|s| s == "webm"))
        .collect();
    assert!(!videos.is_empty(), "WebM must finalize");
    assert_eq!(
        &std::fs::read(videos[0].path()).unwrap()[..4],
        &[0x1a, 0x45, 0xdf, 0xa3]
    );
    rt.status(&run.id, "completed", None);
    rt.browser_manual(&profile.id, "close").await.unwrap();
    // Same profile after browser restart retains login; a different agent does not.
    let session = rt.browser_session(&profile, &run.id).await.unwrap();
    {
        let s = session.lock().await;
        s.cdp
            .call(
                &s.tab,
                "Page.navigate",
                json!({"url":format!("http://{address}/")}),
            )
            .await
            .unwrap();
        sleep(Duration::from_millis(300)).await;
        assert!(
            s.cdp
                .eval(&s.tab, "document.cookie".into())
                .await
                .unwrap()
                .as_str()
                .unwrap()
                .contains("login=sira")
        );
    }
    let (_, other, other_run) = setup(&rt, "Researcher");
    let session = rt.browser_session(&other, &other_run.id).await.unwrap();
    {
        let s = session.lock().await;
        s.cdp
            .call(
                &s.tab,
                "Page.navigate",
                json!({"url":format!("http://{address}/")}),
            )
            .await
            .unwrap();
        sleep(Duration::from_millis(300)).await;
        assert_eq!(
            s.cdp.eval(&s.tab, "document.cookie".into()).await.unwrap(),
            ""
        );
    }
    rt.browser_release(&run.id).await;
    let replay = rt
        .enqueue(
            SendInput {
                agent_id: a.id.clone(),
                group_id: None,
                job_id: None,
                prompt: "Replay fixture".into(),
            },
            None,
        )
        .unwrap();
    rt.status(&replay.id, "running", None);
    let worker = rt.clone();
    let replay_id = replay.id.clone();
    let source_id = run.id.clone();
    let replay_task =
        tokio::spawn(async move { worker.browser_replay_run(&replay_id, &source_id).await });
    let mut approvals = 0;
    for _ in 0..200 {
        for approval in rt
            .db
            .list::<Approval>("approval")
            .unwrap()
            .into_iter()
            .filter(|a| a.execution_id == replay.id && a.status == "pending")
        {
            approvals += 1;
            rt.decide(&approval.id, true).unwrap();
        }
        if replay_task.is_finished() {
            break;
        }
        sleep(Duration::from_millis(50)).await;
    }
    assert_eq!(
        approvals, 2,
        "Replay needs fresh approvals for both actions"
    );
    assert!(replay_task.await.unwrap().unwrap().contains("Replayed 2"));
    rt.browser_release(&replay.id).await;
    assert_eq!(rt.browser_profile(&a.id).unwrap().id, profile.id);
    rt.browser_shutdown().await;
    server.abort();
    eprintln!("Browser evidence: {}", rt.browser_dir(&run.id).display());
}

#[test]
fn browser_artifacts_require_explicit_agent_sharing() {
    let rt = runtime();
    let (a, _, run) = setup(&rt, "Creator");
    let (b, _, _) = setup(&rt, "Publisher");
    let dir = rt.browser_dir(&run.id);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("media.png"), b"fixture").unwrap();
    let reference = format!("browser:{}/media.png", run.id);
    assert!(rt.browser_file(&a.id, &reference).is_ok());
    assert!(rt.browser_file(&b.id, &reference).is_err());
    rt.db
        .put("browser_share", &format!("{}:{reference}", b.id), &true)
        .unwrap();
    assert!(rt.browser_file(&b.id, &reference).is_ok());
    assert!(
        rt.browser_file(&a.id, &format!("browser:{}/../../opendots.sqlite", run.id))
            .is_err()
    );
}

#[tokio::test]
#[ignore = "Uses installed provider login and an isolated local Chrome vision fixture"]
async fn live_browser_provider_vision() {
    use axum::{Router, response::Html, routing::get};
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let web = tokio::spawn(async move {
        axum::serve(listener,Router::new().route("/",get(||async{Html(r#"<!doctype html><title>Vision fixture</title><canvas width="600" height="400"></canvas><script>const c=document.querySelector('canvas').getContext('2d');c.fillStyle='white';c.fillRect(0,0,600,400);c.fillStyle='#00aa00';c.beginPath();c.moveTo(300,40);c.lineTo(70,360);c.lineTo(530,360);c.closePath();c.fill();</script>"#)}))).await.unwrap()
    });
    let rt = runtime();
    let (mut agent, _, mut run) = setup(&rt, "Vision verifier");
    agent.provider = std::env::var("OPENDOTS_TEST_PROVIDER").unwrap();
    rt.db.put("agent", &agent.id, &agent).unwrap();
    let session = rt.acquire_browser(&run).await.unwrap();
    {
        let s = session.lock().await;
        s.cdp
            .call(
                &s.tab,
                "Page.navigate",
                json!({"url":format!("http://{address}/")}),
            )
            .await
            .unwrap();
    }
    sleep(Duration::from_millis(300)).await;
    run.prompt="Call browser_observe with screenshot true. Based only on the returned screenshot, describe the large shape and its color. Do not navigate, interact, or use other tools. End your answer with VISION_OK.".into();
    let token = id();
    rt.tokens
        .lock()
        .unwrap()
        .insert(token.clone(), run.id.clone());
    let bridge = tokio::spawn(crate::mcp::serve(rt.clone()));
    while rt.endpoint.lock().unwrap().is_empty() {
        sleep(Duration::from_millis(20)).await;
    }
    let result = tokio::time::timeout(
        Duration::from_secs(120),
        crate::providers::execute(rt.clone(), &run, &token),
    )
    .await;
    rt.browser_shutdown().await;
    rt.stopping.store(true, Ordering::SeqCst);
    bridge.await.unwrap().unwrap();
    web.abort();
    let answer = result
        .expect("Vision run timed out")
        .expect("Provider failed");
    eprintln!("Vision answer: {answer}");
    assert!(
        answer.to_lowercase().contains("green") && answer.to_lowercase().contains("triangle"),
        "Provider did not identify screenshot content: {answer}"
    );
    assert!(
        rt.db
            .events(&run.id)
            .unwrap()
            .iter()
            .any(|e| e.kind == "browser_screenshot")
    );
}

#[tokio::test]
#[ignore = "Launches installed Chrome against local file/action fixtures"]
async fn browser_fixture_files_tabs_takeover_and_cancel() {
    use axum::{Router, http::header, response::Html, routing::get};
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let web = tokio::spawn(async move {
        axum::serve(listener,Router::new().route("/",get(||async{Html(r#"<!doctype html><title>Action fixture</title><input aria-label="Caption"><input type="file" aria-label="Media"><select aria-label="Audience"><option value="private">Private</option><option value="public">Public</option></select><button onclick="this.textContent='Clicked'">Coordinate action</button><div style="height:2400px">Scroll fixture</div>"#)})).route("/file",get(||async{([(header::CONTENT_DISPOSITION,"attachment; filename=fixture.txt")],"fixture download")}))).await.unwrap()
    });
    let rt = runtime();
    let (mut a, p, run) = setup(&rt, "Actions");
    for action in ACTIONS {
        a.policies
            .insert(format!("browser:{action}"), "allow".into());
    }
    rt.db.put("agent", &a.id, &a).unwrap();
    let state = observe(&rt, &run.id).await;
    rt.browser_tool(&run.id,"browser_action",json!({"action":"navigate","snapshot":state["snapshot"],"url":format!("http://{address}/")})).await.unwrap();
    sleep(Duration::from_millis(300)).await;
    let state = observe(&rt, &run.id).await;
    let index = state["elements"]
        .as_array()
        .unwrap()
        .iter()
        .find(|e| e["name"] == "Caption")
        .unwrap()["index"]
        .clone();
    rt.browser_tool(&run.id,"browser_action",json!({"action":"type","snapshot":state["snapshot"],"index":index,"text":"A fixture caption"})).await.unwrap();
    let state = observe(&rt, &run.id).await;
    let index = state["elements"]
        .as_array()
        .unwrap()
        .iter()
        .find(|e| e["name"] == "Audience")
        .unwrap()["index"]
        .clone();
    rt.browser_tool(
        &run.id,
        "browser_action",
        json!({"action":"select","snapshot":state["snapshot"],"index":index,"text":"public"}),
    )
    .await
    .unwrap();
    let file = rt.root.join("upload.txt");
    std::fs::write(&file, "fixture upload").unwrap();
    let artifact = id();
    rt.db
        .put(
            "browser_upload",
            &artifact,
            &json!({"id":artifact,"agent_id":a.id,"path":file,"name":"upload.txt"}),
        )
        .unwrap();
    let state = observe(&rt, &run.id).await;
    let index = state["elements"]
        .as_array()
        .unwrap()
        .iter()
        .find(|e| e["name"] == "Media")
        .unwrap()["index"]
        .clone();
    rt.browser_tool(
        &run.id,
        "browser_action",
        json!({"action":"upload","snapshot":state["snapshot"],"index":index,"artifact":artifact}),
    )
    .await
    .unwrap();
    let session = rt.acquire_browser(&run).await.unwrap();
    {
        let s = session.lock().await;
        assert_eq!(
            s.cdp
                .eval(&s.tab, "document.querySelector('input').value".into())
                .await
                .unwrap(),
            "A fixture caption"
        );
        assert_eq!(
            s.cdp
                .eval(&s.tab, "document.querySelector('select').value".into())
                .await
                .unwrap(),
            "public"
        );
        assert_eq!(
            s.cdp
                .eval(
                    &s.tab,
                    "document.querySelector('input[type=file]').files[0].name".into()
                )
                .await
                .unwrap(),
            "upload.txt"
        );
    }
    let state = observe(&rt, &run.id).await;
    let target = state["elements"]
        .as_array()
        .unwrap()
        .iter()
        .find(|e| e["name"] == "Coordinate action")
        .unwrap();
    rt.browser_tool(&run.id,"browser_action",json!({"action":"click_at","snapshot":state["snapshot"],"x":target["x"].as_f64().unwrap()+5.,"y":target["y"].as_f64().unwrap()+5.})).await.unwrap();
    let state = observe(&rt, &run.id).await;
    assert!(
        state["elements"]
            .as_array()
            .unwrap()
            .iter()
            .any(|e| e["name"] == "Clicked")
    );
    rt.browser_tool(
        &run.id,
        "browser_action",
        json!({"action":"scroll","snapshot":state["snapshot"],"y":500}),
    )
    .await
    .unwrap();
    sleep(Duration::from_millis(300)).await;
    let state = observe(&rt, &run.id).await;
    assert!(state["viewport"]["scrollY"].as_f64().unwrap() > 0.);
    let first = state["tab"].clone();
    rt.browser_tool(
        &run.id,
        "browser_action",
        json!({"action":"new_tab","snapshot":state["snapshot"],"url":format!("http://{address}/")}),
    )
    .await
    .unwrap();
    sleep(Duration::from_millis(300)).await;
    let state = observe(&rt, &run.id).await;
    assert_ne!(first, state["tab"]);
    rt.browser_tool(
        &run.id,
        "browser_action",
        json!({"action":"switch_tab","snapshot":state["snapshot"],"tab":first}),
    )
    .await
    .unwrap();
    let state = observe(&rt, &run.id).await;
    rt.browser_tool(&run.id,"browser_action",json!({"action":"download","snapshot":state["snapshot"],"url":format!("http://{address}/file")})).await.unwrap();
    sleep(Duration::from_millis(500)).await;
    let downloaded: Vec<_> = std::fs::read_dir(rt.browser_dir(&run.id).join("downloads"))
        .unwrap()
        .flatten()
        .collect();
    assert!(
        downloaded
            .iter()
            .any(|e| std::fs::read_to_string(e.path()).is_ok_and(|s| s == "fixture download"))
    );
    rt.browser_manual(&p.id, "takeover").await.unwrap();
    let view = rt.browser_view(&run.id).await.unwrap();
    assert_eq!(view["manual"], true);
    assert!(view["frame"].is_null());
    let worker = rt.clone();
    let run_id = run.id.clone();
    let waiting = tokio::spawn(async move {
        worker
            .browser_tool(&run_id, "browser_observe", json!({}))
            .await
    });
    sleep(Duration::from_millis(200)).await;
    assert!(!waiting.is_finished());
    rt.cancel(&run.id).unwrap();
    assert!(waiting.await.unwrap().is_err());
    rt.browser_release(&run.id).await;
    rt.browser_shutdown().await;
    web.abort();
}

#[cfg(unix)]
#[tokio::test]
async fn live_profile_lock_is_not_replaced() {
    let dir = std::env::temp_dir().join(format!("opendots-lock-{}", id()));
    std::fs::create_dir_all(&dir).unwrap();
    std::os::unix::fs::symlink(
        format!("local-{}", std::process::id()),
        dir.join("SingletonLock"),
    )
    .unwrap();
    let error = match cdp::launch("does-not-exist", &dir, true).await {
        Ok(_) => panic!("Must refuse locked profile"),
        Err(e) => e,
    };
    assert!(error.contains("locked"));
    assert!(std::fs::read_link(dir.join("SingletonLock")).is_ok());
    std::fs::remove_dir_all(dir).unwrap();
}

#[tokio::test]
#[ignore = "Launches isolated Chrome to verify autonomous process recovery"]
async fn browser_fixture_relaunches_exited_profile_without_replaying_actions() {
    let rt = runtime();
    let (_, profile, run) = setup(&rt, "Recovery");
    let before = observe(&rt, &run.id).await;
    let original = rt.acquire_browser(&run).await.unwrap();
    {
        let mut s = original.lock().await;
        s.process.stop().await;
    }
    let pending = Approval {
        id: id(),
        execution_id: run.id.clone(),
        kind: "browser".into(),
        payload: json!({"action":"click"}),
        status: "pending".into(),
        created_at: now(),
    };
    rt.db.put("approval", &pending.id, &pending).unwrap();
    // An action from the old page must never execute in a replacement browser.
    let rejected = rt
        .browser_tool(
            &run.id,
            "browser_action",
            json!({"action":"navigate","snapshot":before["snapshot"],"url":"https://example.com"}),
        )
        .await
        .unwrap_err();
    assert!(rejected.contains("Observe"));
    assert_eq!(
        rt.db
            .get::<Approval>("approval", &pending.id)
            .unwrap()
            .status,
        "expired"
    );
    let after = observe(&rt, &run.id).await;
    assert_ne!(before["snapshot"], after["snapshot"]);
    let recovered = rt.acquire_browser(&run).await.unwrap();
    assert!(!Arc::ptr_eq(&original, &recovered));
    assert_eq!(rt.browser_profile(&run.agent_id).unwrap().id, profile.id);
    assert!(
        rt.db
            .events(&run.id)
            .unwrap()
            .iter()
            .any(|e| e.kind == "browser_restarted")
    );
    assert!(
        !rt.db
            .events(&run.id)
            .unwrap()
            .iter()
            .any(|e| e.kind == "browser_action_started")
    );
    rt.browser_shutdown().await;
}
