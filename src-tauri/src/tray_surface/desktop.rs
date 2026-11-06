//! Windows/Linux placement in physical pixels. Work-area insets identify the
//! taskbar even when its icon is inside Windows' hidden-icons flyout.
#[derive(Clone, Copy, Debug)]
pub struct Bounds {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}
impl Bounds {
    pub fn right(self) -> f64 {
        self.x + self.width
    }
    pub fn bottom(self) -> f64 {
        self.y + self.height
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Edge {
    Top,
    Bottom,
    Left,
    Right,
}

fn nearest_edge(work: Bounds, anchor: Bounds) -> Edge {
    let center = (
        anchor.x + anchor.width / 2.0,
        anchor.y + anchor.height / 2.0,
    );
    [
        (Edge::Bottom, (work.bottom() - center.1).abs()),
        (Edge::Top, (center.1 - work.y).abs()),
        (Edge::Left, (center.0 - work.x).abs()),
        (Edge::Right, (work.right() - center.0).abs()),
    ]
    .into_iter()
    .min_by(|a, b| a.1.total_cmp(&b.1))
    .unwrap()
    .0
}

fn taskbar_edge(display: Bounds, work: Bounds, anchor: Option<Bounds>) -> Edge {
    let insets = [
        (Edge::Bottom, display.bottom() - work.bottom()),
        (Edge::Top, work.y - display.y),
        (Edge::Left, work.x - display.x),
        (Edge::Right, display.right() - work.right()),
    ];
    let candidates: Vec<_> = insets
        .into_iter()
        .filter(|(_, inset)| *inset > 0.5)
        .collect();
    if candidates.len() == 1 {
        return candidates[0].0;
    }
    if let Some(anchor) = anchor {
        let nearest = nearest_edge(work, anchor);
        if candidates.is_empty() || candidates.iter().any(|(edge, _)| *edge == nearest) {
            return nearest;
        }
    }
    candidates
        .into_iter()
        .max_by(|a, b| a.1.total_cmp(&b.1))
        .map_or(Edge::Bottom, |(edge, _)| edge)
}

fn at_edge(work: Bounds, anchor: Option<Bounds>, scale: f64, edge: Edge) -> Bounds {
    let gap = (8.0 * scale)
        .min(((work.width - 1.0) / 2.0).max(0.0))
        .min(((work.height - 1.0) / 2.0).max(0.0));
    let mut width = (super::WIDTH * scale).min(work.width - 2.0 * gap).max(1.0);
    let mut height = (super::HEIGHT * scale)
        .min(work.height - 2.0 * gap)
        .max(1.0);
    let a = anchor.unwrap_or_else(|| match edge {
        Edge::Top => Bounds {
            x: work.right(),
            y: work.y,
            width: 0.0,
            height: 0.0,
        },
        Edge::Bottom => Bounds {
            x: work.right(),
            y: work.bottom(),
            width: 0.0,
            height: 0.0,
        },
        Edge::Left => Bounds {
            x: work.x,
            y: work.bottom(),
            width: 0.0,
            height: 0.0,
        },
        Edge::Right => Bounds {
            x: work.right(),
            y: work.bottom(),
            width: 0.0,
            height: 0.0,
        },
    });
    let (x, y) = match edge {
        Edge::Top | Edge::Bottom => {
            let above = (a.y - work.y - 2.0 * gap).max(0.0);
            let below = (work.bottom() - a.bottom() - 2.0 * gap).max(0.0);
            let prefer_above = edge == Edge::Bottom;
            let use_above = if prefer_above {
                above >= height || below < height && above >= below
            } else {
                !(below >= height || above < height && below >= above)
            };
            let space = if use_above { above } else { below };
            height = height.min(space).max(1.0);
            (
                a.x + a.width / 2.0 - width / 2.0,
                if use_above {
                    a.y - height - gap
                } else {
                    a.bottom() + gap
                },
            )
        }
        Edge::Left | Edge::Right => {
            let left = (a.x - work.x - 2.0 * gap).max(0.0);
            let right = (work.right() - a.right() - 2.0 * gap).max(0.0);
            let prefer_left = edge == Edge::Right;
            let use_left = if prefer_left {
                left >= width || right < width && left >= right
            } else {
                !(right >= width || left < width && right >= left)
            };
            let space = if use_left { left } else { right };
            width = width.min(space).max(1.0);
            (
                if use_left {
                    a.x - width - gap
                } else {
                    a.right() + gap
                },
                a.y + a.height / 2.0 - height / 2.0,
            )
        }
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

pub fn placement_on_display(
    display: Bounds,
    work: Bounds,
    anchor: Option<Bounds>,
    scale: f64,
) -> Bounds {
    at_edge(work, anchor, scale, taskbar_edge(display, work, anchor))
}

#[cfg(test)]
pub fn placement(work: Bounds, anchor: Option<Bounds>, scale: f64) -> Bounds {
    at_edge(
        work,
        anchor,
        scale,
        anchor.map_or(Edge::Top, |anchor| nearest_edge(work, anchor)),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    fn b(x: f64, y: f64, width: f64, height: f64) -> Bounds {
        Bounds {
            x,
            y,
            width,
            height,
        }
    }

    #[test]
    fn overflow_flyout_follows_taskbar_edge_instead_of_nearest_screen_corner() {
        let screen = b(0.0, 0.0, 1920.0, 1080.0);
        let work = b(0.0, 0.0, 1920.0, 1040.0);
        for x in [1680.0, 1776.0, 1840.0] {
            let icon = b(x, 900.0, 24.0, 24.0);
            let panel = placement_on_display(screen, work, Some(icon), 1.0);
            assert_eq!(panel.bottom(), 892.0);
            assert_eq!(panel.width, 420.0);
        }
    }

    #[test]
    fn hidden_icons_on_all_taskbar_edges_stay_adjacent() {
        let screen = b(0.0, 0.0, 1920.0, 1080.0);
        for (work, icon, edge) in [
            (
                b(0.0, 0.0, 1920.0, 1040.0),
                b(1840.0, 900.0, 24.0, 24.0),
                Edge::Bottom,
            ),
            (
                b(0.0, 40.0, 1920.0, 1040.0),
                b(1840.0, 150.0, 24.0, 24.0),
                Edge::Top,
            ),
            (
                b(72.0, 0.0, 1848.0, 1080.0),
                b(160.0, 700.0, 24.0, 24.0),
                Edge::Left,
            ),
            (
                b(0.0, 0.0, 1848.0, 1080.0),
                b(1700.0, 700.0, 24.0, 24.0),
                Edge::Right,
            ),
        ] {
            let panel = placement_on_display(screen, work, Some(icon), 1.0);
            assert_eq!(taskbar_edge(screen, work, Some(icon)), edge);
            match edge {
                Edge::Bottom => assert_eq!(panel.bottom(), icon.y - 8.0),
                Edge::Top => assert_eq!(panel.y, icon.bottom() + 8.0),
                Edge::Left => assert_eq!(panel.x, icon.right() + 8.0),
                Edge::Right => assert_eq!(panel.right(), icon.x - 8.0),
            }
        }
    }

    #[test]
    fn negative_monitor_origins_and_mixed_dpi_keep_physical_size_inside_work_area() {
        for scale in [1.0, 1.25, 1.5, 2.0] {
            for (x, y) in [
                (-3840.0, -2160.0),
                (-2560.0, 0.0),
                (0.0, -2160.0),
                (2560.0, 120.0),
            ] {
                let screen = b(x, y, 2560.0 * scale, 1440.0 * scale);
                let work = b(x, y, screen.width, screen.height - 40.0 * scale);
                let icon = b(
                    work.right() - 60.0 * scale,
                    work.bottom() + 8.0 * scale,
                    24.0 * scale,
                    24.0 * scale,
                );
                let panel = placement_on_display(screen, work, Some(icon), scale);
                assert_eq!(panel.width, super::super::WIDTH * scale);
                assert_eq!(panel.height, super::super::HEIGHT * scale);
                assert!(panel.x >= work.x && panel.right() <= work.right());
                assert!(panel.y >= work.y && panel.bottom() <= work.bottom());
            }
        }
    }

    #[test]
    fn auto_hidden_taskbar_and_small_work_area_keep_the_panel_usable() {
        for work in [
            b(0.0, 0.0, 1920.0, 1080.0),
            b(-320.0, 0.0, 320.0, 240.0),
            b(0.0, 0.0, 1.0, 1.0),
        ] {
            for icon in [
                None,
                Some(b(work.right() - 24.0, work.bottom() - 24.0, 24.0, 24.0)),
            ] {
                let panel = placement_on_display(work, work, icon, 1.5);
                assert!(panel.x >= work.x && panel.y >= work.y);
                assert!(panel.right() <= work.right() && panel.bottom() <= work.bottom());
                assert!(panel.width > 0.0 && panel.height > 0.0);
            }
        }
    }
}
