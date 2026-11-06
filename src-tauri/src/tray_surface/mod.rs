//! The tray webview has a separate presentation surface from the main window.
use tauri::{WebviewUrl, WebviewWindow, WebviewWindowBuilder};
pub mod interaction;

#[cfg(any(target_os = "macos", test))]
pub mod geometry;
#[cfg(target_os = "macos")]
pub mod macos;

pub const LABEL: &str = "tray";
pub const WIDTH: f64 = 420.0;
pub const HEIGHT: f64 = 570.0;

/// Production and the isolated preview use the same menu-free status icon.
/// Only Linux attaches a fallback menu because Tauri does not emit tray clicks there.
pub fn tray_icon_builder(id: &str) -> tauri::tray::TrayIconBuilder<tauri::Wry> {
    #[cfg(target_os = "macos")]
    let icon = tauri::image::Image::new_owned(
        include_bytes!("../../icons/tray/template@2x.rgba").to_vec(),
        36,
        36,
    );
    #[cfg(not(target_os = "macos"))]
    let icon = tauri::image::Image::new_owned(
        include_bytes!("../../icons/tray/color@2x.rgba").to_vec(),
        64,
        64,
    );
    tauri::tray::TrayIconBuilder::with_id(id)
        .icon(icon)
        .icon_as_template(cfg!(target_os = "macos"))
        .show_menu_on_left_click(false)
}

#[cfg(any(target_os = "macos", target_os = "windows"))]
pub fn direct_panel_clicks(tray: &tauri::tray::TrayIcon) -> tauri::Result<()> {
    tray.with_inner_tray_icon(|inner| inner.set_show_menu_on_right_click(false))
}

pub fn create_host(
    app: &tauri::AppHandle,
    url: WebviewUrl,
    focus_changed: impl Fn(bool) + 'static,
) -> tauri::Result<WebviewWindow> {
    let window = WebviewWindowBuilder::new(app, LABEL, url)
        .title("XXSwitch 快捷面板")
        .inner_size(WIDTH, HEIGHT)
        .visible(false)
        .focused(false)
        .decorations(false)
        .resizable(false)
        .maximizable(false)
        .minimizable(false)
        .always_on_top(true)
        .skip_taskbar(true)
        .visible_on_all_workspaces(true)
        .shadow(true)
        .build()?;
    #[cfg(target_os = "macos")]
    macos::attach(&window, focus_changed)?;
    #[cfg(not(target_os = "macos"))]
    let _ = focus_changed;
    Ok(window)
}
