mod browser;
mod commands;
mod db;
mod mcp;
mod models;
mod process;
mod providers;
mod runtime;
mod schedule;
use runtime::Runtime;
use std::{
    collections::HashMap,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
};
use tauri::{
    Manager,
    menu::{Menu, MenuItem},
    tray::TrayIconBuilder,
};

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let app = tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _, _| {
            if let Some(window) = app.get_webview_window("main") {
                let _ = window.show();
                let _ = window.set_focus();
            }
        }))
        .plugin(tauri_plugin_opener::init())
        .setup(|app| {
            let root = app.path().app_data_dir()?;
            std::fs::create_dir_all(&root)?;
            let db =
                db::Store::open(&root.join("opendots.sqlite")).map_err(std::io::Error::other)?;
            db.recover().map_err(std::io::Error::other)?;
            let rt = Arc::new(Runtime {
                db,
                browsers: Default::default(),
                app: Some(app.handle().clone()),
                root,
                controls: Mutex::new(HashMap::new()),
                tokens: Mutex::new(HashMap::new()),
                mutations: Mutex::new(()),
                endpoint: Mutex::new(String::new()),
                stopping: AtomicBool::new(false),
            });
            rt.tick(true).map_err(std::io::Error::other)?;
            app.manage(rt.clone());
            let bridge = rt.clone();
            tauri::async_runtime::spawn(async move {
                if let Err(e) = mcp::serve(bridge.clone()).await {
                    bridge.event("system", "error", serde_json::json!({"error":e}));
                    bridge.stopping.store(true, Ordering::SeqCst);
                }
            });
            let dispatch = rt.clone();
            tauri::async_runtime::spawn(async move {
                while dispatch.endpoint.lock().unwrap().is_empty() {
                    if dispatch.stopping.load(Ordering::SeqCst) {
                        return;
                    }
                    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
                }
                dispatch.dispatcher().await;
            });
            tauri::async_runtime::spawn(async move {
                while !rt.stopping.load(Ordering::SeqCst) {
                    if let Err(e) = rt.tick(false) {
                        rt.event("system", "error", serde_json::json!({"error":e}));
                    }
                    tokio::time::sleep(std::time::Duration::from_secs(1)).await;
                }
            });
            let show = MenuItem::with_id(app, "show", "Open OpenDots", true, None::<&str>)?;
            let quit = MenuItem::with_id(app, "quit", "Quit OpenDots", true, None::<&str>)?;
            let menu = Menu::with_items(app, &[&show, &quit])?;
            let mut tray = TrayIconBuilder::new()
                .tooltip("OpenDots — jobs run while this app is open")
                .menu(&menu)
                .on_menu_event(|app, event| match event.id.as_ref() {
                    "show" => {
                        if let Some(w) = app.get_webview_window("main") {
                            let _ = w.show();
                            let _ = w.set_focus();
                        }
                    }
                    "quit" => {
                        let rt = app.state::<Arc<Runtime>>().inner().clone();
                        rt.stopping.store(true, Ordering::SeqCst);
                        let app = app.clone();
                        tauri::async_runtime::spawn(async move {
                            for _ in 0..50 {
                                if rt.controls.lock().unwrap().is_empty() {
                                    break;
                                }
                                tokio::time::sleep(std::time::Duration::from_millis(100)).await;
                            }
                            rt.browser_shutdown().await;
                            app.exit(0);
                        });
                    }
                    _ => {}
                });
            if let Some(icon) = app.default_window_icon() {
                tray = tray.icon(icon.clone());
            }
            tray.build(app)?;
            Ok(())
        })
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                api.prevent_close();
                let _ = window.hide();
            }
        })
        .invoke_handler(tauri::generate_handler![
            browser::commands::browser_profiles,
            browser::commands::save_browser_profile,
            browser::commands::delete_browser_profile,
            browser::commands::browser_control,
            browser::commands::browser_status,
            browser::commands::browser_artifacts,
            browser::commands::read_browser_artifact,
            browser::commands::export_browser_artifact,
            browser::commands::delete_browser_artifact,
            browser::commands::add_browser_upload,
            browser::commands::replay_browser,
            commands::snapshot,
            commands::save_entity,
            commands::delete_entity,
            commands::messages,
            commands::execution_events,
            commands::send_message,
            commands::cancel_execution,
            commands::retry_execution,
            commands::resolve_approval,
            commands::run_job,
            commands::discover_provider,
            commands::check_integration,
            commands::set_secret,
            commands::import_skill,
            commands::create_example
        ])
        .build(tauri::generate_context!())
        .expect("error building OpenDots");
    app.run(|app, event| {
        if let tauri::RunEvent::ExitRequested { api, .. } = &event
            && let Some(rt) = app.try_state::<Arc<Runtime>>()
            && !rt.stopping.swap(true, Ordering::SeqCst)
        {
            api.prevent_exit();
            let rt = rt.inner().clone();
            let app = app.clone();
            tauri::async_runtime::spawn(async move {
                for _ in 0..50 {
                    if rt.controls.lock().unwrap().is_empty() {
                        break;
                    }
                    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
                }
                rt.browser_shutdown().await;
                app.exit(0);
            });
        }
        if matches!(event, tauri::RunEvent::Exit) {
            process::shutdown_all();
        }
        if let tauri::RunEvent::Exit = event
            && let Some(rt) = app.try_state::<Arc<Runtime>>()
        {
            rt.stopping.store(true, Ordering::SeqCst);
            for c in rt.controls.lock().unwrap().values() {
                c.store(true, Ordering::SeqCst);
            }
        }
    });
}

#[cfg(test)]
mod tests;
