//! Argus patch: with `ARGUS_GPUI_COMPOSED_PRESENT`, frames reach the screen
//! through a DirectComposition bitmap instead of a flip of the swap chain.
use super::*;

pub(super) struct FlatBitmap {
    surface: IDCompositionSurface,
    size: (u32, u32),
    /// A bitmap's first update covers all of it.
    drawn: bool,
}

/// Whether frames reach the screen through a DirectComposition bitmap rather
/// than by flipping the swap chain (`ARGUS_GPUI_COMPOSED_PRESENT`). The
/// compositor always draws a bitmap itself; a flip chain it may hand to a
/// hardware overlay plane, whose blending with what lies behind a translucent
/// window is the display driver's.
pub(super) fn composed_present_requested() -> bool {
    static REQUESTED: OnceLock<bool> = OnceLock::new();
    *REQUESTED.get_or_init(|| std::env::var_os("ARGUS_GPUI_COMPOSED_PRESENT").is_some())
}

impl DirectXRenderer {
    /// Copies the frame into the bitmap of the flat visual and commits it, in
    /// place of a flip. The render target is then never presented, as while a
    /// chart tree is shown, and keeps the last frame for partial paints.
    pub(super) fn present_composed(&mut self) -> Result<()> {
        let devices = self.devices.as_ref().context("devices missing")?;
        let resources = self.resources.as_ref().context("resources missing")?;
        let source = resources
            .render_target
            .as_ref()
            .context("render target missing")?;
        let comp = self
            .direct_composition
            .as_mut()
            .context("composition missing")?;
        let visual = comp.swap_visual.as_ref().context("flat visual missing")?;
        let size = (self.width, self.height);
        if comp.flat.as_ref().is_none_or(|flat| flat.size != size) {
            let surface = unsafe {
                comp.comp_device.CreateSurface(
                    size.0,
                    size.1,
                    RENDER_TARGET_FORMAT,
                    DXGI_ALPHA_MODE_PREMULTIPLIED,
                )?
            };
            unsafe { visual.SetContent(&surface)? };
            if std::env::var_os("ARGUS_GPUI_DIAGNOSTICS").is_some() {
                println!(
                    "{{\"event\":\"gpu-composed-present\",\"width\":{},\"height\":{}}}",
                    size.0, size.1
                );
            }
            comp.flat = Some(FlatBitmap {
                surface,
                size,
                drawn: false,
            });
        }
        let flat = comp.flat.as_mut().context("flat bitmap missing")?;
        match resources.present_dirty.as_deref().filter(|_| flat.drawn) {
            Some(rects) => {
                for rect in rects {
                    chart_composition::copy_scene(&flat.surface, devices, source, Some(*rect))?;
                }
            }
            None => chart_composition::copy_scene(&flat.surface, devices, source, None)?,
        }
        flat.drawn = true;
        unsafe { comp.comp_device.Commit()? };
        self.note_presented();
        Ok(())
    }
}
