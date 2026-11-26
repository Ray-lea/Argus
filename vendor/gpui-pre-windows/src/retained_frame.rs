//! Draw a frame from what the render target and the cached static prefix
//! already hold: only what changed since the back buffer's frame is drawn
//! again, and the animated suffix over the prefix when it changed.
use super::*;

/// How the last frame was drawn.
pub(super) enum Last {
    /// Whole, from a scene without an animated suffix.
    Whole {
        revision: u64,
        opaque: bool,
        ink: Option<scene_cache::Snapshot>,
    },
    /// Over the cached prefix; its prefix and suffix are in the resources.
    Cached { opaque: bool },
}

/// How a frame begins in the render target.
pub(super) enum Base<'a> {
    /// Cleared to the window background.
    Clear,
    /// As the last frame left it.
    Keep,
    /// With the cached prefix copied into these regions.
    Restore(&'a Damage),
}

/// What a frame did, for the GPU timing samples.
#[derive(Default)]
pub(super) struct FrameReport {
    pub(super) path: &'static str,
    pub(super) reused: bool,
    pub(super) repaired_pixels: u64,
    pub(super) suffix: bool,
    /// The primitives a local repaint drew, out of the scene's.
    pub(super) local: Option<(usize, usize)>,
    /// CPU time at the end of each step of the frame, with GPU timing.
    pub(super) started: Option<std::time::Instant>,
    pub(super) steps: Vec<(&'static str, f64)>,
}

impl FrameReport {
    pub(super) fn step(&mut self, name: &'static str) {
        if let Some(started) = self.started {
            self.steps
                .push((name, started.elapsed().as_secs_f64() * 1_000_000.));
        }
    }
}

impl DirectXRenderer {
    /// A scene without an animated suffix. Only what differs from the frame
    /// the back buffer holds is drawn, from the primitives that reach it.
    pub(super) fn render_whole(
        &mut self,
        scene: &Scene,
        batches: &[PrimitiveBatch],
        background_appearance: WindowBackgroundAppearance,
        partial: bool,
        cache: bool,
        report: &mut FrameReport,
    ) -> Result<()> {
        report.path = "whole";
        let opaque = matches!(background_appearance, WindowBackgroundAppearance::Opaque);
        let clear_color = if opaque { [1.0f32; 4] } else { [0.0f32; 4] };
        let (width, height) = (self.width, self.height);
        let revision = scene.paint_revision;
        let r = self.resources.as_mut().context("resources missing")?;
        r.compose_prefix = false;
        let last = match r.last.take() {
            Some(Last::Whole {
                revision,
                opaque: was_opaque,
                ink,
            }) if was_opaque == opaque => Some((revision, ink)),
            _ => None,
        };
        let (change, ink) = match last {
            // A scene changes only with a paint, which gives it a new revision.
            Some((seen, ink)) if revision != 0 && seen == revision && self.fresh.is_empty() => {
                (Damage::NONE, ink)
            }
            last => {
                let ink = if cache && revision != 0 {
                    r.snapshot(scene, batches)
                } else {
                    None
                };
                let old = last.and_then(|(_, ink)| ink);
                let change = match (&ink, &old) {
                    (Some(now), Some(old)) => match now.repair(old, width, height, &self.fresh) {
                        Some(scene_cache::Repair::Unchanged) => Damage::NONE,
                        Some(scene_cache::Repair::Region(rect)) => Damage::Rects(vec![rect]),
                        None => Damage::Full,
                    },
                    _ => Damage::Full,
                };
                r.recycle(old);
                (change, ink)
            }
        };
        report.step("ink");
        let mut needed = r.frames.stale();
        needed.extend(&change);
        // Without the cache every frame is drawn whole, for comparisons.
        if !partial || !cache {
            needed = Damage::Full;
        }
        // A frame over the cached prefix that follows compares with this one
        // in full.
        r.suffix_damage = None;
        r.suffix_snapshot = None;
        r.streak = false;
        match &needed {
            Damage::Full => {
                self.pre_draw(&clear_color, Base::Clear)?;
                self.timing_mark(1);
                self.upload_scene_buffers(scene)?;
                self.prepare_batches(batches)?;
                self.timing_mark(2);
                self.draw_batches(scene, batches, 0..batches.len())?;
            }
            Damage::Rects(rects) => {
                report.reused = true;
                self.pre_draw(&clear_color, Base::Keep)?;
                self.timing_mark(1);
                if !rects.is_empty() {
                    report.repaired_pixels = needed.pixels(width, height);
                    report.local =
                        self.draw_region(scene, batches, &passes(rects), &clear_color, report)?;
                }
            }
        }
        report.step("draw");
        self.timing_mark(3);
        let r = self.resources.as_mut().context("resources missing")?;
        r.last = Some(Last::Whole {
            revision,
            opaque,
            ink,
        });
        r.frames.drew(change);
        r.present_dirty = present_regions(partial && cache, &r.frames.unpresented);
        self.scene_cache_event(scene, report.reused, false, false, 0, batches.len());
        Ok(())
    }

    /// A scene with a suffix from `animated_order` on that can change without
    /// a paint (chart curves, load bars). Its prefix stays in a texture and is
    /// repaired where it changed. The suffix is drawn over it when it changed
    /// or when its background has to be restored.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn render_cached(
        &mut self,
        scene: &Scene,
        batches: &[PrimitiveBatch],
        prefix_end: usize,
        animated_order: DrawOrder,
        background_appearance: WindowBackgroundAppearance,
        partial: bool,
        only: Option<RECT>,
        report: &mut FrameReport,
    ) -> Result<()> {
        report.path = "cached";
        let opaque = matches!(background_appearance, WindowBackgroundAppearance::Opaque);
        let clear_color = if opaque { [1.0f32; 4] } else { [0.0f32; 4] };
        let (width, height) = (self.width, self.height);
        let prefix_key = (scene.paint_revision, animated_order as usize, opaque);
        static REPAIR: OnceLock<bool> = OnceLock::new();
        let repair_enabled =
            *REPAIR.get_or_init(|| std::env::var_os("ARGUS_GPUI_DISABLE_PREFIX_REPAIR").is_none());
        static COMPOSE: OnceLock<bool> = OnceLock::new();
        let compose_enabled = *COMPOSE
            .get_or_init(|| std::env::var_os("ARGUS_GPUI_DISABLE_PREFIX_COMPOSITION").is_none());
        let r = self.resources.as_mut().context("resources missing")?;
        r.compose_prefix = false;
        // The last frame compares with this one when it was also drawn over
        // the cached prefix, on the same background.
        let continued = matches!(r.last, Some(Last::Cached { opaque: was }) if was == opaque);
        let same_prefix = r.prefix_key == Some(prefix_key);
        let snapshot = if same_prefix {
            None
        } else {
            r.snapshot(scene, &batches[..prefix_end])
        };
        // How the prefix differs from the last one drawn here. The texture
        // holds that one while `prefix_key` is set and is repaired alike.
        let locality = match (&snapshot, &r.prefix_snapshot) {
            (Some(now), Some(old)) => now.repair(old, width, height, &self.fresh),
            _ => None,
        };
        // Repairing most of the window copies more, back and forth, than
        // drawing all of it again.
        let large = matches!(locality, Some(scene_cache::Repair::Region(rect))
            if (rect.right - rect.left) as u64 * (rect.bottom - rect.top) as u64 * 2
                > u64::from(width) * u64::from(height));
        let repair = locality.filter(|_| {
            !large
                && repair_enabled
                && r.prefix.is_some()
                && r.prefix_key.is_some_and(|key| key.2 == opaque)
        });
        let repair_rect = match repair {
            Some(scene_cache::Repair::Region(rect)) => Some(rect),
            _ => None,
        };
        let reuse_prefix = same_prefix || matches!(repair, Some(scene_cache::Repair::Unchanged));
        let reuse_background = same_prefix || repair.is_some();
        report.step("prefix");
        // What this frame changes from the last one drawn.
        let mut change = if !continued {
            Damage::Full
        } else if same_prefix {
            Damage::NONE
        } else {
            match locality {
                Some(scene_cache::Repair::Unchanged) => Damage::NONE,
                Some(scene_cache::Repair::Region(rect)) => Damage::Rects(vec![rect]),
                None => Damage::Full,
            }
        };
        let suffix = &batches[prefix_end..];
        let suffix_damage = suffix_damage(scene, suffix, width, height);
        let suffix_ink = r.snapshot(scene, suffix);
        let suffix_changed = !continued
            || match (&suffix_ink, &r.suffix_snapshot) {
                (Some(now), Some(old)) => !now.matches(old, &self.fresh),
                _ => true,
            };
        report.step("suffix");
        // Outside the part of the frame that is shown, the suffix waits for a
        // frame that shows it; until then its pixels are not known.
        let suffix_needed = match (only, &suffix_damage) {
            (Some(shown), Some(rects)) => rects.iter().any(|rect| overlaps(rect, &shown)),
            _ => true,
        };
        if suffix_changed && suffix_needed {
            match (&suffix_damage, &r.suffix_damage) {
                (Some(now), Some(old)) => {
                    for rect in now.iter().chain(old) {
                        change.add(*rect);
                    }
                }
                _ => change = Damage::Full,
            }
        }
        // What the back buffer must have drawn anew: this frame's change and
        // what it missed of the frames since it was last drawn.
        let mut restore = r.frames.stale();
        restore.extend(&change);
        if !partial || !reuse_background {
            restore = Damage::Full;
        }
        let draw_suffix = suffix_needed
            && (suffix_changed
                || match &suffix_damage {
                    Some(rects) => restore.reaches(rects),
                    None => true,
                });
        // The suffix is drawn whole, over its restored background.
        let mut needed = restore.clone();
        if draw_suffix {
            match &suffix_damage {
                Some(rects) => {
                    for rect in rects {
                        needed.add(*rect);
                    }
                }
                None => needed = Damage::Full,
            }
        }
        let composed = if draw_suffix
            && reuse_background
            && !matches!(needed, Damage::Full)
            && compose_enabled
            && suffix.iter().all(|batch| match batch {
                PrimitiveBatch::PolychromeSprites { texture_id, .. } => {
                    self.atlas.get_texture_view(*texture_id).is_some()
                }
                PrimitiveBatch::Quads(_) => true,
                _ => false,
            }) {
            independent_sprite_regions(scene, suffix, width, height)
        } else {
            None
        };
        // Composed images restore their own background; only the rest of the
        // suffix and the changed regions are copied from the prefix.
        let base = match &composed {
            Some(plan) => {
                let mut regions = Damage::Rects(plan.restore.clone());
                regions.extend(&restore);
                regions
            }
            None => needed,
        };
        // Over a run of complete paints for changes that are not local, the
        // next frame repaints everything as well: keep no prefix for it. A
        // patched chart tree stays and paints over the prefix: keep it then.
        let keep_complete = (locality.is_some() && !large) || !r.streak || only.is_some();
        // Without the suffix, a frame draws what the buffer needs from the
        // prefix's own primitives, uploading only those that reach it, and
        // copies just the repaired rectangle into the cache.
        let complete = draw_suffix || !reuse_background;
        let redraw = match &restore {
            Damage::Rects(rects) if !complete => Some(passes(rects)),
            _ => None,
        };
        report.reused = reuse_background;
        report.suffix = draw_suffix;
        report.repaired_pixels = match &redraw {
            Some(_) => restore.pixels(width, height),
            None => {
                repair_rect.map_or(0, |r| (r.right - r.left) as u64 * (r.bottom - r.top) as u64)
            }
        };
        report.step("plan");
        self.pre_draw(
            &clear_color,
            match (&redraw, reuse_background) {
                (Some(_), _) => Base::Keep,
                (None, true) => Base::Restore(&base),
                (None, false) => Base::Clear,
            },
        )?;
        self.timing_mark(1);
        report.step("base");
        if complete {
            self.upload_scene_buffers(scene)?;
            self.prepare_batches(batches)?;
            self.timing_mark(2);
            report.step("upload");
        }
        if let Some(rects) = &redraw {
            if !rects.is_empty() {
                report.local =
                    self.draw_region(scene, &batches[..prefix_end], rects, &clear_color, report)?;
            }
            // The prefix alone is drawn there: the cache takes the repair.
            if let Some(rect) = repair_rect {
                self.save_scene_prefix(prefix_key, Some(rect))?;
            }
        } else if let Some(rect) = repair_rect {
            if complete {
                self.clear_and_scissor(rect, &clear_color)?;
                self.draw_batches(scene, batches, 0..prefix_end)?;
                self.restore_rasterizer();
            } else {
                report.local =
                    self.draw_region(scene, &batches[..prefix_end], &[rect], &clear_color, report)?;
            }
            self.save_scene_prefix(prefix_key, Some(rect))?;
        } else if !reuse_background {
            self.draw_batches(scene, batches, 0..prefix_end)?;
            if keep_complete {
                self.save_scene_prefix(prefix_key, None)?;
            }
        }
        self.timing_mark(3);
        report.step("prefixDrawn");
        if draw_suffix {
            if composed.is_some() {
                self.compose_suffix()?;
            }
            self.draw_batches(scene, batches, prefix_end..batches.len())?;
            report.step("suffixDrawn");
        }
        let r = self.resources.as_mut().context("resources missing")?;
        let intact = !suffix_changed
            && !suffix_damage
                .as_ref()
                .is_some_and(|rects| restore.reaches(rects));
        if suffix_needed || intact {
            r.suffix_damage = suffix_damage;
            let old = std::mem::replace(&mut r.suffix_snapshot, suffix_ink);
            r.recycle(old);
        } else {
            // The buffer may hold the old suffix, the restored background and
            // nothing of the new one: the next frame that shows it redraws all.
            let old = r.suffix_snapshot.take();
            r.recycle(old);
            r.recycle(suffix_ink);
            r.suffix_damage = match (r.suffix_damage.take(), suffix_damage) {
                (Some(mut old), Some(now)) => {
                    for rect in now {
                        merge_damage(&mut old, rect);
                    }
                    Some(old)
                }
                _ => None,
            };
        }
        if !same_prefix {
            let old = std::mem::replace(&mut r.prefix_snapshot, snapshot);
            r.recycle(old);
        }
        if repair.is_some() {
            r.prefix_key = Some(prefix_key);
        } else if !reuse_background && !keep_complete {
            r.prefix_key = None;
        }
        r.streak = !reuse_background && (locality.is_none() || large);
        r.last = Some(Last::Cached { opaque });
        r.frames.drew(change);
        r.present_dirty = present_regions(partial, &r.frames.unpresented);
        report.step("record");
        self.scene_cache_event(
            scene,
            reuse_prefix,
            composed.is_some(),
            draw_suffix,
            prefix_end,
            batches.len(),
        );
        Ok(())
    }

    /// Clears each of `rects` and draws into it, under a scissor, what
    /// `batches` of `scene` draw there. When they can all be culled, only the
    /// primitives that reach the rects are uploaded and drawn: their count
    /// and the scene's are returned.
    pub(super) fn draw_region(
        &mut self,
        scene: &Scene,
        batches: &[PrimitiveBatch],
        rects: &[RECT],
        clear_color: &[f32; 4],
        report: &mut FrameReport,
    ) -> Result<Option<(usize, usize)>> {
        let mut local = std::mem::take(&mut self.local);
        let mut local_batches = std::mem::take(&mut self.local_batches);
        let culled = scene_cache::local_scene(&mut local, scene, batches, rects);
        local_batches.clear();
        if culled {
            local_batches.extend(local.batches());
        }
        report.step("cull");
        let result = (|| {
            let (draw_scene, draw_batches) = if culled {
                // The next complete upload must not take these for the scene's.
                self.pipelines.uploaded_revision = None;
                self.upload_scene_buffers(&local)?;
                self.pipelines.uploaded_revision = None;
                (&local, local_batches.as_slice())
            } else {
                self.upload_scene_buffers(scene)?;
                (scene, batches)
            };
            self.prepare_batches(draw_batches)?;
            self.timing_mark(2);
            report.step("localUpload");
            for rect in rects {
                self.clear_and_scissor(*rect, clear_color)?;
                self.draw_batches(draw_scene, draw_batches, 0..draw_batches.len())?;
            }
            self.restore_rasterizer();
            Ok(culled.then(|| (primitive_count(&local), primitive_count(scene))))
        })();
        self.local = local;
        self.local_batches = local_batches;
        result
    }

    /// Clears `rect` and confines the following draws to it until
    /// [`restore_rasterizer`](Self::restore_rasterizer).
    fn clear_and_scissor(&mut self, rect: RECT, clear_color: &[f32; 4]) -> Result<()> {
        let r = self.resources.as_ref().context("resources missing")?;
        let ctx = &self
            .devices
            .as_ref()
            .context("devices missing")?
            .device_context;
        unsafe {
            ctx.cast::<ID3D11DeviceContext1>()?.ClearView(
                r.render_target_view
                    .as_ref()
                    .context("render target view missing")?,
                clear_color,
                Some(&[rect]),
            );
            ctx.RSSetState(&r.prefix_scissor);
            ctx.RSSetScissorRects(Some(&[rect]));
        }
        self.work.clears += 1;
        Ok(())
    }

    pub(super) fn restore_rasterizer(&self) {
        if let (Some(devices), Some(r)) = (&self.devices, &self.resources) {
            unsafe { devices.device_context.RSSetState(&r.rasterizer) };
        }
    }

    /// Lets the suffix's images compose the cached prefix behind themselves.
    fn compose_suffix(&mut self) -> Result<()> {
        let r = self.resources.as_mut().context("resources missing")?;
        r.compose_prefix = true;
        let ctx = &self
            .devices
            .as_ref()
            .context("devices missing")?
            .device_context;
        if let Some(mut params) = self.globals.last_params {
            params.compose_prefix = 1;
            if self.globals.last_params != Some(params) {
                update_buffer(
                    ctx,
                    self.globals
                        .global_params_buffer
                        .as_ref()
                        .context("global params missing")?,
                    &[params],
                )?;
                self.globals.last_params = Some(params);
            }
        }
        unsafe { ctx.PSSetShaderResources(2, Some(slice::from_ref(&r.prefix_view))) };
        Ok(())
    }

    pub(super) fn prepare_batches(&mut self, batches: &[PrimitiveBatch]) -> Result<()> {
        let devices = self.devices.as_ref().context("devices missing")?;
        self.globals
            .batch_params
            .prepare(&devices.device, &devices.device_context, batches)
    }

    fn timing_mark(&mut self, index: usize) {
        if let Some(timing) = &mut self.gpu_timing {
            timing.mark(index);
        }
    }

    /// Draws `batches[range]` of `scene` from the instances uploaded for it,
    /// after [`prepare_batches`](Self::prepare_batches) of all `batches`.
    pub(super) fn draw_batches(
        &mut self,
        scene: &Scene,
        batches: &[PrimitiveBatch],
        range: Range<usize>,
    ) -> Result<()> {
        let annotation = self
            .devices
            .as_ref()
            .and_then(|devices| devices.annotation.clone())
            .filter(|annotation| unsafe { annotation.GetStatus().as_bool() });
        for index in range {
            let batch = &batches[index];
            self.globals.batch_params.active = index;
            let _annotation = annotation
                .as_ref()
                .map(|annotation| Annotation::new(annotation, HSTRING::from(batch.label())));
            match batch {
                PrimitiveBatch::Shadows(range) => self.draw_shadows(range.start, range.len()),
                PrimitiveBatch::Quads(range) => self.draw_quads(range.start, range.len()),
                PrimitiveBatch::Paths(range) => {
                    let paths = &scene.paths[range.clone()];
                    self.draw_paths_to_intermediate(paths)?;
                    self.draw_paths_from_intermediate(paths)
                }
                PrimitiveBatch::Underlines(range) => {
                    self.draw_underlines(range.start, range.len())
                }
                PrimitiveBatch::MonochromeSprites { texture_id, range } => {
                    self.draw_monochrome_sprites(*texture_id, range.start, range.len())
                }
                PrimitiveBatch::SubpixelSprites { texture_id, range } => {
                    self.draw_subpixel_sprites(*texture_id, range.start, range.len())
                }
                PrimitiveBatch::PolychromeSprites { texture_id, range } => {
                    self.draw_polychrome_sprites(*texture_id, range.start, range.len())
                }
                PrimitiveBatch::Surfaces(range) => {
                    self.draw_surfaces(&scene.surfaces[range.clone()])
                }
            }
            .with_context(|| {
                format!(
                    "scene too large:\
                    {} paths, {} shadows, {} quads, {} underlines, {} mono, {} subpixel, {} poly, {} surfaces",
                    scene.paths.len(),
                    scene.shadows.len(),
                    scene.quads.len(),
                    scene.underlines.len(),
                    scene.monochrome_sprites.len(),
                    scene.subpixel_sprites.len(),
                    scene.polychrome_sprites.len(),
                    scene.surfaces.len(),
                )
            })?;
        }
        Ok(())
    }

    pub(super) fn scene_cache_event(
        &self,
        scene: &Scene,
        reused: bool,
        composed: bool,
        suffix: bool,
        prefix_batches: usize,
        batches: usize,
    ) {
        static DIAGNOSTICS: OnceLock<bool> = OnceLock::new();
        if !*DIAGNOSTICS.get_or_init(|| std::env::var_os("ARGUS_GPUI_DIAGNOSTICS").is_some()) {
            return;
        }
        static FRAMES: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        static COMPOSED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
        let frame = FRAMES.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let changed = COMPOSED.swap(composed, std::sync::atomic::Ordering::Relaxed) != composed;
        if frame < 3 || frame.is_multiple_of(400) || changed {
            let area = self
                .resources
                .as_ref()
                .and_then(|r| r.present_dirty.as_ref())
                .map(|rects| {
                    rects
                        .iter()
                        .map(|r| (r.right - r.left) * (r.bottom - r.top))
                        .sum::<i32>()
                })
                .unwrap_or(0);
            println!(
                "{{\"event\":\"gpu-scene-cache\",\"frame\":{frame},\"revision\":{},\"animated\":{},\"reused\":{reused},\"composed\":{composed},\"suffixDrawn\":{suffix},\"prefixBatches\":{prefix_batches},\"batches\":{batches},\"dirtyArea\":{area}}}",
                scene.paint_revision,
                scene.animated_order.is_some(),
            );
        }
    }

    fn save_scene_prefix(&mut self, key: (u64, usize, bool), dirty: Option<RECT>) -> Result<()> {
        let devices = self.devices.as_ref().context("devices missing")?;
        let resources = self.resources.as_mut().context("resources missing")?;
        let target = resources
            .render_target
            .as_ref()
            .context("render target missing")?;
        unsafe {
            if resources.prefix.is_none() {
                let mut desc = D3D11_TEXTURE2D_DESC::default();
                target.GetDesc(&mut desc);
                desc.BindFlags = D3D11_BIND_SHADER_RESOURCE.0 as u32;
                desc.MiscFlags = 0;
                devices
                    .device
                    .CreateTexture2D(&desc, None, Some(&mut resources.prefix))?;
                devices.device.CreateShaderResourceView(
                    resources.prefix.as_ref().context("prefix texture")?,
                    None,
                    Some(&mut resources.prefix_view),
                )?;
            }
            // CopyResource accepts a bound render target, so the copy does not
            // require changing output-merger bindings between batches.
            let prefix = resources.prefix.as_ref().context("scene prefix missing")?;
            if let Some(rect) = dirty {
                let region = D3D11_BOX {
                    left: rect.left as u32,
                    top: rect.top as u32,
                    front: 0,
                    right: rect.right as u32,
                    bottom: rect.bottom as u32,
                    back: 1,
                };
                devices.device_context.CopySubresourceRegion(
                    prefix,
                    0,
                    region.left,
                    region.top,
                    0,
                    target,
                    0,
                    Some(&region),
                );
                self.work.copy_pixels +=
                    u64::from(region.right - region.left) * u64::from(region.bottom - region.top);
            } else {
                devices.device_context.CopyResource(prefix, target);
                self.work.copy_pixels += u64::from(self.width) * u64::from(self.height);
            }
            self.work.copies += 1;
        }
        resources.prefix_key = Some(key);
        Ok(())
    }
}
