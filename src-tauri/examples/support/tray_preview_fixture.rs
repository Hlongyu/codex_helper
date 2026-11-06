//! Synthetic IPC only for the independently built native preview example.
#![cfg(target_os = "macos")]
use serde_json::{json, Value};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Mutex,
};
use tauri::{Emitter, Manager};

#[derive(Default)]
pub struct Fixture {
    pub visible: AtomicBool,
    state: Mutex<Option<Value>>,
}

fn initial() -> Value {
    json!({
        "app_version":"原生隔离预览", "router_status":{"running":true,"address":"127.0.0.1:18080","error":null},
        "clients":{"codex":true,"claude":true,"pi":false},
        "codex_model":"preview-model-a", "codex_models":["preview-model-a","preview-model-b"],
        "codex_provider_count":2,"claude_provider_count":1,
        "today":{"request_count":128,"total_tokens":1234000,"output_tokens":124000},
        "usage_error":null,"panel_visible":false
    })
}

#[tauri::command]
pub fn load_tray_snapshot(state: tauri::State<Fixture>) -> Value {
    let mut stored = state.state.lock().unwrap();
    let value = stored.get_or_insert_with(initial);
    value["panel_visible"] = json!(state.visible.load(Ordering::Relaxed));
    value.clone()
}

#[tauri::command]
pub fn update_tray_client(payload: Value, state: tauri::State<Fixture>) -> Result<Value, String> {
    {
        let mut stored = state.state.lock().unwrap();
        let value = stored.get_or_insert_with(initial);
        if let (Some(client), Some(enabled)) =
            (payload["client"].as_str(), payload["enabled"].as_bool())
        {
            if !["codex", "claude", "pi"].contains(&client) {
                return Err("未知模拟客户端".into());
            }
            value["clients"][client] = json!(enabled);
        } else if let Some(model) = payload["codex_model"].as_str() {
            if value["clients"]["codex"] != true {
                return Err("请先开启 Codex 接管".into());
            }
            if !["preview-model-a", "preview-model-b"].contains(&model) {
                return Err("模拟模型不存在".into());
            }
            value["codex_model"] = json!(model);
        } else {
            return Err("未知模拟变更".into());
        }
    }
    Ok(load_tray_snapshot(state))
}

#[tauri::command]
pub fn tray_action(action: String, app: tauri::AppHandle) {
    if action == "quit" {
        app.exit(0);
        return;
    }
    // Navigation never opens a real main window in this isolated preview.
    let _ = super::tray_surface::macos::hide();
    app.state::<Fixture>()
        .visible
        .store(false, Ordering::Relaxed);
    let _ = app.emit_to(super::tray_surface::LABEL, "tray-visibility", false);
}

pub fn install_tray(app: &tauri::AppHandle) -> tauri::Result<()> {
    use tauri::tray::TrayIconEvent;
    let tray = super::tray_surface::tray_icon_builder("native-preview")
        .title("预览")
        .tooltip("XXSwitch 原生隔离预览")
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button,
                button_state,
                ..
            } = event
            {
                if !super::tray_surface::interaction::opens_panel(button, button_state) {
                    return;
                }
                let app = tray.app_handle().clone();
                if app.state::<Fixture>().visible.load(Ordering::Relaxed) {
                    tray_action("hide".into(), app);
                } else {
                    let result = tray.with_inner_tray_icon(|inner| {
                        let item = inner
                            .ns_status_item()
                            .ok_or("preview status item unavailable")?;
                        super::tray_surface::macos::position(&item, true)?;
                        super::tray_surface::macos::show()
                    });
                    if matches!(result, Ok(Ok(()))) {
                        app.state::<Fixture>()
                            .visible
                            .store(true, Ordering::Relaxed);
                        let _ = app.emit_to(super::tray_surface::LABEL, "tray-visibility", true);
                    }
                }
            }
        })
        .build(app)?;
    super::tray_surface::direct_panel_clicks(&tray)?;
    // This isolated example opens its synthetic panel on launch so a background
    // UI inspector can attach to this tray-only app without opening a main window.
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_millis(350)).await;
        let target = app.clone();
        let _ = app.run_on_main_thread(move || {
            let tray = target.tray_by_id("native-preview").unwrap();
            let result = tray.with_inner_tray_icon(|inner| {
                let item = inner
                    .ns_status_item()
                    .ok_or("preview status item unavailable")?;
                super::tray_surface::macos::position(&item, false)?;
                super::tray_surface::macos::show()
            });
            if matches!(result, Ok(Ok(()))) {
                target
                    .state::<Fixture>()
                    .visible
                    .store(true, Ordering::Relaxed);
                let _ = target.emit_to(super::tray_surface::LABEL, "tray-visibility", true);
            }
        });
    });
    Ok(())
}
