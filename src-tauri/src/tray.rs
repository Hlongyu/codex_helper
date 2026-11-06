//! Native tray integration. The compact window receives summaries, never provider credentials.
use super::*;
#[cfg(target_os = "linux")]
use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::TrayIconEvent;
#[cfg(not(target_os = "macos"))]
use tauri::PhysicalSize;
use tauri::{Emitter, PhysicalPosition, WebviewUrl, WebviewWindow};

const TRAY_ID: &str = "status";
pub(super) const PANEL_LABEL: &str = tray_surface::LABEL;
#[cfg(any(not(target_os = "macos"), test))]
const PANEL_WIDTH: f64 = tray_surface::WIDTH;
#[cfg(any(not(target_os = "macos"), test))]
const PANEL_HEIGHT: f64 = tray_surface::HEIGHT;

#[derive(Default)]
pub(super) struct TrayRuntime {
    generation: AtomicU64,
    focused_generation: AtomicU64,
    navigation: Mutex<Option<String>>,
    visible: std::sync::atomic::AtomicBool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(super) struct TrayPreferences {
    #[serde(default = "default_true")]
    visible: bool,
}

impl Default for TrayPreferences {
    fn default() -> Self {
        Self { visible: true }
    }
}

fn preferences() -> Result<TrayPreferences, String> {
    let path = manager_dir()?.join("tray-preferences.json");
    match fs::read(&path) {
        Ok(bytes) => serde_json::from_slice(&bytes).map_err(|e| format!("托盘偏好无法读取：{e}")),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(TrayPreferences::default()),
        Err(e) => Err(format!("托盘偏好无法读取：{e}")),
    }
}

#[tauri::command]
pub(super) fn get_tray_preferences() -> Result<TrayPreferences, String> {
    preferences()
}

#[tauri::command]
pub(super) fn set_tray_visibility(
    visible: bool,
    app: tauri::AppHandle,
) -> Result<TrayPreferences, String> {
    let previous = preferences()?;
    let icon = app.tray_by_id(TRAY_ID).ok_or("托盘图标暂不可用")?;
    icon.set_visible(visible).map_err(|e| e.to_string())?;
    let next = TrayPreferences { visible };
    let path = manager_dir()?.join("tray-preferences.json");
    let bytes = serde_json::to_vec_pretty(&next).map_err(|e| e.to_string())?;
    if let Err(error) = provider_sync::atomic_write(&path, &bytes) {
        let _ = icon.set_visible(previous.visible);
        return Err(error);
    }
    if !visible {
        hide_panel(&app);
    }
    Ok(next)
}

#[derive(Debug, Serialize)]
pub(super) struct TraySnapshot {
    app_version: String,
    router_status: RouterStatus,
    clients: BTreeMap<&'static str, bool>,
    codex_model: String,
    codex_models: Vec<String>,
    codex_provider_count: usize,
    claude_provider_count: usize,
    today: Option<RouteUsageSummary>,
    usage_error: Option<String>,
    panel_visible: bool,
}

fn require_panel(window: &WebviewWindow) -> Result<(), String> {
    if window.label() == PANEL_LABEL {
        Ok(())
    } else {
        Err("此操作仅用于快捷面板".into())
    }
}

#[tauri::command]
pub(super) async fn load_tray_snapshot(
    app: tauri::AppHandle,
    window: WebviewWindow,
    runtime: tauri::State<'_, RouterRuntime>,
) -> Result<TraySnapshot, String> {
    require_panel(&window)?;
    snapshot(&app, &runtime).await
}

async fn snapshot(app: &tauri::AppHandle, runtime: &RouterRuntime) -> Result<TraySnapshot, String> {
    let state = load_state_file()?;
    let day = Local::now().format("%Y-%m-%d").to_string();
    let usage = load_route_usage_stats(Some(LoadRouteLogsPayload {
        filter: Some(RouteLogFilter {
            start_day: Some(day.clone()),
            end_day: Some(day),
            page_size: Some(1),
            ..Default::default()
        }),
    }))
    .await;
    let (today, usage_error) = match usage {
        Ok(stats) => (Some(stats.summary), None),
        Err(e) => (None, Some(e)),
    };
    Ok(TraySnapshot {
        app_version: env!("XXSWITCH_VERSION").to_string(),
        router_status: router_status(runtime, &state.router),
        clients: BTreeMap::from([
            ("codex", state.clients.codex.enabled),
            ("claude", state.clients.claude.enabled),
            ("pi", state.clients.pi.enabled),
        ]),
        codex_model: state.router.codex_model.clone(),
        codex_models: codex_model_choices_for_state(&state),
        codex_provider_count: state
            .providers
            .iter()
            .filter(|p| p.status == ProviderStatus::Enabled)
            .count(),
        claude_provider_count: state
            .claude_providers
            .iter()
            .filter(|p| p.status == ProviderStatus::Enabled)
            .count(),
        today,
        usage_error,
        panel_visible: app.state::<TrayRuntime>().visible.load(Ordering::Relaxed),
    })
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct TrayClientPatch {
    client: Option<AgentClientKind>,
    enabled: Option<bool>,
    codex_model: Option<String>,
}

fn patch_client(state: &mut ManagerState, patch: TrayClientPatch) -> Result<(), String> {
    match (patch.client, patch.enabled, patch.codex_model) {
        (Some(client), Some(enabled), None) => {
            client_config_mut(&mut state.clients, client).enabled = enabled
        }
        (None, None, Some(model)) => {
            if !state.clients.codex.enabled {
                return Err("请先开启 Codex 接管".into());
            }
            state.router.codex_model = validate_codex_model_selection(state, &model)?;
        }
        _ => return Err("快捷面板只接受单项客户端变更".into()),
    }
    Ok(())
}

#[tauri::command]
pub(super) async fn update_tray_client(
    payload: TrayClientPatch,
    app: tauri::AppHandle,
    window: WebviewWindow,
    runtime: tauri::State<'_, RouterRuntime>,
) -> Result<TraySnapshot, String> {
    require_panel(&window)?;
    let mut state = load_state_file()?;
    patch_client(&mut state, payload)?;
    commit_client_configs(state, runtime.clone())?;
    let _ = app.emit_to(MAIN_WINDOW_LABEL, "tray-state-changed", ());
    snapshot(&app, &runtime).await
}

#[derive(Clone, Copy, Debug, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum TrayAction {
    Show,
    Hide,
    Dashboard,
    Route,
    Providers,
    Usage,
    Settings,
    Quit,
}

pub(super) fn hide_panel(app: &tauri::AppHandle) {
    app.state::<TrayRuntime>()
        .generation
        .fetch_add(1, Ordering::Relaxed);
    app.state::<TrayRuntime>()
        .visible
        .store(false, Ordering::Relaxed);
    #[cfg(target_os = "macos")]
    let _ = app.run_on_main_thread(|| {
        let _ = tray_surface::macos::hide();
    });
    #[cfg(not(target_os = "macos"))]
    if let Some(window) = app.get_webview_window(PANEL_LABEL) {
        let _ = window.hide();
    }
    let _ = app.emit_to(PANEL_LABEL, "tray-visibility", false);
}

fn navigate(app: &tauri::AppHandle, screen: &str) {
    if let Ok(mut pending) = app.state::<TrayRuntime>().navigation.lock() {
        *pending = Some(screen.to_string());
    }
    hide_panel(app);
    show_main_window(app);
    let _ = app.emit_to(MAIN_WINDOW_LABEL, "tray-navigate", ());
}

#[tauri::command]
pub(super) fn take_tray_navigation(
    app: tauri::AppHandle,
    window: WebviewWindow,
) -> Result<Option<String>, String> {
    if window.label() != MAIN_WINDOW_LABEL {
        return Err("此操作仅用于主窗口".into());
    }
    app.state::<TrayRuntime>()
        .navigation
        .lock()
        .map(|mut pending| pending.take())
        .map_err(|_| "导航暂不可用".into())
}

#[tauri::command]
pub(super) fn tray_action(
    action: TrayAction,
    app: tauri::AppHandle,
    window: WebviewWindow,
) -> Result<(), String> {
    require_panel(&window)?;
    match action {
        TrayAction::Show => show_panel(&app, None)?,
        TrayAction::Hide => hide_panel(&app),
        TrayAction::Quit => app.exit(0),
        TrayAction::Dashboard => navigate(&app, "dashboard"),
        TrayAction::Route => navigate(&app, "route"),
        TrayAction::Providers => navigate(&app, "providers"),
        TrayAction::Usage => navigate(&app, "usage"),
        TrayAction::Settings => navigate(&app, "settings"),
    }
    Ok(())
}

#[derive(Clone, Copy, Debug)]
#[cfg(any(not(target_os = "macos"), test))]
struct Bounds {
    x: f64,
    y: f64,
    width: f64,
    height: f64,
}
#[cfg(any(not(target_os = "macos"), test))]
impl Bounds {
    fn right(self) -> f64 {
        self.x + self.width
    }
    fn bottom(self) -> f64 {
        self.y + self.height
    }
}

#[cfg(any(not(target_os = "macos"), test))]
fn placement(work: Bounds, anchor: Option<Bounds>, scale: f64) -> Bounds {
    let gap = (8.0 * scale)
        .min(work.width / 4.0)
        .min(work.height / 4.0)
        .min(((work.width - 1.0) / 2.0).max(0.0))
        .min(((work.height - 1.0) / 2.0).max(0.0));
    let width = (PANEL_WIDTH * scale).min(work.width - 2.0 * gap).max(1.0);
    let height = (PANEL_HEIGHT * scale).min(work.height - 2.0 * gap).max(1.0);
    let (x, y) = match anchor {
        Some(a) if a.bottom() <= work.y + 32.0 * scale => {
            (a.x + a.width / 2.0 - width / 2.0, a.bottom() + gap)
        }
        Some(a) if a.y >= work.bottom() - 32.0 * scale => {
            (a.x + a.width / 2.0 - width / 2.0, a.y - height - gap)
        }
        Some(a) if a.right() <= work.x + 32.0 * scale => {
            (a.right() + gap, a.y + a.height / 2.0 - height / 2.0)
        }
        Some(a) if a.x >= work.right() - 32.0 * scale => {
            (a.x - width - gap, a.y + a.height / 2.0 - height / 2.0)
        }
        _ => (work.right() - width - gap, work.y + gap),
    };
    Bounds {
        x: x.clamp(work.x + gap, (work.right() - gap - width).max(work.x + gap)),
        y: y.clamp(
            work.y + gap,
            (work.bottom() - gap - height).max(work.y + gap),
        ),
        width,
        height,
    }
}

#[cfg(not(target_os = "macos"))]
fn show_panel(
    app: &tauri::AppHandle,
    click: Option<(tauri::Rect, PhysicalPosition<f64>)>,
) -> Result<(), String> {
    let window = app
        .get_webview_window(PANEL_LABEL)
        .ok_or("快捷面板暂不可用")?;
    let rect = click.as_ref().map(|(rect, _)| *rect).or_else(|| {
        app.tray_by_id(TRAY_ID)
            .and_then(|tray| tray.rect().ok().flatten())
    });
    let baseline = window
        .current_monitor()
        .ok()
        .flatten()
        .or_else(|| window.primary_monitor().ok().flatten());
    let anchor_point = click.map(|(_, point)| point).or_else(|| {
        rect.map(|rect| {
            let scale = baseline
                .as_ref()
                .map_or(1.0, |monitor| monitor.scale_factor());
            let position = rect.position.to_physical::<f64>(scale);
            let size = rect.size.to_physical::<f64>(scale);
            PhysicalPosition::new(
                position.x + size.width / 2.0,
                position.y + size.height / 2.0,
            )
        })
    });
    let monitor = anchor_point
        .and_then(|point| window.monitor_from_point(point.x, point.y).ok().flatten())
        .or(baseline);
    if let Some(monitor) = monitor {
        let scale = monitor.scale_factor();
        let work = monitor.work_area();
        let anchor = rect.map(|rect| {
            let p = rect.position.to_physical::<f64>(scale);
            let s = rect.size.to_physical::<f64>(scale);
            Bounds {
                x: p.x,
                y: p.y,
                width: s.width,
                height: s.height,
            }
        });
        let position = placement(
            Bounds {
                x: work.position.x as f64,
                y: work.position.y as f64,
                width: work.size.width as f64,
                height: work.size.height as f64,
            },
            anchor,
            scale,
        );
        // Wayland can refuse absolute placement; the compositor still chooses a usable position.
        let _ = window.set_position(PhysicalPosition::new(
            position.x.round() as i32,
            position.y.round() as i32,
        ));
        let _ = window.set_size(PhysicalSize::new(
            position.width.round() as u32,
            position.height.round() as u32,
        ));
    }
    app.state::<TrayRuntime>()
        .generation
        .fetch_add(1, Ordering::Relaxed);
    window.show().map_err(|e| e.to_string())?;
    let _ = window.set_focus();
    if window.is_focused().unwrap_or(false) {
        on_focus(app);
    }
    app.state::<TrayRuntime>()
        .visible
        .store(true, Ordering::Relaxed);
    let _ = app.emit_to(PANEL_LABEL, "tray-visibility", true);
    Ok(())
}

#[cfg(target_os = "macos")]
fn show_panel(
    app: &tauri::AppHandle,
    click: Option<(tauri::Rect, PhysicalPosition<f64>)>,
) -> Result<(), String> {
    present_macos_panel(app, click.is_some(), false)
}

#[cfg(target_os = "macos")]
fn present_macos_panel(
    app: &tauri::AppHandle,
    from_click: bool,
    toggle: bool,
) -> Result<(), String> {
    let tray = app.tray_by_id(TRAY_ID).ok_or("状态栏图标暂不可用")?;
    let handle = app.clone();
    tray.with_inner_tray_icon(move |inner| {
        let item = inner.ns_status_item().ok_or("状态栏图标暂不可用")?;
        let same_screen = tray_surface::macos::position(&item, from_click)?;
        if toggle
            && same_screen
            && handle
                .state::<TrayRuntime>()
                .visible
                .load(Ordering::Relaxed)
        {
            hide_panel(&handle);
            return Ok(());
        }
        handle
            .state::<TrayRuntime>()
            .generation
            .fetch_add(1, Ordering::Relaxed);
        tray_surface::macos::show()?;
        // Moving an already-key panel to another display need not emit a new
        // becomeKeyWindow callback. Keep blur handling on this show generation.
        if tray_surface::macos::is_focused() {
            on_focus(&handle);
        }
        handle
            .state::<TrayRuntime>()
            .visible
            .store(true, Ordering::Relaxed);
        let _ = handle.emit_to(PANEL_LABEL, "tray-visibility", true);
        Ok(())
    })
    .map_err(|error| error.to_string())?
}

pub(super) fn on_blur(app: &tauri::AppHandle) {
    let generation = app
        .state::<TrayRuntime>()
        .generation
        .load(Ordering::Relaxed);
    if app
        .state::<TrayRuntime>()
        .focused_generation
        .load(Ordering::Relaxed)
        != generation
    {
        return;
    }
    let handle = app.clone();
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(Duration::from_millis(140)).await;
        let target = handle.clone();
        let _ = handle.run_on_main_thread(move || {
            if target
                .state::<TrayRuntime>()
                .generation
                .load(Ordering::Relaxed)
                != generation
            {
                return;
            }
            #[cfg(target_os = "macos")]
            let focused = tray_surface::macos::is_focused();
            #[cfg(not(target_os = "macos"))]
            let focused = target
                .get_webview_window(PANEL_LABEL)
                .is_some_and(|window| window.is_focused().unwrap_or(false));
            if target
                .state::<TrayRuntime>()
                .visible
                .load(Ordering::Relaxed)
                && !focused
            {
                hide_panel(&target);
            }
        });
    });
}

pub(super) fn on_focus(app: &tauri::AppHandle) {
    let runtime = app.state::<TrayRuntime>();
    runtime.focused_generation.store(
        runtime.generation.load(Ordering::Relaxed),
        Ordering::Relaxed,
    );
}

pub(super) fn setup(app: &tauri::AppHandle) -> tauri::Result<()> {
    let handle = app.clone();
    tray_surface::create_host(
        app,
        WebviewUrl::App("index.html?view=tray".into()),
        move |focused| {
            if focused {
                on_focus(&handle);
            } else {
                on_blur(&handle);
            }
        },
    )?;
    let builder = tray_surface::tray_icon_builder(TRAY_ID).tooltip("XXSwitch · 打开快捷面板");
    #[cfg(target_os = "linux")]
    let builder = {
        let quick = MenuItem::with_id(app, "quick", "打开快捷面板", true, None::<&str>)?;
        let show = MenuItem::with_id(app, "show", "打开 XXSwitch", true, None::<&str>)?;
        let route = MenuItem::with_id(app, "route", "路由设置…", true, None::<&str>)?;
        let providers = MenuItem::with_id(app, "providers", "供应商…", true, None::<&str>)?;
        let usage = MenuItem::with_id(app, "usage", "使用统计…", true, None::<&str>)?;
        let settings = MenuItem::with_id(app, "settings", "设置…", true, None::<&str>)?;
        let quit = MenuItem::with_id(app, "quit", "退出 XXSwitch", true, Some("CmdOrCtrl+Q"))?;
        let separator = PredefinedMenuItem::separator(app)?;
        let separator2 = PredefinedMenuItem::separator(app)?;
        let menu = Menu::with_items(
            app,
            &[
                &quick,
                &show,
                &separator,
                &route,
                &providers,
                &usage,
                &settings,
                &separator2,
                &quit,
            ],
        )?;
        builder
            .menu(&menu)
            .on_menu_event(|app, event| match event.id().as_ref() {
                "quick" => {
                    if let Err(e) = show_panel(app, None) {
                        eprintln!("快捷面板打开失败：{e}");
                    }
                }
                "show" => navigate(app, "dashboard"),
                "route" | "providers" | "usage" | "settings" => navigate(app, event.id().as_ref()),
                "quit" => app.exit(0),
                _ => {}
            })
    };
    let tray = builder
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button,
                button_state,
                rect: _rect,
                position: _position,
                ..
            } = event
            {
                if !tray_surface::interaction::opens_panel(button, button_state) {
                    return;
                }
                let app = tray.app_handle();
                #[cfg(target_os = "macos")]
                if let Err(error) = present_macos_panel(app, true, true) {
                    eprintln!("快捷面板打开失败：{error}");
                }
                #[cfg(not(target_os = "macos"))]
                if app.state::<TrayRuntime>().visible.load(Ordering::Relaxed) {
                    hide_panel(app);
                } else if let Err(e) = show_panel(app, Some((_rect, _position))) {
                    eprintln!("快捷面板打开失败：{e}");
                }
            }
        })
        .build(app)?;
    #[cfg(any(target_os = "macos", target_os = "windows"))]
    tray_surface::direct_panel_clicks(&tray)?;
    match preferences() {
        Ok(preferences) => tray.set_visible(preferences.visible)?,
        Err(e) => eprintln!("{e}"),
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bounds(x: f64, y: f64, width: f64, height: f64) -> Bounds {
        Bounds {
            x,
            y,
            width,
            height,
        }
    }

    #[test]
    fn panel_placement_follows_each_taskbar_edge() {
        let work = bounds(0.0, 24.0, 1440.0, 876.0);
        let top = placement(work, Some(bounds(1100.0, 0.0, 24.0, 24.0)), 1.0);
        assert_eq!(top.y, 32.0);
        assert_eq!(top.x + top.width / 2.0, 1112.0);
        let bottom = placement(work, Some(bounds(1100.0, 900.0, 24.0, 40.0)), 1.0);
        assert_eq!(bottom.bottom(), 892.0);
        let left = placement(work, Some(bounds(-40.0, 400.0, 40.0, 24.0)), 1.0);
        assert_eq!(left.x, 8.0);
        let right = placement(work, Some(bounds(1440.0, 400.0, 40.0, 24.0)), 1.0);
        assert_eq!(right.right(), 1432.0);
    }

    #[test]
    fn panel_remains_inside_negative_origin_scaled_and_small_work_areas() {
        for scale in [1.0, 1.25, 1.5, 2.0] {
            for work in [
                bounds(-2560.0, -1440.0, 2560.0, 1392.0),
                bounds(1920.0, 24.0, 1920.0, 1056.0),
                bounds(0.0, 0.0, 320.0, 240.0),
                bounds(0.0, 0.0, 1.0, 1.0),
            ] {
                for anchor in [
                    None,
                    Some(bounds(work.x - 40.0, work.y - 24.0, 24.0, 24.0)),
                    Some(bounds(work.right(), work.bottom(), 24.0, 24.0)),
                ] {
                    let panel = placement(work, anchor, scale);
                    assert!(panel.x >= work.x && panel.y >= work.y);
                    assert!(panel.right() <= work.right() && panel.bottom() <= work.bottom());
                    assert!(panel.width > 0.0 && panel.height > 0.0);
                    assert!(
                        panel.width <= PANEL_WIDTH * scale && panel.height <= PANEL_HEIGHT * scale
                    );
                }
            }
        }
        let retina = placement(bounds(0.0, 48.0, 2880.0, 1752.0), None, 2.0);
        assert_eq!((retina.width, retina.height), (840.0, 1140.0));
    }

    #[test]
    fn generated_macos_template_is_black_with_transparent_padding() {
        let image = include_bytes!("../icons/tray/template@2x.rgba");
        assert_eq!(image.len(), 36 * 36 * 4);
        assert!(image.chunks_exact(4).all(|pixel| pixel[..3] == [0, 0, 0]));
        assert!(image.chunks_exact(4).any(|pixel| pixel[3] == 255));
        assert!(image
            .chunks_exact(4)
            .any(|pixel| pixel[3] > 0 && pixel[3] < 255));
        for index in 0..36 {
            for (x, y) in [(index, 0), (index, 35), (0, index), (35, index)] {
                assert_eq!(image[(y * 36 + x) * 4 + 3], 0);
            }
        }
        let colored = include_bytes!("../icons/tray/color@2x.rgba");
        assert_eq!(colored.len(), 64 * 64 * 4);
        assert!(colored
            .chunks_exact(4)
            .any(|pixel| pixel[1] > pixel[0] && pixel[3] == 255));
    }

    #[test]
    fn tray_patches_preserve_unrelated_clients_and_router_settings() {
        let mut state = default_state();
        state.clients.codex.enabled = true;
        state.clients.pi.enabled = true;
        state.clients.claude.managed_skill_location = "untouched-location".into();
        state.router.codex_model = "retained-model".into();
        state.router.port = 12345;
        state.router.debug_mode = true;
        let before = serde_json::to_value(&state).unwrap();
        patch_client(
            &mut state,
            TrayClientPatch {
                client: Some(AgentClientKind::Claude),
                enabled: Some(true),
                codex_model: None,
            },
        )
        .unwrap();
        let mut expected = before;
        expected["clients"]["claude"]["enabled"] = json!(true);
        assert_eq!(serde_json::to_value(&state).unwrap(), expected);
        for invalid in [
            json!({}),
            json!({"client":"codex"}),
            json!({"client":"pi","enabled":false,"codex_model":"other"}),
        ] {
            let before = serde_json::to_value(&state).unwrap();
            assert!(patch_client(&mut state, serde_json::from_value(invalid).unwrap()).is_err());
            assert_eq!(serde_json::to_value(&state).unwrap(), before);
        }
        assert!(serde_json::from_value::<TrayClientPatch>(
            json!({"client":"codex","enabled":true,"port":9000})
        )
        .is_err());
    }

    #[test]
    fn model_switch_requires_codex_takeover_and_a_configured_route() {
        let mut state = default_state();
        state.providers.push(serde_json::from_value(json!({
            "id":"tray-test", "name":"Tray test", "enabled":true,
            "config":{"model_providers":{"custom":{"base_url":"https://example.com/v1","experimental_bearer_token":"synthetic-test-token"}}},
            "allowed_models":["tray-model-a","tray-model-b"]
        })).unwrap());
        let patch = || TrayClientPatch {
            client: None,
            enabled: None,
            codex_model: Some("TRAY-MODEL-B".into()),
        };
        assert!(patch_client(&mut state, patch()).is_err());
        state.clients.codex.enabled = true;
        patch_client(&mut state, patch()).unwrap();
        assert_eq!(state.router.codex_model, "tray-model-b");
        assert!(patch_client(
            &mut state,
            TrayClientPatch {
                client: None,
                enabled: None,
                codex_model: Some("unknown-model".into())
            }
        )
        .is_err());
        assert_eq!(state.router.codex_model, "tray-model-b");
    }
}
