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

/// The devices' order as arranged by dragging, by stable key (`stable_key`).
pub(in crate::frontend) const ORDER: &str = "perfDeviceOrder";
/// How the disks are ordered among themselves (`DiskOrder`); set from the
/// menu as a preference of the same name.
pub(in crate::frontend) const DISK_ORDER: &str = "perfDiskOrder";
/// Puts every device back in the collector's order.
pub(in crate::frontend) const RESET_ORDER: &str = "perfDeviceOrderReset";

/// How long a row takes to collapse or return, and the mark to slide: the
/// interface's slower motion, on the curve for things moving between places.
const MOVE: Duration = Duration::from_millis(250);

/// How the disks are ordered among themselves.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(in crate::frontend) enum DiskOrder {
    /// By disk number, as Windows numbers them (`\\.\PhysicalDriveN`, the
    /// numbers Disk Management shows): the collector's own order.
    #[default]
    Number,
    /// By drive letter, a disk with several by its first; disks without a
    /// letter follow, by number.
    Letter,
    /// Wherever they were dragged to.
    Custom,
}

impl DiskOrder {
    pub(in crate::frontend) const ALL: [Self; 3] = [Self::Number, Self::Letter, Self::Custom];

    pub(in crate::frontend) fn parse(value: &str) -> Self {
        match value {
            "letter" => Self::Letter,
            "custom" => Self::Custom,
            _ => Self::Number,
        }
    }

    pub(in crate::frontend) fn key(self) -> &'static str {
        match self {
            Self::Number => "number",
            Self::Letter => "letter",
            Self::Custom => "custom",
        }
    }
}

/// A device as the list orders it: its stable key and, for a disk, its
/// number and first drive letter.
pub(super) struct Entry<'a> {
    pub(super) key: &'a str,
    pub(super) disk: Option<(u64, Option<&'a str>)>,
}

/// The disks among `entries` (by index) in the order `disks` puts them;
/// `None` for an order of their own (`DiskOrder::Custom`).
fn ranked_disks(entries: &[Entry], disks: DiskOrder) -> Option<Vec<usize>> {
    let mut ranked = entries
        .iter()
        .enumerate()
        .filter_map(|(index, entry)| entry.disk.map(|disk| (index, disk)))
        .collect::<Vec<_>>();
    match disks {
        DiskOrder::Custom => return None,
        DiskOrder::Number => ranked.sort_by_key(|(_, (number, _))| *number),
        DiskOrder::Letter => ranked.sort_by(|(_, (a, first)), (_, (b, second))| {
            // Lettered disks first, by letter; then the others, by number.
            (first.is_none(), first.map(str::to_ascii_uppercase), a).cmp(&(
                second.is_none(),
                second.map(str::to_ascii_uppercase),
                b,
            ))
        }),
    }
    Some(ranked.into_iter().map(|(index, _)| index).collect())
}

/// The list's order, as indices into `entries` (the collector's order):
/// as `saved` arranges them, with the disks in the places the disks take
/// there put in the order `disks` asks for. A device not in `saved` (new, or
/// none arranged yet) follows the device it follows in the collector's order.
pub(super) fn arrange(entries: &[Entry], saved: &[String], disks: DiskOrder) -> Vec<usize> {
    let mut order = Vec::with_capacity(entries.len());
    for key in saved {
        if let Some(index) = entries.iter().position(|entry| entry.key == key)
            && !order.contains(&index)
        {
            order.push(index);
        }
    }
    for index in 0..entries.len() {
        if order.contains(&index) {
            continue;
        }
        let after = (0..index)
            .rev()
            .find_map(|before| order.iter().position(|placed| *placed == before));
        let at = match after {
            Some(position) => position + 1,
            // Nothing before it is placed: before the first that comes after.
            None => (index + 1..entries.len())
                .find_map(|next| order.iter().position(|placed| *placed == next))
                .unwrap_or(order.len()),
        };
        order.insert(at, index);
    }
    if let Some(ranked) = ranked_disks(entries, disks) {
        let mut ranked = ranked.into_iter();
        for slot in order.iter_mut() {
            if entries[*slot].disk.is_some()
                && let Some(disk) = ranked.next()
            {
                *slot = disk;
            }
        }
    }
    order
}

/// The order to save (`ORDER`) once the devices present now are arranged as
/// `present` (stable keys): the saved keys of devices absent now (unplugged,
/// or not found this start) keep their places after the device they followed
/// there, so that they come back to them (`arrange`). Those that followed no
/// present device stay in front.
pub(super) fn merged_order(saved: &[String], present: &[String]) -> Vec<String> {
    let mut leading = Vec::new();
    let mut following: HashMap<&str, Vec<&String>> = HashMap::new();
    let mut anchor: Option<&str> = None;
    for key in saved {
        if present.contains(key) {
            anchor = Some(key);
        } else if let Some(anchor) = anchor {
            following.entry(anchor).or_default().push(key);
        } else {
            leading.push(key);
        }
    }
    let mut order = Vec::with_capacity(present.len() + saved.len());
    for key in leading.into_iter().chain(present.iter().flat_map(|key| {
        std::iter::once(key).chain(following.remove(key.as_str()).unwrap_or_default())
    })) {
        if !order.contains(key) {
            order.push(key.clone());
        }
    }
    order
}

/// Whether `order` (indices into `entries`) keeps the disks in the order
/// `disks` asks for: a drag that does not keeps them where it put them.
pub(super) fn keeps(entries: &[Entry], order: &[usize], disks: DiskOrder) -> bool {
    let Some(ranked) = ranked_disks(entries, disks) else {
        return true;
    };
    order
        .iter()
        .copied()
        .filter(|index| entries[*index].disk.is_some())
        .eq(ranked)
}

/// Where the dragged row belongs in `order` (row ids, top to bottom) once
/// its middle is at `middle`: it trades places with the next shown row
/// whose middle it has passed, one at a time, either way. `middles` gives
/// each shown row's middle as laid out in `order`; hidden rows have none
/// and keep their places among the others.
pub(super) fn reordered(
    order: &[String],
    dragged: &str,
    middle: f32,
    middles: &HashMap<String, f32>,
) -> Option<Vec<String>> {
    let at = order.iter().position(|id| id == dragged)?;
    let shown = |index: &usize| middles.contains_key(&order[*index]);
    if let Some(next) = (at + 1..order.len()).find(shown)
        && middle > middles[&order[next]]
    {
        let mut moved = order.to_vec();
        let id = moved.remove(at);
        moved.insert(next, id);
        return Some(moved);
    }
    if let Some(previous) = (0..at).rev().find(shown)
        && middle < middles[&order[previous]]
    {
        let mut moved = order.to_vec();
        let id = moved.remove(at);
        moved.insert(previous, id);
        return Some(moved);
    }
    None
}

/// A device row being dragged to another place.
#[derive(Clone)]
pub(super) struct RowDrag {
    pub(super) id: String,
    /// The pointer's y when it took the row, and now.
    pub(super) grab: f32,
    pub(super) pointer: f32,
    /// Where the row was laid out when taken, from the top of the list.
    pub(super) start: f32,
    /// The rows' order as the drag has arranged them so far, by id, and
    /// when it took the row.
    pub(super) order: Vec<String>,
    pub(super) initial: Vec<String>,
    /// Where the row was last drawn: within the list, wherever the pointer is.
    pub(super) drawn: Option<f32>,
}

impl RowDrag {
    /// Where the pointer has taken the row: its start moved as far as the
    /// pointer has. It decides the row's place, even past either end.
    pub(super) fn at(&self) -> f32 {
        self.start + self.pointer - self.grab
    }
}

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
    /// A row being dragged to another place.
    pub(super) drag: Option<RowDrag>,
    /// Rows gliding to new places after the order changed.
    flips: crate::frontend::flip::Flips,
    /// The order the rows were last drawn in, by id.
    order: Vec<String>,
    /// Each row's height and presence as last drawn, by id: where the rows
    /// are in another order (`tops`).
    sizes: HashMap<String, (Pixels, f32)>,
}

impl RailMotion {
    /// Where each row is laid out in `order` (ids, top to bottom), `gap`
    /// apart, at the heights and presences last drawn: its top and height.
    pub(super) fn tops(&self, order: &[String], gap: Pixels) -> HashMap<String, (f32, f32)> {
        let known = order
            .iter()
            .filter(|id| self.sizes.contains_key(*id))
            .collect::<Vec<_>>();
        let (heights, presence): (Vec<_>, Vec<_>) =
            known.iter().map(|id| self.sizes[id.as_str()]).unzip();
        known
            .into_iter()
            .zip(layout(&heights, &presence, gap))
            .map(|(id, place)| (id.clone(), (f32::from(place.top), f32::from(place.height))))
            .collect()
    }

    /// Notes the rows' order and sizes as drawn now. Rows the order moved
    /// glide from where they were, except the one under the pointer.
    pub(super) fn arranged(
        &mut self,
        order: Vec<String>,
        sizes: HashMap<String, (Pixels, f32)>,
        gap: Pixels,
        animate: bool,
        now: Instant,
    ) {
        let moved = !self.order.is_empty() && self.order != order;
        let before = if moved && animate {
            self.tops(&self.order, gap)
        } else {
            HashMap::new()
        };
        self.sizes = sizes;
        if !before.is_empty() {
            let after = self.tops(&order, gap);
            let dragged = self.drag.as_ref().map(|drag| drag.id.as_str());
            for (id, (top, _)) in &after {
                if Some(id.as_str()) != dragged
                    && let Some((old, _)) = before.get(id)
                    && old != top
                {
                    self.flips.moved(id, top - old, now);
                }
            }
        }
        self.order = order;
    }

    /// How far from its place row `id` is drawn while it glides there.
    pub(super) fn offset(&self, id: &str, now: Instant) -> f32 {
        self.flips.offset(id, now)
    }

    /// Takes row `id`, laid out at `top`, under the pointer at `pointer`.
    pub(super) fn take(&mut self, id: &str, top: f32, pointer: f32) {
        self.drag = Some(RowDrag {
            id: id.to_owned(),
            grab: pointer,
            pointer,
            start: top,
            order: self.order.clone(),
            initial: self.order.clone(),
            drawn: None,
        });
    }

    /// Follows the pointer at `pointer`: once the dragged row's middle
    /// passes a neighbour's, the two trade places and the one passed glides
    /// to its new place.
    pub(super) fn follow(&mut self, pointer: f32, gap: Pixels) {
        let Some(drag) = self.drag.as_mut() else {
            return;
        };
        drag.pointer = pointer;
        let id = drag.id.clone();
        for _ in 0..self.order.len().max(1) {
            let Some(drag) = self.drag.as_ref() else {
                return;
            };
            let tops = self.tops(&drag.order, gap);
            let height = tops.get(&id).map_or(0., |(_, height)| *height);
            let middles = tops
                .iter()
                .filter(|(other, (_, height))| **other != id && *height > 0.)
                .map(|(other, (top, height))| (other.clone(), top + height / 2.))
                .collect::<HashMap<_, _>>();
            let Some(order) = reordered(&drag.order, &id, drag.at() + height / 2., &middles) else {
                return;
            };
            if let Some(drag) = self.drag.as_mut() {
                drag.order = order;
            }
        }
    }

    /// Lets go of the dragged row: it glides from under the pointer into
    /// its place. Returns the drag, with the order it arranged.
    pub(super) fn release(&mut self, gap: Pixels, animate: bool, now: Instant) -> Option<RowDrag> {
        let drag = self.drag.take()?;
        if animate && let Some((top, _)) = self.tops(&drag.order, gap).get(&drag.id) {
            self.flips
                .moved(&drag.id, top - drag.drawn.unwrap_or(drag.at()), now);
        }
        Some(drag)
    }
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
            || self.flips.active(now)
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
        DiskOrder, Entry, MOVE, Place, RailMotion, arrange, distinct, keeps, layout, merged_order,
        opacity, reordered, shown, stable_key, successor, with_hidden,
    };
    use gpui_kit::px;
    use serde_json::json;
    use std::collections::{HashMap, HashSet};
    use std::time::Instant;

    fn set(keys: &[&str]) -> HashSet<String> {
        keys.iter().map(|key| (*key).to_owned()).collect()
    }

    fn strings(keys: &[&str]) -> Vec<String> {
        keys.iter().map(|key| (*key).to_owned()).collect()
    }

    #[test]
    fn an_unplugged_device_keeps_its_place_when_the_others_are_reordered() {
        // "usb" is unplugged while the others are dragged around: it stays
        // after the device it followed.
        let saved = strings(&["cpu", "memory", "usb", "disk", "gpu"]);
        assert_eq!(
            merged_order(&saved, &strings(&["cpu", "gpu", "memory", "disk"])),
            ["cpu", "gpu", "memory", "usb", "disk"]
        );
        // Absent devices with nothing present before them stay in front, in
        // their order; the others go along with the device they followed.
        let saved = strings(&["usb", "dock", "cpu", "memory", "nic"]);
        assert_eq!(
            merged_order(&saved, &strings(&["memory", "cpu"])),
            ["usb", "dock", "memory", "nic", "cpu"]
        );
        // Nothing saved yet, or every device present: the new order as it is.
        let present = strings(&["memory", "cpu", "disk"]);
        assert_eq!(merged_order(&[], &present), present);
        assert_eq!(
            merged_order(&strings(&["cpu", "disk", "memory"]), &present),
            present
        );
        // Plugged in again, it returns to that place.
        let merged = merged_order(
            &strings(&["cpu", "memory", "usb", "disk", "gpu"]),
            &strings(&["cpu", "gpu", "memory", "disk"]),
        );
        let entries = ["cpu", "memory", "disk", "usb", "gpu"].map(|key| Entry { key, disk: None });
        let keys = arrange(&entries, &merged, DiskOrder::Custom)
            .into_iter()
            .map(|index| entries[index].key)
            .collect::<Vec<_>>();
        assert_eq!(keys, ["cpu", "gpu", "memory", "usb", "disk"]);
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

    fn entries() -> Vec<Entry<'static>> {
        let disk = |key, number, letter| Entry {
            key,
            disk: Some((number, letter)),
        };
        let other = |key| Entry { key, disk: None };
        vec![
            other("cpu"),
            other("memory"),
            disk("ssd", 0, Some("D:")),
            disk("hdd", 1, Some("C:")),
            disk("card", 2, None),
            disk("usb", 3, Some("E:")),
            other("nic"),
        ]
    }

    fn keys(entries: &[Entry<'static>], order: &[usize]) -> Vec<&'static str> {
        order.iter().map(|index| entries[*index].key).collect()
    }

    #[test]
    fn disks_go_by_number_or_by_their_first_letter() {
        let entries = entries();
        let none: &[String] = &[];
        assert_eq!(
            keys(&entries, &arrange(&entries, none, DiskOrder::Number)),
            ["cpu", "memory", "ssd", "hdd", "card", "usb", "nic"]
        );
        // Lettered disks by letter, then the others by number.
        assert_eq!(
            keys(&entries, &arrange(&entries, none, DiskOrder::Letter)),
            ["cpu", "memory", "hdd", "ssd", "usb", "card", "nic"]
        );
        assert_eq!(DiskOrder::parse("letter"), DiskOrder::Letter);
        assert_eq!(DiskOrder::parse("unknown"), DiskOrder::Number);
        for order in DiskOrder::ALL {
            assert_eq!(DiskOrder::parse(order.key()), order);
        }
    }

    #[test]
    fn an_arranged_order_keeps_new_devices_beside_their_neighbours() {
        let entries = entries();
        let saved = ["nic", "cpu", "usb", "memory", "hdd", "ssd"]
            .map(String::from)
            .to_vec();
        // Custom: as dragged; the card, never arranged, follows the hdd it
        // follows in the collector's order.
        assert_eq!(
            keys(&entries, &arrange(&entries, &saved, DiskOrder::Custom)),
            ["nic", "cpu", "usb", "memory", "hdd", "card", "ssd"]
        );
        // By number: the disks keep the places they took, in number order.
        assert_eq!(
            keys(&entries, &arrange(&entries, &saved, DiskOrder::Number)),
            ["nic", "cpu", "ssd", "memory", "hdd", "card", "usb"]
        );
        // Unknown and repeated keys are passed over; nothing placed before
        // a new first device puts it before the first placed after it.
        let saved = ["gone", "memory", "memory"].map(String::from).to_vec();
        assert_eq!(
            keys(&entries, &arrange(&entries, &saved, DiskOrder::Custom)),
            ["cpu", "memory", "ssd", "hdd", "card", "usb", "nic"]
        );
    }

    #[test]
    fn a_drag_that_breaks_the_disk_order_makes_it_custom() {
        let entries = entries();
        // The network adapter dragged above the CPU: the disks are as they were.
        assert!(keeps(&entries, &[6, 0, 1, 2, 3, 4, 5], DiskOrder::Number));
        // Disk 1 dragged above disk 0.
        assert!(!keeps(&entries, &[0, 1, 3, 2, 4, 5, 6], DiskOrder::Number));
        assert!(keeps(&entries, &[0, 1, 3, 2, 5, 4, 6], DiskOrder::Letter));
        assert!(keeps(&entries, &[3, 2, 1, 0, 4, 5, 6], DiskOrder::Custom));
    }

    #[test]
    fn a_dragged_row_trades_places_past_the_middle_of_the_next_shown_one() {
        let order = ["a", "b", "hidden", "c"].map(String::from).to_vec();
        let middles = [("a", 29.), ("b", 91.), ("c", 153.)]
            .map(|(id, middle)| (id.to_owned(), middle))
            .into_iter()
            .collect();
        assert_eq!(reordered(&order, "a", 80., &middles), None);
        assert_eq!(
            reordered(&order, "a", 95., &middles).unwrap(),
            ["b", "a", "hidden", "c"]
        );
        // Past a hidden row, to after the next shown one.
        let order = ["b", "a", "hidden", "c"].map(String::from).to_vec();
        assert_eq!(
            reordered(&order, "a", 160., &middles).unwrap(),
            ["b", "hidden", "c", "a"]
        );
        assert_eq!(
            reordered(&order, "a", 20., &middles).unwrap(),
            ["a", "b", "hidden", "c"]
        );
    }

    #[test]
    fn a_dragged_row_moves_live_and_settles_into_its_place() {
        let start = Instant::now();
        let gap = px(4.);
        let mut rail = RailMotion::default();
        let sizes = |ids: &[&str]| {
            ids.iter()
                .map(|id| ((*id).to_owned(), (px(58.), 1.)))
                .collect::<HashMap<_, _>>()
        };
        let order = |ids: &[&str]| ids.iter().map(|id| (*id).to_owned()).collect::<Vec<_>>();
        rail.arranged(
            order(&["a", "b", "c"]),
            sizes(&["a", "b", "c"]),
            gap,
            true,
            start,
        );
        rail.take("a", 0., 100.);
        // Not yet past b's middle (91): nothing moves.
        rail.follow(150., gap);
        assert_eq!(rail.drag.as_ref().unwrap().order, ["a", "b", "c"]);
        // Past it: a and b trade places at once.
        rail.follow(170., gap);
        assert_eq!(rail.drag.as_ref().unwrap().order, ["b", "a", "c"]);
        // Drawn in the new order, b glides up from where it was; a does not.
        let dragged = rail.drag.as_ref().unwrap().order.clone();
        rail.arranged(dragged, sizes(&["a", "b", "c"]), gap, true, start);
        assert_eq!(rail.offset("b", start), 62.);
        assert_eq!(rail.offset("a", start), 0.);
        let drag = rail.release(gap, true, start).unwrap();
        assert_eq!(drag.order, ["b", "a", "c"]);
        // Let go 8 px below its new place (62): it glides up from there.
        assert_eq!(rail.offset("a", start), 8.);
        assert!(rail.moving(start));
        assert!(rail.drag.is_none());
    }

    #[test]
    fn a_row_fades_before_its_place_closes() {
        assert_eq!(opacity(1.), 1.);
        assert_eq!(opacity(0.35), 0.);
        assert_eq!(opacity(0.), 0.);
        assert!(opacity(0.7) > 0. && opacity(0.7) < 1.);
    }
}
