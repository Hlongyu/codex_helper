//! AppKit screen coordinates: logical points with an upward Y axis on every display.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rect {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}
impl Rect {
    pub fn right(self) -> f64 {
        self.x + self.width
    }
    pub fn top(self) -> f64 {
        self.y + self.height
    }
    pub fn contains(self, point: (f64, f64)) -> bool {
        point.0 >= self.x && point.0 < self.right() && point.1 >= self.y && point.1 < self.top()
    }
}

#[derive(Clone, Copy, Debug)]
pub struct Screen {
    pub frame: Rect,
    pub work: Rect,
}

/// Resolve the actual clicked menu bar. Never infer it from the previous panel
/// window, the main display, or a physical point scaled by a different monitor.
pub fn menu_anchor(
    screens: &[Screen],
    button: Rect,
    click: Option<(f64, f64)>,
) -> Option<(usize, Rect)> {
    let index = click
        .and_then(|point| {
            screens
                .iter()
                .position(|screen| screen.frame.contains(point))
        })
        .or_else(|| {
            screens.iter().position(|screen| {
                screen.frame.contains((
                    button.x + button.width / 2.0,
                    button.y + button.height / 2.0,
                ))
            })
        })?;
    let screen = screens[index];
    let anchor = if screen.frame.contains((
        button.x + button.width / 2.0,
        button.y + button.height / 2.0,
    )) {
        button
    } else {
        // macOS can mirror menu bars while exposing the status button's window
        // on the other display. Anchor to this click's menu bar, not that window.
        let x = click?.0;
        let bar_bottom = screen.work.top().min(screen.frame.top() - button.height);
        Rect {
            x: x - button.width / 2.0,
            y: bar_bottom,
            width: button.width,
            height: button.height,
        }
    };
    Some((index, anchor))
}

pub fn below_menu(work: Rect, anchor: Rect, desired: (f64, f64)) -> Rect {
    let gap = 6.0_f64
        .min(((work.width - 1.0) / 2.0).max(0.0))
        .min(((work.height - 1.0) / 2.0).max(0.0));
    let width = desired.0.min(work.width - 2.0 * gap).max(1.0);
    let top = (anchor.y - gap)
        .min(work.top() - gap)
        .max(work.y + gap + 1.0);
    let height = desired.1.min(top - work.y - gap).max(1.0);
    Rect {
        x: (anchor.x + anchor.width / 2.0 - width / 2.0)
            .clamp(work.x + gap, (work.right() - gap - width).max(work.x + gap)),
        y: top - height,
        width,
        height,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn rect(x: f64, y: f64, width: f64, height: f64) -> Rect {
        Rect {
            x,
            y,
            width,
            height,
        }
    }

    #[test]
    fn click_on_mirrored_second_menu_bar_does_not_reuse_main_screen_anchor() {
        let screens = [
            Screen {
                frame: rect(0.0, 0.0, 1512.0, 982.0),
                work: rect(0.0, 60.0, 1512.0, 884.0),
            },
            Screen {
                frame: rect(-1920.0, 100.0, 1920.0, 1080.0),
                work: rect(-1920.0, 100.0, 1920.0, 1056.0),
            },
        ];
        let button = rect(1400.0, 958.0, 24.0, 24.0);
        let (index, anchor) = menu_anchor(&screens, button, Some((-180.0, 1168.0))).unwrap();
        assert_eq!(index, 1);
        let panel = below_menu(screens[index].work, anchor, (420.0, 570.0));
        assert_eq!(panel.y + panel.height, 1150.0);
        assert!(panel.x < 0.0 && panel.right() <= 0.0);
        assert_eq!(
            panel.width, 420.0,
            "AppKit points must not be multiplied by Retina scale"
        );
    }

    #[test]
    fn points_support_displays_above_below_and_on_either_side_at_mixed_dpi() {
        for (x, y, w, h) in [
            (0.0, 0.0, 1512.0, 982.0),
            (-1920.0, 120.0, 1920.0, 1080.0),
            (1512.0, 0.0, 2560.0, 1440.0),
            (0.0, -1080.0, 1920.0, 1080.0),
            (100.0, 982.0, 1920.0, 1080.0),
            (0.0, 0.0, 320.0, 240.0),
        ] {
            let screen = Screen {
                frame: rect(x, y, w, h),
                work: rect(x, y + 40.0, w, h - 64.0),
            };
            for offset in [1.0, w / 2.0, w - 1.0] {
                let button = rect(x + offset - 12.0, y + h - 24.0, 24.0, 24.0);
                let (index, anchor) =
                    menu_anchor(&[screen], button, Some((x + offset, y + h - 12.0))).unwrap();
                assert_eq!(index, 0);
                let panel = below_menu(screen.work, anchor, (420.0, 570.0));
                assert!(panel.x >= screen.work.x && panel.right() <= screen.work.right());
                assert!(panel.y >= screen.work.y && panel.top() <= anchor.y);
                assert!(panel.height > 0.0 && panel.height <= 570.0);
            }
        }
    }
}
