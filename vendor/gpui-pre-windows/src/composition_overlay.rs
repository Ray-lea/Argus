use super::*;

#[derive(Clone)]
pub(super) struct CompositionOverlay {
    size: (u32, u32),
    root: IDCompositionVisual,
    base: IDCompositionSurface,
    foreground: IDCompositionSurface,
}

impl CompositionOverlay {
    fn new(device: &IDCompositionDevice, size: (u32, u32)) -> Result<Self> {
        unsafe {
            let surface = || {
                device.CreateSurface(
                    size.0,
                    size.1,
                    RENDER_TARGET_FORMAT,
                    DXGI_ALPHA_MODE_PREMULTIPLIED,
                )
            };
            let base = surface()?;
            let foreground = surface()?;
            let root = device.CreateVisual()?;
            let front = device.CreateVisual()?;
            root.SetContent(&base)?;
            front.SetContent(&foreground)?;
            root.AddVisual(&front, true, None)?;
            Ok(Self {
                size,
                root,
                base,
                foreground,
            })
        }
    }
}

impl DirectXRenderer {
    pub(crate) fn supports_composition_overlay(&self) -> bool {
        self.retired.is_none()
            && !self.skip_draws
            && self
                .direct_composition
                .as_ref()
                .is_some_and(|comp| comp.swap_visual.is_some() && !comp.overlay_unavailable)
    }

    pub(super) fn present_composition_overlay(
        &mut self,
        scene: &Scene,
        order: DrawOrder,
    ) -> Result<()> {
        let size = (self.width, self.height);
        let comp = self
            .direct_composition
            .as_ref()
            .context("missing compositor")?;
        let previous = comp.overlay.as_ref().filter(|layers| layers.size == size);
        let attaching = previous.is_none();
        let layers = match previous {
            Some(layers) => layers.clone(),
            None => CompositionOverlay::new(&comp.comp_device, size)?,
        };
        let batches = split_batches(scene, Some(order));
        let cut = batches.partition_point(|batch| batch_last_order(scene, batch) < order);
        let resources = self
            .resources
            .as_mut()
            .context("missing overlay resources")?;
        resources.compose_prefix = false;
        let target = resources
            .render_target
            .clone()
            .context("missing overlay target")?;
        self.upload_scene_buffers(scene)?;
        self.prepare_batches(&batches)?;
        for (surface, range) in [
            (&layers.base, 0..cut),
            (&layers.foreground, cut..batches.len()),
        ] {
            self.pre_draw(&[0.; 4], Base::Clear)?;
            self.draw_batches(scene, &batches, range)?;
            let devices = self.devices.as_ref().context("missing overlay device")?;
            chart_composition::copy_scene(surface, devices, &target, None)?;
        }
        let comp = self.direct_composition.as_mut().unwrap();
        unsafe {
            if attaching {
                comp.comp_target.SetRoot(&layers.root)?;
            }
            comp.comp_device.Commit()?;
        }
        if attaching {
            comp.charts.deactivate();
            comp.overlay = Some(layers);
            if chart_composition::diagnostics() {
                println!(
                    "{{\"event\":\"gpu-composition-overlay\",\"active\":true,\"baseBatches\":{cut},\"batches\":{}}}",
                    batches.len()
                );
            }
        }
        // The scratch target now contains only the foreground, even if a
        // partial frame in the normal renderer previously covered the same area.
        self.resources.as_mut().unwrap().forget();
        Ok(())
    }

    pub(super) fn finish_composition_overlay(
        &mut self,
        scene: &Scene,
        background: WindowBackgroundAppearance,
    ) -> Result<()> {
        self.render(scene, background, false, None)?;
        self.present()?;
        let comp = self
            .direct_composition
            .as_mut()
            .context("missing compositor")?;
        unsafe {
            comp.comp_target.SetRoot(comp.swap_visual.as_ref())?;
            comp.comp_device.Commit()?;
        }
        comp.overlay = None;
        if chart_composition::diagnostics() {
            println!("{{\"event\":\"gpu-composition-overlay\",\"active\":false}}");
        }
        Ok(())
    }
}
