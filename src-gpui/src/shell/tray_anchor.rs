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
        let x = (self.icon[0] + self.icon[2]) / 2;
        let y = (self.icon[1] + self.icon[3]) / 2;
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

    /// The top-left corner of a flyout `width` × `height` pixels: `gap`
    /// pixels off the taskbar (or the icon, when the icon sits above it in
    /// the overflow), centred on the icon along the taskbar, and at least
    /// `gap` inside the work area.
    pub fn place(&self, width: i32, height: i32, gap: i32) -> (i32, i32) {
        let [left, top, right, bottom] = self.work;
        let [icon_left, icon_top, icon_right, icon_bottom] = self.icon;
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
    use super::{Anchor, Edge};

    const MONITOR: [i32; 4] = [0, 0, 1920, 1080];

    fn anchor(icon: [i32; 4], work: [i32; 4]) -> Anchor {
        Anchor {
            icon,
            work,
            monitor: MONITOR,
            scale: 1.,
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
        // In the overflow above the taskbar: over the icon instead.
        let a = anchor([1700, 980, 1724, 1004], [0, 0, 1920, 1032]);
        assert_eq!(a.place(200, 300, 12), (1612, 668));
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
