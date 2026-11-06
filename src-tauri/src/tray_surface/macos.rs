//! A real non-activating NSPanel. Tauri retains a hidden IPC/webview owner;
//! only its content view is presented, without activating the main application.
use super::geometry::{below_menu, menu_anchor, Rect, Screen};
use objc2::{define_class, msg_send, rc::Retained, DefinedClass, MainThreadOnly};
use objc2_app_kit::{
    NSAutoresizingMaskOptions, NSBackingStoreType, NSColor, NSEvent, NSFloatingWindowLevel,
    NSPanel, NSScreen, NSStatusItem, NSView, NSWindow, NSWindowCollectionBehavior as Behavior,
    NSWindowStyleMask,
};
use objc2_foundation::{ns_string, MainThreadMarker, NSPoint, NSRect, NSSize};
use std::cell::RefCell;

pub struct PanelIvars {
    focus_changed: Box<dyn Fn(bool)>,
}

define_class!(
    // SAFETY: NSPanel supports subclassing. All instances and callbacks stay on
    // AppKit's main thread; signatures match the public superclass API.
    #[unsafe(super = NSPanel)]
    #[name = "XXSwitchStatusPanel"]
    #[thread_kind = MainThreadOnly]
    #[ivars = PanelIvars]
    pub struct StatusPanel;

    impl StatusPanel {
        #[unsafe(method(canBecomeKeyWindow))]
        fn can_become_key(&self) -> bool { true }

        #[unsafe(method(canBecomeMainWindow))]
        fn can_become_main(&self) -> bool { false }

        #[unsafe(method(becomeKeyWindow))]
        fn become_key(&self) {
            unsafe { let _: () = msg_send![super(self), becomeKeyWindow]; }
            (self.ivars().focus_changed)(true);
        }

        #[unsafe(method(resignKeyWindow))]
        fn resign_key(&self) {
            unsafe { let _: () = msg_send![super(self), resignKeyWindow]; }
            (self.ivars().focus_changed)(false);
        }
    }
);

thread_local! {
    static PANEL: RefCell<Option<Retained<StatusPanel>>> = const { RefCell::new(None) };
    static LAST_SCREEN: RefCell<Option<Rect>> = const { RefCell::new(None) };
}

fn io_error(message: &str) -> tauri::Error {
    std::io::Error::other(message).into()
}

pub fn attach(
    window: &tauri::WebviewWindow,
    focus_changed: impl Fn(bool) + 'static,
) -> tauri::Result<()> {
    let mtm = MainThreadMarker::new().ok_or_else(|| io_error("状态栏面板必须在主线程创建"))?;
    let pointer = window.ns_window()?;
    if pointer.is_null() {
        return Err(io_error("状态栏 WebView 容器不可用"));
    }
    // Borrow only while Tauri owns the hidden host. Do not change its class or delegate.
    let host = unsafe { &*pointer.cast::<NSWindow>() };
    let content = host
        .contentView()
        .ok_or_else(|| io_error("状态栏 WebView 不可用"))?;
    let allocated = StatusPanel::alloc(mtm).set_ivars(PanelIvars {
        focus_changed: Box::new(focus_changed),
    });
    let panel: Retained<StatusPanel> = unsafe {
        msg_send![super(allocated), initWithContentRect: NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(super::WIDTH, super::HEIGHT)),
            styleMask: NSWindowStyleMask::Borderless | NSWindowStyleMask::NonactivatingPanel,
            backing: NSBackingStoreType::Buffered, defer: false]
    };
    unsafe {
        panel.setReleasedWhenClosed(false);
    }
    panel.setTitle(ns_string!("XXSwitch 快捷面板"));
    panel.setFloatingPanel(true);
    panel.setBecomesKeyOnlyIfNeeded(false);
    panel.setHidesOnDeactivate(false);
    panel.setMovable(false);
    panel.setExcludedFromWindowsMenu(true);
    panel.setCollectionBehavior(
        Behavior::CanJoinAllSpaces
            | Behavior::FullScreenAuxiliary
            | Behavior::Transient
            | Behavior::IgnoresCycle,
    );
    panel.setLevel(NSFloatingWindowLevel);
    panel.setOpaque(false);
    panel.setBackgroundColor(Some(&NSColor::clearColor()));
    panel.setHasShadow(true);
    // The content retains its WKWebView and IPC controller. AppKit resizes in
    // points on the destination display; no DPI conversion is necessary.
    host.setContentView(None);
    panel.setContentView(Some(&content));
    content.setAutoresizingMask(
        NSAutoresizingMaskOptions::ViewWidthSizable | NSAutoresizingMaskOptions::ViewHeightSizable,
    );
    content.setWantsLayer(true);
    if let Some(layer) = content.layer() {
        layer.setCornerRadius(12.0);
        layer.setMasksToBounds(true);
    }
    PANEL.with(|state| *state.borrow_mut() = Some(panel));
    window.with_webview(|webview| {
        // Tauri guarantees that this closure runs on the native UI thread and
        // that its WKWebView pointer remains valid for the closure's duration.
        let view = unsafe { &*webview.inner().cast::<NSView>() };
        let _ = with_panel(|panel| panel.setInitialFirstResponder(Some(view)));
    })?;
    Ok(())
}

pub fn with_panel<T>(callback: impl FnOnce(&StatusPanel) -> T) -> Result<T, String> {
    if MainThreadMarker::new().is_none() {
        return Err("状态栏操作必须在主线程执行".into());
    }
    PANEL.with(|state| {
        let panel = state.borrow().clone().ok_or("状态栏面板尚未初始化")?;
        Ok(callback(&panel))
    })
}

fn rect(value: NSRect) -> Rect {
    Rect {
        x: value.origin.x,
        y: value.origin.y,
        width: value.size.width,
        height: value.size.height,
    }
}

/// NSStatusBarButton and NSScreen supply one logical coordinate system, even
/// with mirrored menu bars, negative origins, and mixed Retina DPI.
pub fn position(item: &NSStatusItem, from_click: bool) -> Result<bool, String> {
    let mtm = MainThreadMarker::new().ok_or("状态栏操作必须在主线程执行")?;
    let button = item.button(mtm).ok_or("状态栏图标不可见")?;
    // tray-icon clears the left button highlight on release, but leaves the
    // right button highlighted. Neither click opens a native menu here.
    button.highlight(false);
    let window = button.window().ok_or("状态栏图标未附着显示器")?;
    let anchor = rect(window.convertRectToScreen(button.convertRect_toView(button.bounds(), None)));
    let native_screens = NSScreen::screens(mtm);
    let screens: Vec<_> = native_screens
        .iter()
        .map(|screen| Screen {
            frame: rect(screen.frame()),
            work: rect(screen.visibleFrame()),
        })
        .collect();
    let click = from_click.then(|| {
        let point = NSEvent::mouseLocation();
        (point.x, point.y)
    });
    let (index, anchor) = menu_anchor(&screens, anchor, click).ok_or("无法确定状态栏所在显示器")?;
    let frame = below_menu(screens[index].work, anchor, (super::WIDTH, super::HEIGHT));
    with_panel(|panel| {
        panel.setFrame_display(
            NSRect::new(
                NSPoint::new(frame.x, frame.y),
                NSSize::new(frame.width, frame.height),
            ),
            false,
        )
    })?;
    Ok(LAST_SCREEN
        .with(|last| last.replace(Some(screens[index].frame)) == Some(screens[index].frame)))
}

pub fn show() -> Result<(), String> {
    with_panel(|panel| {
        panel.orderFrontRegardless();
        panel.makeKeyWindow();
        if let Some(view) = panel.initialFirstResponder() {
            panel.makeFirstResponder(Some(&view));
        }
    })
}

pub fn hide() -> Result<(), String> {
    with_panel(|panel| panel.orderOut(None))
}
pub fn is_focused() -> bool {
    with_panel(|panel| panel.isKeyWindow()).unwrap_or(false)
}
