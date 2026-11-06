//! Isolated native contract probe: no manager state, router, plugins or real IPC.
#[cfg(target_os = "macos")]
#[path = "support/tray_preview_fixture.rs"]
mod fixture;
#[cfg(target_os = "macos")]
#[path = "../src/tray_surface/mod.rs"]
#[allow(dead_code)]
mod tray_surface;

#[cfg(target_os = "macos")]
fn main() {
    use objc2::ClassType;
    use objc2_app_kit::{NSPanel, NSView, NSWindowStyleMask};
    use objc2_foundation::{NSObjectProtocol, NSPoint, NSRect, NSSize};
    use std::sync::atomic::Ordering;
    use tauri::{Emitter, Manager};
    fn require(condition: bool, message: &str) {
        if !condition {
            eprintln!("FAIL: {message}");
            std::process::exit(1);
        }
    }
    let preview = std::env::args().any(|arg| arg == "--preview")
        || std::env::current_exe()
            .ok()
            .and_then(|path| path.file_name().map(|name| name == "tray_surface_preview"))
            .unwrap_or(false);
    let mut context = tauri::generate_context!();
    context.config_mut().app.windows.clear();
    context.config_mut().identifier = "com.local.xxswitch.tray-surface-probe".into();
    // Never attach this isolated fixture to the normal desktop development URL.
    context.config_mut().build.dev_url = None;
    tauri::Builder::default()
        .manage(fixture::Fixture::default())
        .invoke_handler(tauri::generate_handler![
            fixture::load_tray_snapshot,
            fixture::update_tray_client,
            fixture::tray_action
        ])
        .setup(move |app| {
            app.set_activation_policy(tauri::ActivationPolicy::Accessory);
            let handle = app.handle().clone();
            let url = if preview {
                tauri::WebviewUrl::App("index.html?view=tray".into())
            } else {
                tauri::WebviewUrl::External("about:blank".parse().unwrap())
            };
            let host = tray_surface::create_host(app.handle(), url, move |focused| {
                if focused || !preview {
                    return;
                }
                let app = handle.clone();
                tauri::async_runtime::spawn(async move {
                    tokio::time::sleep(std::time::Duration::from_millis(140)).await;
                    let target = app.clone();
                    let _ = app.run_on_main_thread(move || {
                        if !tray_surface::macos::is_focused() {
                            let _ = tray_surface::macos::hide();
                            target
                                .state::<fixture::Fixture>()
                                .visible
                                .store(false, Ordering::Relaxed);
                            let _ = target.emit_to(tray_surface::LABEL, "tray-visibility", false);
                        }
                    });
                });
            })?;
            tray_surface::macos::with_panel(|native| {
                require(
                    native.isKindOfClass(NSPanel::class()),
                    "tray presentation must be an NSPanel, not an ordinary system window",
                );
                require(
                    native
                        .styleMask()
                        .contains(NSWindowStyleMask::NonactivatingPanel),
                    "tray presentation must not activate the app or switch Spaces",
                );
                require(
                    native.canBecomeKeyWindow(),
                    "model selection must accept keyboard input",
                );
                require(
                    !native.canBecomeMainWindow(),
                    "tray presentation must not become the main window",
                );
                require(!native.isVisible(), "native probe must stay hidden");
                require(
                    native.initialFirstResponder().is_some(),
                    "keyboard responder must move with the WebView",
                );
            })
            .unwrap();
            host.with_webview(|webview| {
                let view = unsafe { &*webview.inner().cast::<NSView>() };
                let owner = view
                    .window()
                    .expect("webview must have a presentation owner");
                require(
                    owner.isKindOfClass(NSPanel::class()),
                    "the actual WKWebView must belong to the panel",
                );
            })?;
            require(
                !host.is_visible()?,
                "the ordinary IPC owner must remain hidden",
            );
            // Verify that reparenting preserves actual WKWebView resizing, not
            // only the panel's geometry, before trying an attached small display.
            for (width, height) in [(320.0, 240.0), (tray_surface::WIDTH, tray_surface::HEIGHT)] {
                tray_surface::macos::with_panel(|native| {
                    native.setFrame_display(
                        NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(width, height)),
                        false,
                    )
                })
                .unwrap();
                host.with_webview(move |webview| {
                    let view = unsafe { &*webview.inner().cast::<NSView>() };
                    let size = view.frame().size;
                    require(
                        (size.width - width).abs() < 1.0 && (size.height - height).abs() < 1.0,
                        "the actual WKWebView must resize with the compact panel",
                    );
                })?;
            }
            println!(
                "PASS: native tray is a non-activating, keyboard-capable panel; host stays hidden"
            );
            if preview {
                fixture::install_tray(app.handle())?;
            } else {
                let tray = tray_surface::tray_icon_builder("contract-probe").build(app.handle())?;
                tray_surface::direct_panel_clicks(&tray)?;
                tray.with_inner_tray_icon(|inner| {
                    let item = inner.ns_status_item().expect("probe status item");
                    require(
                        item.menu(objc2_foundation::MainThreadMarker::new().unwrap())
                            .is_none(),
                        "left and right click must not expose a system menu",
                    );
                })?;
                println!("PASS: production status icon has no native context menu");
                std::process::exit(0);
            }
            Ok(())
        })
        .build(context)
        .expect("build isolated tray probe")
        .run(|_, _| {});
}

#[cfg(not(target_os = "macos"))]
fn main() {}
