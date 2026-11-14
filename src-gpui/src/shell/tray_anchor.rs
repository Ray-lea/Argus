//! Where the tray's flyouts open: beside the icon, a gap away from the
//! taskbar, inside the work area of the icon's display.
use serde::{Deserialize, Serialize};

/// The tray icon and its display, in physical pixels (left, top, right,
/// bottom), and the display's scale.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub(crate) struct Anchor {
    pub icon: [i32; 4],
    pub work: [i32; 4],
    pub monitor: [i32; 4],
    pub scale: f32,
    /// The hidden icons flyout holding the icon, when it is there rather
    /// than on the taskbar.
    #[serde(default)]
    pub overflow: Option<Overflow>,
}

/// The shell's flyout of hidden tray icons: a flyout of Argus opened from
/// it sits beside it rather than over it, and leaves it active, since it
/// closes when another window takes the foreground.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub(crate) struct Overflow {
    pub rect: [i32; 4],
    /// Its window, `HWND` as a number.
    pub window: isize,
}

/// The side of the display the taskbar is on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Edge {
    Bottom,
    Top,
    Left,
    Right,
}

impl Anchor {
    /// The display edge nearest the icon: the taskbar's, wherever it is and
    /// whether or not it hides itself.
    pub fn edge(&self) -> Edge {
        let [left, top, right, bottom] = self.monitor;
        let beside = self.beside();
        let x = (beside[0] + beside[2]) / 2;
        let y = (beside[1] + beside[3]) / 2;
        [
            (bottom - y, Edge::Bottom),
            (y - top, Edge::Top),
            (x - left, Edge::Left),
            (right - x, Edge::Right),
        ]
        .into_iter()
        .min_by_key(|(distance, _)| *distance)
        .map_or(Edge::Bottom, |(_, edge)| edge)
    }

    /// What a flyout opens beside: the icon, or the hidden icons flyout
    /// holding it, which it leaves uncovered.
    fn beside(&self) -> [i32; 4] {
        self.overflow.map_or(self.icon, |overflow| overflow.rect)
    }

    /// The top-left corner of a flyout `width` × `height` pixels: `gap`
    /// pixels off the taskbar (or the hidden icons flyout, when the icon is
    /// in it), centred on the icon along the taskbar, and at least `gap`
    /// inside the work area.
    pub fn place(&self, width: i32, height: i32, gap: i32) -> (i32, i32) {
        let [left, top, right, bottom] = self.work;
        let [icon_left, icon_top, icon_right, icon_bottom] = self.beside();
        let centre_x = (icon_left + icon_right) / 2 - width / 2;
        let centre_y = (icon_top + icon_bottom) / 2 - height / 2;
        let (x, y) = match self.edge() {
            Edge::Bottom => (centre_x, bottom.min(icon_top) - gap - height),
            Edge::Top => (centre_x, top.max(icon_bottom) + gap),
            Edge::Left => (left.max(icon_right) + gap, centre_y),
            Edge::Right => (right.min(icon_left) - gap - width, centre_y),
        };
        let fit = |value: i32, low: i32, high: i32, length: i32| {
            value.min(high - gap - length).max(low + gap)
        };
        (fit(x, left, right, width), fit(y, top, bottom, height))
    }
}

#[cfg(test)]
mod tests {
    use super::{Anchor, Edge, Overflow};

    const MONITOR: [i32; 4] = [0, 0, 1920, 1080];

    fn anchor(icon: [i32; 4], work: [i32; 4]) -> Anchor {
        Anchor {
            icon,
            work,
            monitor: MONITOR,
            scale: 1.,
            overflow: None,
        }
    }

    #[test]
    fn a_flyout_sits_over_the_taskbar_beside_the_icon() {
        // Bottom taskbar, icon left of the clock.
        let a = anchor([1700, 1040, 1724, 1072], [0, 0, 1920, 1032]);
        assert_eq!(a.edge(), Edge::Bottom);
        assert_eq!(a.place(200, 300, 12), (1612, 720));
        // Too near the right edge to centre: kept 12 px inside.
        let near = anchor([1860, 1040, 1884, 1072], [0, 0, 1920, 1032]);
        assert_eq!(near.place(360, 500, 12), (1548, 520));
        // In the hidden icons flyout above the taskbar: over the flyout,
        // centred on it, leaving it uncovered.
        let a = Anchor {
            overflow: Some(Overflow {
                rect: [1600, 876, 1756, 1032],
                window: 1,
            }),
            ..anchor([1700, 980, 1724, 1004], [0, 0, 1920, 1032])
        };
        assert_eq!(a.edge(), Edge::Bottom);
        assert_eq!(a.place(200, 300, 12), (1578, 564));
    }

    #[test]
    fn every_taskbar_edge_and_a_hidden_taskbar() {
        let a = anchor([1700, 8, 1724, 40], [0, 48, 1920, 1080]);
        assert_eq!(a.edge(), Edge::Top);
        assert_eq!(a.place(200, 300, 12), (1612, 60));
        let a = anchor([8, 900, 40, 924], [48, 0, 1920, 1080]);
        assert_eq!(a.edge(), Edge::Left);
        assert_eq!(a.place(200, 300, 12), (60, 762));
        let a = anchor([1880, 900, 1912, 924], [0, 0, 1872, 1080]);
        assert_eq!(a.edge(), Edge::Right);
        assert_eq!(a.place(200, 300, 12), (1660, 762));
        // An auto-hidden taskbar leaves the whole display as work area.
        let a = anchor([1700, 1040, 1724, 1072], MONITOR);
        assert_eq!(a.edge(), Edge::Bottom);
        assert_eq!(a.place(200, 300, 12), (1612, 728));
    }

    #[test]
    fn a_flyout_larger_than_the_room_keeps_its_top_left_inside() {
        let a = anchor([1700, 1040, 1724, 1072], [0, 0, 1920, 1032]);
        assert_eq!(a.place(400, 2000, 12), (1508, 12));
    }
}
