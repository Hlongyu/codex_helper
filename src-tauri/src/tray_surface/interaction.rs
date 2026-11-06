use tauri::tray::{MouseButton, MouseButtonState};

/// Both primary and secondary clicks open the same panel, once on release.
pub fn opens_panel(button: MouseButton, state: MouseButtonState) -> bool {
    matches!(button, MouseButton::Left | MouseButton::Right) && state == MouseButtonState::Up
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn either_mouse_button_opens_panel_on_release_only() {
        for button in [MouseButton::Left, MouseButton::Right] {
            assert!(
                opens_panel(button, MouseButtonState::Up),
                "{button:?} must open the panel directly"
            );
            assert!(
                !opens_panel(button, MouseButtonState::Down),
                "a single click must not dispatch twice"
            );
        }
        assert!(!opens_panel(MouseButton::Middle, MouseButtonState::Up));
    }
}
