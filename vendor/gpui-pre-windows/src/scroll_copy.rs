//! Copy what a list scrolled from the last frame instead of drawing it again.
//!
//! A wheel glide moves a list by whole device pixels each frame
//! (`ScrollRegion`). The pixels it shows were already drawn one frame
//! earlier, one move away: a frame copies them from a copy of the last frame
//! and draws only the rows that came into view and whatever else changed.
//! `Snapshot::scrolled` matches every primitive with the last frame's to find
//! that; the result equals a complete paint of the scene.
use super::*;

/// How long after a list last scrolled the last frame is still kept.
const KEEP_AFTER_SCROLL: std::time::Duration = std::time::Duration::from_secs(1);
/// More separate rectangles than this are joined: each is a scissored pass.
const MAX_PASSES: usize = 8;

/// The last frame drawn while lists scroll.
pub(super) struct ScrollCopy {
    canvas: ID3D11Texture2D,
    /// `Frames::drawn` of the frame `canvas` holds.
    frame: u64,
    /// The primitives of that frame, all its batches.
    ink: scene_cache::Snapshot,
    /// Whether it was drawn on an opaque window background.
    opaque: bool,
    /// When a list last scrolled.
    active: std::time::Instant,
}

/// What the scroll containers of a scene did since they were last seen.
#[derive(Default)]
pub(super) struct ScrollNote {
    /// The largest container that moved by whole device pixels, inside
    /// whole pixels of its clip, and the move.
    candidate: Option<(RECT, (i32, i32))>,
    /// Whether any container scrolled.
    moved: bool,
}

fn area(rect: &RECT) -> u64 {
    (rect.right - rect.left).max(0) as u64 * (rect.bottom - rect.top).max(0) as u64
}

fn union(a: &RECT, b: &RECT) -> RECT {
    RECT {
        left: a.left.min(b.left),
        top: a.top.min(b.top),
        right: a.right.max(b.right),
        bottom: a.bottom.max(b.bottom),
    }
}

/// Adds `rect` to `rects`, joined with each one whose bounds together cover
/// little more than the two: cells of a row, a rectangle and one inside it.
/// Rows of a list span the same columns, and a strip down its edge touches
/// them all: joining those (`merge_damage` would) draws the whole list.
fn add_joined(rects: &mut Vec<RECT>, mut rect: RECT) {
    let mut index = 0;
    while index < rects.len() {
        let other = rects[index];
        let joined = union(&rect, &other);
        if area(&joined) * 8 <= (area(&rect) + area(&other)) * 9 {
            rects.swap_remove(index);
            rect = joined;
            index = 0;
        } else {
            index += 1;
        }
    }
    rects.push(rect);
}

/// Joins `rects` until at most `limit` remain, each time the two whose
/// bounds add the least area.
fn join(rects: &mut Vec<RECT>, limit: usize) {
    while rects.len() > limit {
        let mut best = (0, 1, u64::MAX);
        for i in 0..rects.len() {
            for j in i + 1..rects.len() {
                let cost = area(&union(&rects[i], &rects[j]))
                    .saturating_sub(area(&rects[i]) + area(&rects[j]));
                if cost < best.2 {
                    best = (i, j, cost);
                }
            }
        }
        let joined = union(&rects[best.0], &rects[best.1]);
        rects.swap_remove(best.1);
        rects.swap_remove(best.0);
        add_joined(rects, joined);
    }
}

impl DirectXRenderer {
    /// Compares the scroll containers of `scene` with those last seen.
    pub(super) fn note_scroll(&mut self, scene: &Scene) -> ScrollNote {
        let mut note = ScrollNote::default();
        let mut best = 0;
        for region in &scene.scroll_regions {
            let Some(old) = self.scroll_seen.insert(region.id, *region) else {
                // Already moved where it first shows, as a page slides in:
                // the next frame moves it again.
                note.moved |= region.offset != Point::default();
                continue;
            };
            if old.offset == region.offset {
                continue;
            }
            note.moved = true;
            let dx = region.offset.x.0 - old.offset.x.0;
            let dy = region.offset.y.0 - old.offset.y.0;
            let by = (dx.round() as i32, dy.round() as i32);
            if old.clip != region.clip
                || (dx - by.0 as f32).abs() > 1e-3
                || (dy - by.1 as f32).abs() > 1e-3
            {
                continue;
            }
            // The pixels whose centers the clip holds, wholly.
            let c = region.clip;
            let clip = RECT {
                left: (c.left().0.ceil() as i32).max(0),
                top: (c.top().0.ceil() as i32).max(0),
                right: (c.right().0.floor() as i32).min(self.width as i32),
                bottom: (c.bottom().0.floor() as i32).min(self.height as i32),
            };
            if by.0.abs() < clip.right - clip.left
                && by.1.abs() < clip.bottom - clip.top
                && area(&clip) > best
            {
                best = area(&clip);
                note.candidate = Some((clip, by));
            }
        }
        if self.scroll_seen.len() > 256 {
            let present: std::collections::HashSet<usize> =
                scene.scroll_regions.iter().map(|r| r.id).collect();
            self.scroll_seen.retain(|id, _| present.contains(id));
        }
        note
    }

    /// Draws `scene` from the kept copy of the last frame when a list only
    /// scrolled: false, having drawn nothing, when that is not possible or
    /// would not draw much less than the usual way.
    pub(super) fn render_scrolled(
        &mut self,
        scene: &Scene,
        batches: &[PrimitiveBatch],
        background_appearance: WindowBackgroundAppearance,
        note: &ScrollNote,
        report: &mut FrameReport,
    ) -> Result<bool> {
        static ENABLED: OnceLock<bool> = OnceLock::new();
        if !*ENABLED
            .get_or_init(|| std::env::var_os("ARGUS_GPUI_DISABLE_SCROLL_COPY").is_none())
        {
            return Ok(false);
        }
        let Some((clip, by)) = note.candidate else {
            return Ok(false);
        };
        let (width, height) = (self.width, self.height);
        let opaque = matches!(background_appearance, WindowBackgroundAppearance::Opaque);
        let r = self.resources.as_mut().context("resources missing")?;
        if r.scroll_copy
            .as_ref()
            .is_none_or(|c| c.frame != r.frames.drawn || c.opaque != opaque)
        {
            return Ok(false);
        }
        let Some(now) = r.snapshot(scene, batches) else {
            return Ok(false);
        };
        let copy = r.scroll_copy.as_ref().context("scroll copy")?;
        let Some(plan) = now.scrolled(&copy.ink, clip, by, width, height, &self.fresh) else {
            r.recycle(Some(now));
            return Ok(false);
        };
        report.step("scrollMatch");
        let mut damage = Vec::new();
        for rect in plan.damage {
            add_joined(&mut damage, rect);
        }
        // Raster area costs more than passes: eight beat three and sixteen
        // on the software rasterizer.
        join(&mut damage, MAX_PASSES);
        let drawn: u64 = damage.iter().map(area).sum();
        // The usual way draws the whole clip at least.
        if drawn * 4 > area(&clip) * 3 {
            r.recycle(Some(now));
            return Ok(false);
        }
        let clear_color = if opaque { [1.0f32; 4] } else { [0.0f32; 4] };
        r.compose_prefix = false;
        let stale = r.frames.stale();
        let copied = plan.copied;
        self.pre_draw(&clear_color, Base::Keep)?;
        {
            let devices = self.devices.as_ref().context("devices missing")?;
            let r = self.resources.as_ref().context("resources missing")?;
            let target = r.render_target.as_ref().context("render target missing")?;
            let canvas = &r.scroll_copy.as_ref().context("scroll copy")?.canvas;
            let ctx = &devices.device_context;
            let copy_box = |to: &ID3D11Texture2D, at: (i32, i32), from: &ID3D11Texture2D, rect: RECT| unsafe {
                ctx.CopySubresourceRegion(
                    to,
                    0,
                    at.0 as u32,
                    at.1 as u32,
                    0,
                    from,
                    0,
                    Some(&D3D11_BOX {
                        left: rect.left as u32,
                        top: rect.top as u32,
                        front: 0,
                        right: rect.right as u32,
                        bottom: rect.bottom as u32,
                        back: 1,
                    }),
                );
            };
            // The back buffer as the last frame left it, but where the moved
            // pixels go next.
            let stale = match &stale {
                Damage::Full => vec![RECT {
                    left: 0,
                    top: 0,
                    right: width as i32,
                    bottom: height as i32,
                }],
                Damage::Rects(rects) => rects.clone(),
            };
            let mut restored = 0;
            for rect in stale {
                for piece in subtract_rect(rect, copied) {
                    copy_box(target, (piece.left, piece.top), canvas, piece);
                    restored += area(&piece);
                }
            }
            copy_box(
                target,
                (copied.left, copied.top),
                canvas,
                RECT {
                    left: copied.left - by.0,
                    top: copied.top - by.1,
                    right: copied.right - by.0,
                    bottom: copied.bottom - by.1,
                },
            );
            self.work.copies += 2;
            self.work.copy_pixels += restored + area(&copied);
        }
        report.step("scrollCopy");
        if !damage.is_empty() {
            report.local = self.draw_region(scene, batches, &damage, &clear_color, report)?;
        }
        // What changed from the last frame, now also in the kept copy.
        let mut change = Damage::NONE;
        change.add(copied);
        for rect in &damage {
            change.add(*rect);
        }
        {
            let devices = self.devices.as_ref().context("devices missing")?;
            let r = self.resources.as_ref().context("resources missing")?;
            let target = r.render_target.as_ref().context("render target missing")?;
            let canvas = &r.scroll_copy.as_ref().context("scroll copy")?.canvas;
            if let Damage::Rects(rects) = &change {
                for rect in rects {
                    unsafe {
                        devices.device_context.CopySubresourceRegion(
                            canvas,
                            0,
                            rect.left as u32,
                            rect.top as u32,
                            0,
                            target,
                            0,
                            Some(&D3D11_BOX {
                                left: rect.left as u32,
                                top: rect.top as u32,
                                front: 0,
                                right: rect.right as u32,
                                bottom: rect.bottom as u32,
                                back: 1,
                            }),
                        );
                    }
                }
            }
        }
        if std::env::var_os("ARGUS_GPUI_DIAGNOSTICS").is_some() {
            println!(
                "{{\"event\":\"gpu-scroll-copy\",\"moved\":{},\"by\":[{},{}],\"copiedPixels\":{},\"drawnPixels\":{drawn},\"passes\":{}}}",
                plan.moved,
                by.0,
                by.1,
                area(&copied),
                damage.len(),
            );
        }
        report.path = "scrolled";
        report.reused = true;
        report.repaired_pixels = drawn;
        let revision = scene.paint_revision;
        let r = self.resources.as_mut().context("resources missing")?;
        let suffix = r.suffix_snapshot.take();
        r.recycle(suffix);
        r.suffix_damage = None;
        r.streak = false;
        let mut whole = r.spare.pop().unwrap_or_default();
        whole.clone_from(&now);
        let old = r.last.replace(Last::Whole {
            revision,
            opaque,
            ink: Some(whole),
        });
        if let Some(Last::Whole { ink, .. }) = old {
            r.recycle(ink);
        }
        r.frames.drew(change);
        r.present_dirty = present_regions(true, &r.frames.unpresented);
        let drawn_frame = r.frames.drawn;
        let old = r.scroll_copy.as_mut().map(|copy| {
            copy.frame = drawn_frame;
            copy.active = std::time::Instant::now();
            std::mem::replace(&mut copy.ink, now)
        });
        r.recycle(old);
        self.scene_cache_event(scene, true, false, false, 0, batches.len());
        Ok(true)
    }

    /// After a frame drawn the usual way: keeps a copy of it while lists
    /// scroll, for the next frame to copy moved pixels from.
    pub(super) fn keep_scroll_copy(
        &mut self,
        scene: &Scene,
        batches: &[PrimitiveBatch],
        background_appearance: WindowBackgroundAppearance,
        note: &ScrollNote,
        only: Option<RECT>,
    ) -> Result<()> {
        let devices = self.devices.as_ref().context("devices missing")?;
        let r = self.resources.as_mut().context("resources missing")?;
        let now = std::time::Instant::now();
        let previous = r.scroll_copy.take();
        let active = if note.moved {
            Some(now)
        } else {
            previous
                .as_ref()
                .map(|c| c.active)
                .filter(|at| now.duration_since(*at) < KEEP_AFTER_SCROLL)
        };
        // A frame shown only in part leaves other pixels of the render
        // target unknown.
        let ink = match (active, only) {
            (Some(_), None) => r.snapshot(scene, batches),
            _ => None,
        };
        let (Some(active), Some(ink)) = (active, ink) else {
            r.recycle(previous.map(|copy| copy.ink));
            return Ok(());
        };
        let canvas = match previous {
            Some(copy) => {
                r.recycle(Some(copy.ink));
                copy.canvas
            }
            None => unsafe {
                let target = r.render_target.as_ref().context("render target missing")?;
                let mut desc = D3D11_TEXTURE2D_DESC::default();
                target.GetDesc(&mut desc);
                desc.BindFlags = D3D11_BIND_SHADER_RESOURCE.0 as u32;
                desc.MiscFlags = 0;
                let mut canvas = None;
                devices
                    .device
                    .CreateTexture2D(&desc, None, Some(&mut canvas))?;
                canvas.context("scroll copy texture")?
            },
        };
        let target = r.render_target.as_ref().context("render target missing")?;
        unsafe { devices.device_context.CopyResource(&canvas, target) };
        self.work.copies += 1;
        self.work.copy_pixels += u64::from(self.width) * u64::from(self.height);
        r.scroll_copy = Some(ScrollCopy {
            canvas,
            frame: r.frames.drawn,
            ink,
            opaque: matches!(background_appearance, WindowBackgroundAppearance::Opaque),
            active,
        });
        Ok(())
    }
}
