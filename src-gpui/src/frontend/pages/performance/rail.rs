//! The device list's hidden devices: what a hidden device is remembered by,
//! which devices the list shows, who takes the selection when a row leaves,
//! and how rows collapse and return while the others glide to their places.
use gpui_kit::{Pixels, px};
use serde_json::Value;
use std::collections::{HashMap, HashSet};
use std::time::{Duration, Instant};

/// The saved keys of the hidden devices (`stable_key`).
pub(in crate::frontend) const HIDDEN: &str = "perfHiddenDevices";
/// Hides or shows one device: `{"key": <stable key>, "hidden": bool}`.
pub(in crate::frontend) const HIDE: &str = "perfDeviceHidden";
/// Shows every device again.
pub(in crate::frontend) const RESTORE: &str = "perfDevicesRestore";

/// How long a row takes to collapse or return, and the mark to slide: the
/// interface's slower motion, on the curve for things moving between places.
const MOVE: Duration = Duration::from_millis(250);

fn field<'a>(info: &'a Value, name: &str) -> &'a str {
    info[name].as_str().unwrap_or_default()
}

/// What a hidden device is remembered by. Not its place in the list, its
/// drive number or its adapter LUID: those change when it is plugged in
/// again, when another device comes before it or when Windows restarts.
pub(super) fn stable_key(kind: &str, info: &Value) -> String {
    match kind {
        "disk" => format!(
            "disk:{}|{}|{}",
            field(info, "bus"),
            field(info, "model"),
            info["size"].as_u64().unwrap_or(0)
        ),
        // The connection name tells two ports of one adapter model apart.
        "network" => format!("nic:{}|{}", field(info, "description"), field(info, "name")),
        "gpu" => format!("gpu:{}", field(info, "name")),
        "battery" => format!("bat:{}|{}", field(info, "name"), field(info, "serial")),
        other => other.to_owned(),
    }
}

/// Devices alike in everything `stable_key` reads (two drives of one model)
/// are told apart by their order: "…", "…#1", "…#2".
pub(super) fn distinct(keys: Vec<String>) -> Vec<String> {
    let mut seen: HashMap<String, usize> = HashMap::new();
    keys.into_iter()
        .map(|key| {
            let count = seen.entry(key.clone()).or_insert(0);
            *count += 1;
            if *count == 1 {
                key
            } else {
                format!("{key}#{}", *count - 1)
            }
        })
        .collect()
}

/// Which of the devices (by stable key, in list order) the list shows: those
/// not hidden, and the first one should the saved choice hide every device
/// present now (the one left was unplugged).
pub(super) fn shown(keys: &[&str], hidden: &HashSet<String>) -> Vec<bool> {
    let mut shown = keys
        .iter()
        .map(|key| !hidden.contains(*key))
        .collect::<Vec<_>>();
    if !shown.contains(&true)
        && let Some(first) = shown.first_mut()
    {
        *first = true;
    }
    shown
}

/// The row selected once row `leaving` is hidden: the next shown one, or the
/// one before it when it was the last.
pub(super) fn successor(shown: &[bool], leaving: usize) -> Option<usize> {
    let after = (leaving + 1..shown.len()).find(|index| shown[*index]);
    after.or_else(|| {
        (0..leaving.min(shown.len()))
            .rev()
            .find(|index| shown[*index])
    })
}

/// The hidden set with `key` hidden or shown again. Nothing changes when
/// that would hide the last device the list shows.
pub(super) fn with_hidden(
    keys: &[&str],
    hidden: &HashSet<String>,
    key: &str,
    hide: bool,
) -> Option<HashSet<String>> {
    let mut next = hidden.clone();
    let changed = if hide {
        let shown = shown(keys, hidden);
        let visible = shown.iter().filter(|shown| **shown).count();
        let present = keys
            .iter()
            .zip(&shown)
            .any(|(present, shown)| *present == key && *shown);
        if present && visible <= 1 {
            return None;
        }
        next.insert(key.to_owned())
    } else {
        next.remove(key)
    };
    changed.then_some(next)
}

/// A value easing from one level to another.
#[derive(Clone, Copy)]
struct Glide {
    from: f32,
    to: f32,
    started: Instant,
    duration: Duration,
}

impl Glide {
    fn settled(at: f32, now: Instant) -> Self {
        Self {
            from: at,
            to: at,
            started: now,
            duration: Duration::ZERO,
        }
    }
    fn progress(&self, now: Instant) -> f32 {
        if self.duration.is_zero() {
            return 1.;
        }
        (now.saturating_duration_since(self.started).as_secs_f32() / self.duration.as_secs_f32())
            .min(1.)
    }
    fn at(&self, now: Instant) -> f32 {
        let progress = self.progress(now);
        // The curve is solved numerically: its ends are taken exactly.
        if progress <= 0. {
            return self.from;
        }
        if progress >= 1. {
            return self.to;
        }
        self.from + (self.to - self.from) * crate::frontend::theme::ease_move(progress)
    }
    fn running(&self, now: Instant) -> bool {
        self.progress(now) < 1.
    }
}

/// The selection mark sliding to another row: how far from that row it
/// started, and when.
struct Mark {
    selected: String,
    from: Pixels,
    started: Option<Instant>,
    /// Where it was last drawn.
    top: Pixels,
}

/// The device list's motion. A row's presence runs from 0 (hidden) to 1: it
/// takes that much of its height and of the gap before it (`layout`), so the
/// rows below move as it changes. The mark keeps to the selected row wherever
/// that row is drawn, and slides when another row is selected.
#[derive(Default)]
pub(super) struct RailMotion {
    rows: HashMap<String, Glide>,
    mark: Option<Mark>,
}

impl RailMotion {
    /// Every device's presence at `now`, by id and whether the list shows
    /// it. A row whose state changed begins to move; a device seen for the
    /// first time (a new one, or the page just opened) takes its place at
    /// once. Without `animate` everything does.
    pub(super) fn presence(
        &mut self,
        devices: &[(&str, bool)],
        animate: bool,
        now: Instant,
    ) -> Vec<f32> {
        self.rows
            .retain(|id, _| devices.iter().any(|(device, _)| device == id));
        devices
            .iter()
            .map(|(id, shown)| {
                let target = if *shown { 1. } else { 0. };
                let glide = self
                    .rows
                    .entry((*id).to_owned())
                    .or_insert_with(|| Glide::settled(target, now));
                if !animate {
                    *glide = Glide::settled(target, now);
                } else if glide.to != target {
                    // From where it is drawn now, in the share of the time
                    // its remaining way takes: a row called back half way
                    // returns in half the time.
                    let from = glide.at(now);
                    *glide = Glide {
                        from,
                        to: target,
                        started: now,
                        duration: MOVE.mul_f32((target - from).abs()),
                    };
                }
                glide.at(now)
            })
            .collect()
    }

    /// The mark's top at `now`, for the selected row drawn at `row_top`.
    pub(super) fn mark_top(
        &mut self,
        selected: &str,
        row_top: Pixels,
        animate: bool,
        now: Instant,
    ) -> Pixels {
        let mark = self.mark.get_or_insert_with(|| Mark {
            selected: selected.to_owned(),
            from: px(0.),
            started: None,
            top: row_top,
        });
        if mark.selected != selected {
            mark.selected = selected.to_owned();
            mark.from = mark.top - row_top;
            mark.started = Some(now);
        }
        let progress = mark.started.filter(|_| animate).map_or(1., |started| {
            (now.saturating_duration_since(started).as_secs_f32() / MOVE.as_secs_f32()).min(1.)
        });
        if progress >= 1. {
            mark.started = None;
            mark.from = px(0.);
        }
        mark.top = row_top + mark.from * (1. - crate::frontend::theme::ease_move(progress));
        mark.top
    }

    /// Whether anything is still on its way at `now`.
    pub(super) fn moving(&self, now: Instant) -> bool {
        self.rows.values().any(|glide| glide.running(now))
            || self
                .mark
                .as_ref()
                .is_some_and(|mark| mark.started.is_some())
    }
}

/// Where a row is drawn in the list.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct Place {
    /// The space kept above it.
    pub(super) gap: Pixels,
    pub(super) top: Pixels,
    /// The height it is clipped to.
    pub(super) height: Pixels,
}

/// The rows' places for their heights and presences. Rows are `gap` apart.
/// A row on its way takes its share of its height and of the gap above it,
/// and the gap closes too while what was above a row leaves: the list starts
/// at its first row and ends at its last one all along.
pub(super) fn layout(heights: &[Pixels], presence: &[f32], gap: Pixels) -> Vec<Place> {
    let mut above = 0f32;
    let mut end = px(0.);
    heights
        .iter()
        .zip(presence)
        .map(|(height, presence)| {
            let presence = presence.clamp(0., 1.);
            let lead = gap * (presence * above.min(1.));
            let place = Place {
                gap: lead,
                top: end + lead,
                height: *height * presence,
            };
            end = place.top + place.height;
            above += presence;
            place
        })
        .collect()
}

/// How opaque a row is drawn at `presence`: gone before its slot has closed,
/// and back only once there is room for it.
pub(super) fn opacity(presence: f32) -> f32 {
    ((presence - 0.35) / 0.65).clamp(0., 1.)
}

#[cfg(test)]
mod tests {
    use super::{
        MOVE, Place, RailMotion, distinct, layout, opacity, shown, stable_key, successor,
        with_hidden,
    };
    use gpui_kit::px;
    use serde_json::json;
    use std::collections::HashSet;
    use std::time::Instant;

    fn set(keys: &[&str]) -> HashSet<String> {
        keys.iter().map(|key| (*key).to_owned()).collect()
    }

    #[test]
    fn a_device_is_remembered_by_what_survives_replugging_and_restarts() {
        let disk = |index: u64| {
            json!({"index": index, "model": "Portable SSD", "size": 1000, "bus": "usb",
                "removable": true, "volumes": ["E:"]})
        };
        // The drive number and its letters follow the order of plugging in.
        assert_eq!(stable_key("disk", &disk(2)), stable_key("disk", &disk(5)));
        assert_eq!(stable_key("disk", &disk(2)), "disk:usb|Portable SSD|1000");
        let nic = |luid: &str, name: &str| json!({"id": luid, "name": name, "description": "Tunnel Adapter"});
        // A virtual adapter gets a new LUID each time it is created.
        assert_eq!(
            stable_key("network", &nic("1688849860263936", "Tunnel")),
            stable_key("network", &nic("1688849877041152", "Tunnel"))
        );
        // Two ports of one adapter model stay apart.
        assert_ne!(
            stable_key("network", &nic("1", "Ethernet")),
            stable_key("network", &nic("2", "Ethernet 2"))
        );
        let gpu = |luid: &str| json!({"id": luid, "name": "Graphics Adapter"});
        assert_eq!(
            stable_key("gpu", &gpu("71234")),
            stable_key("gpu", &gpu("98765"))
        );
        assert_eq!(stable_key("cpu", &json!(null)), "cpu");
        assert_eq!(stable_key("memory", &json!(null)), "memory");
    }

    #[test]
    fn devices_alike_are_told_apart_by_their_order() {
        let keys = distinct(vec!["a".into(), "b".into(), "a".into(), "a".into()]);
        assert_eq!(keys, ["a", "b", "a#1", "a#2"]);
    }

    #[test]
    fn the_list_never_ends_up_empty() {
        let keys = ["cpu", "memory", "disk:a"];
        assert_eq!(shown(&keys, &set(&["memory"])), [true, false, true]);
        // The only device left shown was unplugged: the first one returns.
        assert_eq!(shown(&keys[..2], &set(&["cpu", "memory"])), [true, false]);
        // Hiding the last shown device is refused; an absent one may be forgotten.
        assert!(with_hidden(&keys, &set(&["cpu", "memory"]), "disk:a", true).is_none());
        assert_eq!(
            with_hidden(&keys, &set(&["memory"]), "disk:a", true),
            Some(set(&["memory", "disk:a"]))
        );
        assert_eq!(
            with_hidden(&keys, &set(&["memory"]), "memory", false),
            Some(set(&[]))
        );
        // Nothing to do: already as asked.
        assert!(with_hidden(&keys, &set(&["memory"]), "memory", true).is_none());
        assert!(with_hidden(&keys, &set(&[]), "memory", false).is_none());
    }

    #[test]
    fn the_selection_goes_to_the_next_row_or_the_one_before_the_last() {
        assert_eq!(successor(&[true, true, true], 1), Some(2));
        assert_eq!(successor(&[true, true, false], 1), Some(0));
        assert_eq!(successor(&[true, false, false, true], 0), Some(3));
        assert_eq!(successor(&[true, false, true], 2), Some(0));
        assert_eq!(successor(&[false, true, false], 1), None);
    }

    #[test]
    fn a_hidden_row_collapses_and_a_restored_one_returns() {
        let start = Instant::now();
        let mut rail = RailMotion::default();
        // First seen: in place at once, shown or hidden.
        assert_eq!(
            rail.presence(&[("cpu", true), ("disk", false)], true, start),
            [1., 0.]
        );
        assert!(!rail.moving(start));
        assert_eq!(
            rail.presence(&[("cpu", false), ("disk", true)], true, start),
            [1., 0.]
        );
        assert!(rail.moving(start));
        let half = rail.presence(&[("cpu", false), ("disk", true)], true, start + MOVE / 2);
        assert!(half[0] > 0. && half[0] < 1. && half[1] > 0. && half[1] < 1.);
        assert!((half[0] + half[1] - 1.).abs() < 1e-5);
        // Called back half way: it returns from where it is, in less time.
        let back = rail.presence(&[("cpu", true), ("disk", true)], true, start + MOVE / 2);
        assert!((back[0] - half[0]).abs() < 1e-5);
        let end = start + MOVE / 2 + MOVE.mul_f32(1. - half[0]);
        assert_eq!(
            rail.presence(&[("cpu", true), ("disk", true)], true, end),
            [1., 1.]
        );
        assert!(!rail.moving(end));
        // Motion turned off: at once.
        assert_eq!(
            rail.presence(&[("cpu", false), ("disk", true)], false, end),
            [0., 1.]
        );
        assert!(!rail.moving(end));
        // A device that left is forgotten: back, it is in place at once.
        rail.presence(&[("disk", true)], true, end);
        assert_eq!(
            rail.presence(&[("cpu", true), ("disk", true)], true, end),
            [1., 1.]
        );
    }

    #[test]
    fn the_mark_stays_put_while_the_next_row_moves_into_a_hidden_selection() {
        let start = Instant::now();
        let (height, gap) = (px(58.), px(4.));
        let mut rail = RailMotion::default();
        rail.presence(&[("a", true), ("b", true)], true, start);
        assert_eq!(rail.mark_top("a", px(0.), true, start), px(0.));
        // "a" is hidden while selected: "b" takes the selection and moves up.
        for step in 0..=10 {
            let now = start + MOVE.mul_f32(step as f32 / 10.);
            let presence = rail.presence(&[("a", false), ("b", true)], true, now);
            let row_top = layout(&[height, height], &presence, gap)[1].top;
            let top = rail.mark_top("b", row_top, true, now);
            assert!(f32::from(top).abs() < 0.01, "step {step}: {top:?}");
        }
        assert!(!rail.moving(start + MOVE));
    }

    #[test]
    fn the_mark_slides_to_another_row_and_follows_rows_closing_above_it() {
        let start = Instant::now();
        let mut rail = RailMotion::default();
        assert_eq!(rail.mark_top("a", px(0.), true, start), px(0.));
        assert_eq!(rail.mark_top("c", px(124.), true, start), px(0.));
        let middle = rail.mark_top("c", px(124.), true, start + MOVE / 2);
        assert!(middle > px(0.) && middle < px(124.));
        assert_eq!(rail.mark_top("c", px(124.), true, start + MOVE), px(124.));
        assert!(!rail.moving(start + MOVE));
        // A row above closes: the mark is wherever the selected row is.
        assert_eq!(rail.mark_top("c", px(93.), true, start + MOVE * 2), px(93.));
        // Motion turned off: another row is marked at once.
        assert_eq!(rail.mark_top("a", px(0.), false, start + MOVE * 2), px(0.));
    }

    #[test]
    fn rows_keep_their_gaps_and_close_them_as_they_leave() {
        let (height, tall, gap) = (px(58.), px(76.), px(4.));
        let place = |gap, top, height| Place { gap, top, height };
        // At rest: as a column of rows `gap` apart.
        assert_eq!(
            layout(&[height, tall, height], &[1., 1., 1.], gap),
            [
                place(px(0.), px(0.), height),
                place(gap, px(62.), tall),
                place(gap, px(142.), height)
            ]
        );
        // Hidden rows take no room, whichever they are: the list starts at
        // its first row and ends at its last.
        assert_eq!(
            layout(&[height, tall, height], &[0., 1., 0.], gap),
            [
                place(px(0.), px(0.), px(0.)),
                place(px(0.), px(0.), tall),
                place(px(0.), px(76.), px(0.))
            ]
        );
        // Half way out, the first row has half its height, and the row below
        // half the gap above it.
        let half = layout(&[height, tall], &[0.5, 1.], gap);
        assert_eq!(half[0], place(px(0.), px(0.), px(29.)));
        assert_eq!(half[1], place(px(2.), px(31.), tall));
    }

    #[test]
    fn a_row_fades_before_its_place_closes() {
        assert_eq!(opacity(1.), 1.);
        assert_eq!(opacity(0.35), 0.);
        assert_eq!(opacity(0.), 0.);
        assert!(opacity(0.7) > 0. && opacity(0.7) < 1.);
    }
}
