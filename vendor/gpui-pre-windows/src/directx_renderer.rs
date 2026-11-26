use std::{
    collections::VecDeque,
    ops::Range,
    slice,
    sync::{Arc, OnceLock},
};

use anyhow::{Context, Result};
use gpui_util::ResultExt;
use windows::{
    Win32::{
        Foundation::{HWND, RECT, S_OK},
        Graphics::{
            Direct3D::*,
            Direct3D11::*,
            DirectComposition::*,
            DirectWrite::*,
            Dxgi::{Common::*, *},
        },
    },
    core::{HSTRING, Interface},
};

use crate::directx_renderer::shader_resources::{RawShaderBytes, ShaderModule, ShaderTarget};
use crate::*;
use gpui::*;

#[cfg(test)]
#[path = "retained_tests.rs"]
mod retained_tests;

#[path = "chart_composition.rs"]
mod chart_composition;

#[path = "composition_overlay.rs"]
mod composition_overlay;
use composition_overlay::CompositionOverlay;

#[path = "scene_cache.rs"]
mod scene_cache;
#[path = "scroll_copy.rs"]
mod scroll_copy;

#[path = "frame_damage.rs"]
mod frame_damage;
use frame_damage::{Damage, Frames, merge_damage, overlaps, passes, present_regions};
#[path = "scene_split.rs"]
mod scene_split;
use scene_split::{
    batch_last_order, independent_sprite_regions, primitive_count, split_batches, subtract_rect,
    suffix_damage,
};
#[path = "retained_frame.rs"]
mod retained_frame;
use retained_frame::{Base, FrameReport, Last};
#[path = "batch_constants.rs"]
mod batch_constants;
use batch_constants::BatchConstants;
#[path = "composed_present.rs"]
mod composed_present;
use composed_present::{FlatBitmap, composed_present_requested};
#[path = "path_targets.rs"]
mod path_targets;
use path_targets::{PathTargets, path_target_trace};

pub(crate) const DISABLE_DIRECT_COMPOSITION: &str = "GPUI_DISABLE_DIRECT_COMPOSITION";

const RENDER_TARGET_FORMAT: DXGI_FORMAT = DXGI_FORMAT_B8G8R8A8_UNORM;
// This configuration is used for MSAA rendering on paths only, and it's guaranteed to be supported by DirectX 11.
const PATH_MULTISAMPLE_COUNT: u32 = 4;
const MAX_INSTANCE_BUFFER_SIZE: usize = 256 * 1024 * 1024;
/// Compositor passes the old frame covers a window that changes device.
const TAKEOVER_PASSES: usize = 4;

pub(crate) struct FontInfo {
    pub gamma_ratios: [f32; 4],
    pub grayscale_enhanced_contrast: f32,
    pub subpixel_enhanced_contrast: f32,
    pub is_bgr: bool,
}

pub(crate) struct DirectXRenderer {
    hwnd: HWND,
    atlas: Arc<DirectXAtlas>,
    gpu_timing: Option<crate::gpu_timing::GpuTimings<5>>,
    work: FrameWork,
    last_atlas_stats: (u64, u64),
    devices: Option<DirectXRendererDevices>,
    resources: Option<DirectXResources>,
    globals: DirectXGlobalElements,
    pipelines: DirectXRenderPipelines,
    direct_composition: Option<DirectComposition>,
    font_info: &'static FontInfo,
    /// Atlas tiles uploaded since the last frame was rendered.
    fresh: scene_cache::FreshTiles,
    /// The primitives of the last local repaint and their batches, kept for
    /// their buffers.
    local: Scene,
    local_batches: Vec<PrimitiveBatch>,
    /// When a scene last moved something without a paint (chart curves,
    /// load bars), and the revision of the scene last drawn.
    charts_moved: Option<std::time::Instant>,
    drawn_revision: u64,

    width: u32,
    height: u32,

    /// Whether we want to skip drwaing due to device lost events.
    ///
    /// In that case we want to discard the first frame that we draw as we got reset in the middle of a frame
    /// meaning we lost all the allocated gpu textures and scene resources.
    skip_draws: bool,
    disable_direct_composition: bool,
    /// Whether the devices were made for a software-rendering request.
    software: bool,
    /// What showed the window before a requested change of device, still on
    /// screen until the new device has presented a frame
    /// (`finish_device_change`).
    retired: Option<Retired>,
    /// The scroll containers of earlier scenes, by id, as last prepainted.
    scroll_seen: std::collections::HashMap<usize, ScrollRegion>,
}

/// The objects of a replaced device that still works, released in this
/// order: the composition target and its tree, the swap chain, the device.
#[derive(Default)]
struct Retired(
    Option<DirectComposition>,
    Option<DirectXResources>,
    Option<DirectXRendererDevices>,
);

/// Direct3D objects
#[derive(Clone)]
pub(crate) struct DirectXRendererDevices {
    pub(crate) adapter: IDXGIAdapter1,
    pub(crate) dxgi_factory: IDXGIFactory6,
    pub(crate) device: ID3D11Device,
    pub(crate) device_context: ID3D11DeviceContext,
    dxgi_device: Option<IDXGIDevice>,
    annotation: Option<ID3DUserDefinedAnnotation>,
}

struct DirectXResources {
    /// The static prefix of the scene last drawn with one, while
    /// `prefix_key` names it.
    prefix: Option<ID3D11Texture2D>,
    prefix_view: Option<ID3D11ShaderResourceView>,
    compose_prefix: bool,
    prefix_key: Option<(u64, usize, bool)>,
    /// The prefix ink of the last frame drawn over a cached prefix.
    prefix_snapshot: Option<scene_cache::Snapshot>,
    prefix_scissor: ID3D11RasterizerState,
    rasterizer: ID3D11RasterizerState,
    /// Where the last drawn suffix painted, and its ink.
    suffix_damage: Option<Vec<RECT>>,
    suffix_snapshot: Option<scene_cache::Snapshot>,
    /// Buffers of replaced snapshots, filled again by later frames.
    spare: Vec<scene_cache::Snapshot>,
    /// How the last frame was drawn, for the next one to compare with.
    last: Option<Last>,
    /// The last frame repainted its whole prefix for a change that was not
    /// local: the next such frame does not keep its prefix either.
    streak: bool,
    frames: Frames,
    present_dirty: Option<Vec<RECT>>,
    // Direct3D rendering objects
    swap_chain: IDXGISwapChain1,
    render_target: Option<ID3D11Texture2D>,
    render_target_view: Option<ID3D11RenderTargetView>,

    paths: Option<PathTargets>,
    /// The last frame, kept while a list scrolls (`scroll_copy`).
    scroll_copy: Option<scroll_copy::ScrollCopy>,

    // Cached viewport
    viewport: D3D11_VIEWPORT,
}

struct DirectXRenderPipelines {
    uploaded_revision: Option<u64>,
    shadow_pipeline: PipelineState<Shadow>,
    quad_pipeline: PipelineState<Quad>,
    path_rasterization_pipeline: PipelineState<PathRasterizationSprite>,
    path_sprite_pipeline: PipelineState<PathSprite>,
    underline_pipeline: PipelineState<Underline>,
    mono_sprites: PipelineState<MonochromeSprite>,
    subpixel_sprites: PipelineState<SubpixelSprite>,
    poly_sprites: PipelineState<PolychromeSprite>,
}

#[derive(Clone, Copy, Default)]
struct FrameWork {
    copies: u64,
    copy_pixels: u64,
    clears: u64,
    resolves: u64,
    resolve_pixels: u64,
}

struct DirectXGlobalElements {
    replacement_blend: ID3D11BlendState,
    last_params: Option<GlobalParams>,
    global_params_buffer: Option<ID3D11Buffer>,
    batch_params: BatchConstants,
    sampler: Option<ID3D11SamplerState>,
}

struct Annotation<'a>(&'a ID3DUserDefinedAnnotation);

impl<'a> Annotation<'a> {
    fn new(annotation: &'a ID3DUserDefinedAnnotation, label: HSTRING) -> Self {
        unsafe { annotation.BeginEvent(&label) };
        Self(annotation)
    }
}

impl Drop for Annotation<'_> {
    fn drop(&mut self) {
        unsafe { self.0.EndEvent() };
    }
}

struct DirectComposition {
    comp_device: IDCompositionDevice,
    comp_target: IDCompositionTarget,
    swap_visual: Option<IDCompositionVisual>,
    /// With `ARGUS_GPUI_COMPOSED_PRESENT`: the bitmap `swap_visual` shows in
    /// place of the flip chain.
    flat: Option<FlatBitmap>,
    overlay: Option<CompositionOverlay>,
    overlay_unavailable: bool,
    charts: chart_composition::ChartComposition,
}

impl DirectXRendererDevices {
    pub(crate) fn new(
        directx_devices: &DirectXDevices,
        disable_direct_composition: bool,
    ) -> Result<Self> {
        let DirectXDevices {
            adapter,
            dxgi_factory,
            device,
            device_context,
            ..
        } = directx_devices;
        let dxgi_device = if disable_direct_composition {
            None
        } else {
            Some(device.cast().context("Creating DXGI device")?)
        };
        let annotation = device_context.cast().ok();

        Ok(Self {
            adapter: adapter.clone(),
            dxgi_factory: dxgi_factory.clone(),
            device: device.clone(),
            device_context: device_context.clone(),
            dxgi_device,
            annotation,
        })
    }
}

impl DirectXRenderer {
    pub(crate) fn new(
        hwnd: HWND,
        directx_devices: &DirectXDevices,
        disable_direct_composition: bool,
    ) -> Result<Self> {
        if disable_direct_composition {
            log::info!("Direct Composition is disabled.");
        }

        let devices = DirectXRendererDevices::new(directx_devices, disable_direct_composition)
            .context("Creating DirectX devices")?;

        let hwnd_swap_chain = disable_direct_composition || windows_opaque_window_requested();
        let resources = DirectXResources::new(&devices, 1, 1, hwnd, hwnd_swap_chain)
            .context("Creating DirectX resources")?;
        let globals = DirectXGlobalElements::new(&devices.device)
            .context("Creating DirectX global elements")?;
        let pipelines = DirectXRenderPipelines::new(&devices.device)
            .context("Creating DirectX render pipelines")?;

        let direct_composition = DirectComposition::for_window(
            &devices,
            hwnd,
            (!hwnd_swap_chain).then_some(&resources.swap_chain),
        )?;

        let atlas = Arc::new(DirectXAtlas::new(
            &devices.device,
            &devices.device_context,
            direct_composition
                .as_ref()
                .is_some_and(|c| c.charts.enabled()),
        ));
        Ok(DirectXRenderer {
            hwnd,
            atlas,
            gpu_timing: crate::gpu_timing::GpuTimings::from_environment(
                &devices.device,
                &devices.device_context,
                "render",
            ),
            work: FrameWork::default(),
            last_atlas_stats: (0, 0),
            devices: Some(devices),
            resources: Some(resources),
            globals,
            pipelines,
            direct_composition,
            font_info: Self::get_font_info(),
            fresh: Default::default(),
            local: Scene::default(),
            local_batches: Vec::new(),
            charts_moved: None,
            drawn_revision: 0,
            width: 1,
            height: 1,
            skip_draws: false,
            disable_direct_composition,
            software: directx_devices.software,
            retired: None,
            scroll_seen: Default::default(),
        })
    }

    pub(crate) fn sprite_atlas(&self) -> Arc<dyn PlatformAtlas> {
        self.atlas.clone()
    }

    fn pre_draw(&mut self, clear_color: &[f32; 4], base: Base<'_>) -> Result<()> {
        let resources = self.resources.as_ref().expect("resources missing");
        let device_context = &self
            .devices
            .as_ref()
            .expect("devices missing")
            .device_context;
        static LINEAR_AREA: OnceLock<bool> = OnceLock::new();
        let linear_area_filter = u32::from(
            *LINEAR_AREA
                .get_or_init(|| std::env::var_os("ARGUS_GPUI_DISABLE_LINEAR_AREA").is_none()),
        );
        let params = GlobalParams {
            gamma_ratios: self.font_info.gamma_ratios,
            viewport_size: [resources.viewport.Width, resources.viewport.Height],
            grayscale_enhanced_contrast: self.font_info.grayscale_enhanced_contrast,
            subpixel_enhanced_contrast: self.font_info.subpixel_enhanced_contrast,
            is_bgr: self.font_info.is_bgr as u32,
            compose_prefix: resources.compose_prefix as u32,
            linear_area_filter,
            _pad: 0,
        };
        if self.globals.last_params != Some(params) {
            update_buffer(
                device_context,
                self.globals.global_params_buffer.as_ref().unwrap(),
                &[params],
            )?;
            self.globals.last_params = Some(params);
        }
        unsafe {
            if let Base::Restore(regions) = base {
                let target = resources
                    .render_target
                    .as_ref()
                    .context("render target missing")?;
                let prefix = resources.prefix.as_ref().context("scene prefix missing")?;
                if let Damage::Rects(rects) = regions {
                    for rect in rects {
                        let region = D3D11_BOX {
                            left: rect.left as u32,
                            top: rect.top as u32,
                            front: 0,
                            right: rect.right as u32,
                            bottom: rect.bottom as u32,
                            back: 1,
                        };
                        device_context.CopySubresourceRegion(
                            target,
                            0,
                            region.left,
                            region.top,
                            0,
                            prefix,
                            0,
                            Some(&region),
                        );
                        self.work.copies += 1;
                        self.work.copy_pixels += u64::from(region.right - region.left)
                            * u64::from(region.bottom - region.top);
                    }
                } else {
                    device_context.CopyResource(target, prefix);
                    self.work.copies += 1;
                    self.work.copy_pixels += u64::from(self.width) * u64::from(self.height);
                }
            } else if let Base::Clear = base {
                self.work.clears += 1;
                device_context.ClearRenderTargetView(
                    resources
                        .render_target_view
                        .as_ref()
                        .context("missing render target view")?,
                    clear_color,
                );
            }
            device_context
                .OMSetRenderTargets(Some(slice::from_ref(&resources.render_target_view)), None);
            device_context.RSSetViewports(Some(slice::from_ref(&resources.viewport)));
            device_context
                .VSSetConstantBuffers(0, Some(slice::from_ref(&self.globals.global_params_buffer)));
            device_context
                .VSSetConstantBuffers(1, Some(slice::from_ref(&self.globals.batch_params.legacy)));
            device_context
                .PSSetConstantBuffers(0, Some(slice::from_ref(&self.globals.global_params_buffer)));
            let prefix = if resources.compose_prefix {
                resources.prefix_view.clone()
            } else {
                None
            };
            device_context.PSSetShaderResources(2, Some(slice::from_ref(&prefix)));
        }
        Ok(())
    }

    /// The last drawn frame reached the screen other than through a flip.
    fn note_presented(&mut self) {
        if let Some(r) = &mut self.resources {
            r.frames.presented();
        }
    }

    #[inline]
    fn present(&mut self) -> Result<()> {
        crate::frame_trace::record(3, 0);
        if composed_present_requested()
            && self
                .direct_composition
                .as_ref()
                .is_some_and(|comp| comp.swap_visual.is_some())
        {
            let result = self.present_composed();
            crate::frame_trace::record(4, u64::from(result.is_ok()));
            return result.context("Presenting through the compositor failed");
        }
        let resources = self.resources.as_ref().expect("resources missing");
        let parameters = resources
            .present_dirty
            .as_ref()
            .map(|rects| DXGI_PRESENT_PARAMETERS {
                DirtyRectsCount: rects.len() as u32,
                pDirtyRects: rects.as_ptr() as *mut RECT,
                ..Default::default()
            })
            .unwrap_or_default();
        let result = unsafe {
            resources
                .swap_chain
                .Present1(1, DXGI_PRESENT(0), &parameters)
        };
        crate::frame_trace::record(4, u64::from(result.is_ok()));
        if let Some(r) = self.resources.as_mut() {
            // Any other status (an occluded window, say) leaves it open
            // whether the buffers turned.
            if result == S_OK {
                r.frames.flipped();
                r.frames.presented();
            } else {
                r.frames.forget();
            }
        }
        result.ok().context("Presenting swap chain failed")
    }

    pub(crate) fn handle_device_lost(&mut self, directx_devices: &DirectXDevices) -> Result<()> {
        try_to_recover_from_device_lost(|| {
            self.handle_device_lost_impl(directx_devices)
                .context("DirectXRenderer handling device lost")
        })
    }

    fn handle_device_lost_impl(&mut self, directx_devices: &DirectXDevices) -> Result<()> {
        let disable_direct_composition = self.disable_direct_composition;
        let hwnd_swap_chain = disable_direct_composition || windows_opaque_window_requested();
        // A requested change of renderer leaves a working device behind. Its
        // last frame stays on screen until the new device has presented one;
        // the window would otherwise show nothing while the pipelines are
        // built and the whole scene is drawn again. A window takes one flip
        // chain of its own and its frame callback stops while it is hidden or
        // minimized, so only a composed, visible window keeps the old frame.
        let keep_showing = directx_devices.software != self.software
            && !hwnd_swap_chain
            && self
                .devices
                .as_ref()
                .is_some_and(|devices| unsafe { devices.device.GetDeviceRemovedReason() }.is_ok())
            && unsafe {
                use windows::Win32::UI::WindowsAndMessaging::{IsIconic, IsWindowVisible};
                IsWindowVisible(self.hwnd).as_bool() && !IsIconic(self.hwnd).as_bool()
            };

        unsafe {
            #[cfg(debug_assertions)]
            if let Some(devices) = &self.devices {
                report_live_objects(&devices.device)
                    .context("Failed to report live objects after device lost")
                    .log_err();
            }

            let mut old = Retired(None, self.resources.take(), None);
            if !keep_showing {
                old.1 = None;
            }
            if let Some(devices) = &self.devices {
                devices.device_context.OMSetRenderTargets(None, None);
                devices.device_context.ClearState();
                devices.device_context.Flush();
                #[cfg(debug_assertions)]
                if !keep_showing {
                    report_live_objects(&devices.device)
                        .context("Failed to report live objects after device lost")
                        .log_err();
                }
            }

            old.0 = self.direct_composition.take();
            old.2 = self.devices.take();
            if !keep_showing {
                // With what an unfinished change still kept on screen.
                self.retired = None;
            } else if self.retired.is_none() {
                self.retired = Some(old);
            }
            // Otherwise an earlier change still waits for its first frame:
            // what it replaced stays on screen, what it made is released.
        }

        let devices = DirectXRendererDevices::new(directx_devices, disable_direct_composition)
            .context("Recreating DirectX devices")?;
        let resources = DirectXResources::new(
            &devices,
            self.width,
            self.height,
            self.hwnd,
            hwnd_swap_chain,
        )
        .context("Creating DirectX resources")?;
        let globals = DirectXGlobalElements::new(&devices.device)
            .context("Creating DirectXGlobalElements")?;
        let pipelines = DirectXRenderPipelines::new(&devices.device)
            .context("Creating DirectXRenderPipelines")?;

        // While the old frame is shown, the window's target is still that of
        // the old device: this one is bound after its first present.
        let direct_composition = if self.retired.is_some() {
            None
        } else {
            DirectComposition::for_window(
                &devices,
                self.hwnd,
                (!hwnd_swap_chain).then_some(&resources.swap_chain),
            )?
        };
        let chart_layers = match &direct_composition {
            Some(composition) => composition.charts.enabled(),
            // As the composition bound later will find.
            None if self.retired.is_some() => devices
                .dxgi_device
                .as_ref()
                .is_some_and(|dxgi| {
                    chart_composition::ChartComposition::new(dxgi, self.hwnd).enabled()
                }),
            None => false,
        };

        self.atlas
            .handle_device_lost(&devices.device, &devices.device_context, chart_layers);
        self.fresh.clear();
        self.gpu_timing = crate::gpu_timing::GpuTimings::from_environment(
            &devices.device,
            &devices.device_context,
            "render",
        );
        self.last_atlas_stats = (0, 0);

        unsafe {
            devices
                .device_context
                .OMSetRenderTargets(Some(slice::from_ref(&resources.render_target_view)), None);
        }
        self.devices = Some(devices);
        self.resources = Some(resources);
        self.globals = globals;
        self.pipelines = pipelines;
        self.direct_composition = direct_composition;
        self.software = directx_devices.software;
        self.skip_draws = true;
        Ok(())
    }

    /// Shows the window from the new device once its swap chain has been
    /// presented to, and releases what showed it until then. The old target
    /// goes right before the new one is made, since a window has one of a
    /// kind. A failure leaves the change unfinished for the next frame.
    fn finish_device_change(&mut self) -> Result<()> {
        let Some(retired) = &mut self.retired else {
            return Ok(());
        };
        // Only the target goes between the two: releasing a swap chain and
        // its device takes long enough for the compositor to show the window
        // without either.
        let Retired(old_composition, old_resources, old_devices) = std::mem::take(retired);
        let devices = self.devices.as_ref().context("devices missing")?;
        let resources = self.resources.as_ref().context("resources missing")?;
        let bound = DirectComposition::for_window_after(
            devices,
            self.hwnd,
            Some(&resources.swap_chain),
            old_resources.as_ref().map(|old| &old.swap_chain),
            move || drop(old_composition),
        )
        .or_else(|error| {
            // The old frame could not cover the takeover (its device may be
            // gone by now). The old target is released either way: bind the
            // new chain alone rather than leave the window without one.
            log::warn!("Taking the window over from the old device: {error:#}");
            DirectComposition::for_window(devices, self.hwnd, Some(&resources.swap_chain))
        })
        .context("Binding the window to the new device");
        drop(old_resources);
        drop(old_devices);
        self.direct_composition = bound?;
        self.retired = None;
        Ok(())
    }

    pub(crate) fn draw(
        &mut self,
        scene: &Scene,
        background_appearance: WindowBackgroundAppearance,
    ) -> Result<()> {
        crate::frame_trace::record(6, u64::from(scene.animated_order.is_some()));
        self.atlas.finish_page_updates(true);
        if self.skip_draws {
            // skip drawing this frame, we just recovered from a device lost event
            // and so likely do not have the textures anymore that are required for drawing
            return Ok(());
        }
        if scene.paths.is_empty() {
            self.release_path_targets();
        }
        // Kept until a frame is rendered: chart frames that only move
        // compositor visuals compare with the last rendered frame as well.
        self.fresh.extend(self.atlas.take_fresh_tiles());
        // Drawn again without a paint, the scene moved something in place.
        if scene.paint_revision != 0 && scene.paint_revision == self.drawn_revision {
            self.charts_moved = Some(std::time::Instant::now());
        }
        self.drawn_revision = scene.paint_revision;
        if let Some(order) = scene.composition_overlay
            && self.supports_composition_overlay()
        {
            match self.present_composition_overlay(scene, order) {
                Ok(()) => return Ok(()),
                Err(error) => {
                    log::warn!("Presenting composition overlay: {error:#}");
                    self.direct_composition
                        .as_mut()
                        .unwrap()
                        .overlay_unavailable = true;
                    if let Some(resources) = &mut self.resources {
                        resources.forget();
                    }
                    // Keep the previous image until the requested animation frame
                    // repaints the overlay with the single-surface fallback.
                    return Ok(());
                }
            }
        }
        if self
            .direct_composition
            .as_ref()
            .is_some_and(|comp| comp.overlay.is_some())
        {
            return self.finish_composition_overlay(scene, background_appearance);
        }
        let force = scene.force_full_redraw.replace(false) || self.fresh.len() > 4096;
        if force {
            self.fresh.clear();
            if let Some(r) = &mut self.resources {
                r.forget();
            }
            if let Some(comp) = &mut self.direct_composition {
                comp.charts.invalidate_frame_cache();
            }
        }
        let updated = if force {
            Ok(false)
        } else {
            self.update_chart_tree(scene, background_appearance)
        };
        match updated {
            Ok(true) => return Ok(()),
            Err(error) => {
                log::warn!("Updating chart visuals: {error:#}");
                self.direct_composition.as_mut().unwrap().charts.disable();
            }
            Ok(false) => {}
        }
        let was_composited = self
            .direct_composition
            .as_ref()
            .is_some_and(|c| c.charts.active());
        let tree_was_moving = self
            .direct_composition
            .as_ref()
            .is_some_and(|c| c.charts.moving_tree());
        let backdrop_patch = (!force)
            .then(|| self.plan_chart_backdrop(scene, background_appearance))
            .flatten();
        let partial = !tree_was_moving || backdrop_patch.is_some();
        // The compositor shows the charts of a patched frame: only the
        // patched rectangle of the render target reaches the screen.
        let only = backdrop_patch.as_ref().map(|patch| patch.rect());
        self.render(scene, background_appearance, partial, only)?;
        let mut restore_root = was_composited;
        if let Some(patch) = backdrop_patch {
            match self.patch_chart_backdrop(scene, background_appearance, patch) {
                Ok(true) => {
                    self.note_presented();
                    return Ok(());
                }
                Ok(false) => {}
                Err(error) => {
                    log::warn!("Updating chart backdrop: {error:#}");
                    restore_root = true;
                    self.direct_composition.as_mut().unwrap().charts.disable();
                }
            }
            // The whole frame is shown after all.
            self.render(scene, background_appearance, partial, None)?;
        }
        // Compositor visuals pay off for charts that move between paints.
        // Promoting still charts started a backdrop scan and painted the
        // foreground on every paint, and a paint with the next revision
        // restarted them before they could finish.
        let moving = self
            .charts_moved
            .is_some_and(|at| at.elapsed() < std::time::Duration::from_secs(3));
        let installed = if moving {
            self.install_chart_tree(scene, background_appearance)
        } else {
            Ok(false)
        };
        match installed {
            Ok(true) => {
                self.note_presented();
                return Ok(());
            }
            Err(error) => {
                log::warn!("Creating chart visuals: {error:#}");
                restore_root = true;
                self.direct_composition.as_mut().unwrap().charts.disable();
            }
            Ok(false) => {}
        }
        match self.present_chart_frame(scene) {
            Ok(true) => {
                self.note_presented();
                return Ok(());
            }
            Err(error) => {
                log::warn!("Presenting completed chart frame: {error:#}");
                restore_root = true;
                self.direct_composition.as_mut().unwrap().charts.disable();
            }
            Ok(false) => {}
        }
        if restore_root {
            // The flip chain may last have presented a different scene.
            if let Some(resources) = &mut self.resources {
                resources.present_dirty = None;
            }
        }
        let started = self.gpu_timing.as_ref().map(|_| std::time::Instant::now());
        let result = self.present();
        if let (Some(timing), Some(started)) = (&mut self.gpu_timing, started) {
            timing.present(started.elapsed().as_secs_f64() * 1_000_000.);
        }
        // A changed device takes the window once it has presented a frame;
        // after a failed present the old frame stays until recovery.
        let bound = if result.is_ok() {
            self.finish_device_change()
        } else {
            Ok(())
        };
        result?;
        bound?;
        if restore_root {
            let comp = self.direct_composition.as_mut().unwrap();
            unsafe {
                comp.comp_target.SetRoot(comp.swap_visual.as_ref())?;
                comp.comp_device.Commit()?;
            }
            comp.charts.deactivate();
        }
        Ok(())
    }

    /// Encode `scene` into the render target for `background_appearance`,
    /// without presenting. Shared by [`draw`](Self::draw) (which then
    /// presents) and [`render_to_image`](Self::render_to_image) (which reads
    /// the target back instead), so the two cannot drift. Afterwards the
    /// whole render target shows `scene`, although only what differs from the
    /// frame it held is drawn; `present_dirty` names what changed since a
    /// frame last reached the screen, `None` for everything or `!partial`.
    /// With `only`, the frame is shown just inside that rectangle: a chart
    /// suffix outside it is left for a later frame.
    fn render(
        &mut self,
        scene: &Scene,
        background_appearance: WindowBackgroundAppearance,
        partial: bool,
        only: Option<RECT>,
    ) -> Result<()> {
        static CACHE: OnceLock<bool> = OnceLock::new();
        let cache =
            *CACHE.get_or_init(|| std::env::var_os("ARGUS_GPUI_DISABLE_SCENE_CACHE").is_none());
        // Copy the immutable prefix as exact pixels. Every batch at or after
        // the first mutable draw order still renders with its original blend.
        // A complete interaction paint has not run a bounds-only update yet.
        // Its filtered chart images already identify the same suffix, so keep
        // the static cache available while views animate or the pointer moves.
        let animated_order = scene
            .animated_order
            .or_else(|| {
                scene
                    .polychrome_sprites
                    .iter()
                    .filter(|sprite| sprite.pad == PolychromeSprite::IN_SCENE_AREA_FILTER)
                    .map(|sprite| sprite.order)
                    .min()
            })
            .filter(|_| scene.surfaces.is_empty() && cache);
        let batches = split_batches(scene, animated_order);
        let prefix_end = animated_order
            .and_then(|order| {
                batches
                    .iter()
                    .position(|batch| batch_last_order(scene, batch) >= order)
            })
            .filter(|end| *end > 0 && scene.paint_revision != 0);
        self.fresh.extend(self.atlas.take_fresh_tiles());
        self.work = FrameWork::default();
        let stats_before = self
            .gpu_timing
            .as_ref()
            .map(|_| self.pipelines.stats())
            .unwrap_or((0, 0));
        let atlas_stats = self
            .gpu_timing
            .as_ref()
            .map(|_| self.atlas.upload_stats())
            .unwrap_or((0, 0));
        let atlas_delta = (
            atlas_stats.0.saturating_sub(self.last_atlas_stats.0),
            atlas_stats.1.saturating_sub(self.last_atlas_stats.1),
        );
        self.last_atlas_stats = atlas_stats;
        if let Some(timing) = &mut self.gpu_timing {
            timing.begin();
        }
        let render_started = self.gpu_timing.as_ref().map(|_| std::time::Instant::now());
        let mut report = FrameReport {
            started: render_started,
            ..Default::default()
        };
        // A list that only scrolled is copied from the last frame.
        let scroll = self.note_scroll(scene);
        let copied = if cache && partial && only.is_none() && scene.surfaces.is_empty() {
            self.render_scrolled(scene, &batches, background_appearance, &scroll, &mut report)
        } else {
            Ok(false)
        };
        let result = match copied {
            Ok(true) => Ok(()),
            Err(error) => Err(error),
            Ok(false) => match (prefix_end, animated_order) {
                (Some(end), Some(order)) => self.render_cached(
                    scene,
                    &batches,
                    end,
                    order,
                    background_appearance,
                    partial,
                    only,
                    &mut report,
                ),
                _ => self.render_whole(
                    scene,
                    &batches,
                    background_appearance,
                    partial,
                    cache,
                    &mut report,
                ),
            }
            .and_then(|()| {
                self.keep_scroll_copy(scene, &batches, background_appearance, &scroll, only)
            }),
        };
        // A scissored pass may have stopped halfway.
        self.restore_rasterizer();
        match &result {
            Ok(()) => self.fresh.clear(),
            Err(_) => {
                if let Some(r) = self.resources.as_mut() {
                    r.forget();
                }
            }
        }
        if let Some(timing) = &mut self.gpu_timing {
            let stats_after = self.pipelines.stats();
            let work = self.work;
            let composed = self.resources.as_ref().is_some_and(|r| r.compose_prefix);
            let dirty_pixels = self
                .resources
                .as_ref()
                .and_then(|r| r.present_dirty.as_ref())
                .map(|rects| {
                    rects
                        .iter()
                        .map(|r| (r.right - r.left) as u64 * (r.bottom - r.top) as u64)
                        .sum::<u64>()
                })
                .unwrap_or(u64::from(self.width) * u64::from(self.height));
            let cpu_us =
                render_started.map_or(0., |started| started.elapsed().as_secs_f64() * 1_000_000.);
            let mut local = report.local.map_or(String::new(), |(drawn, of)| {
                format!(",\"localPrimitives\":{drawn},\"scenePrimitives\":{of}")
            });
            if !report.steps.is_empty() {
                let steps = report
                    .steps
                    .iter()
                    .map(|(name, us)| format!("\"{name}\":{us:.1}"))
                    .collect::<Vec<_>>()
                    .join(",");
                local.push_str(&format!(",\"cpuStepsUs\":{{{steps}}}"));
            }
            timing.finish(|| format!("\"ok\":{},\"path\":\"{}\",\"reused\":{},\"repairedPixels\":{},\"suffix\":{},\"composed\":{composed},\"width\":{},\"height\":{},\"drawCalls\":{},\"instanceUploadBytes\":{},\"textureUploads\":{},\"textureUploadBytes\":{},\"copyCalls\":{},\"copyPixels\":{},\"clearCalls\":{},\"resolveCalls\":{},\"resolvePixels\":{},\"dirtyPixels\":{dirty_pixels},\"renderCpuUs\":{cpu_us}{local}", result.is_ok(), report.path, report.reused, report.repaired_pixels, report.suffix, self.width, self.height, stats_after.0.wrapping_sub(stats_before.0), stats_after.1.wrapping_sub(stats_before.1), atlas_delta.0, atlas_delta.1, work.copies, work.copy_pixels, work.clears, work.resolves, work.resolve_pixels));
        }
        result
    }

    /// Render `scene` to an offscreen CPU image **without presenting** so
    /// the window need never be shown or visible (the macOS headless path
    /// goes through MetalRenderer; this is the Windows analogue). Draws into
    /// the existing render target, copies it into a `D3D11_USAGE_STAGING`
    /// texture, maps it, and converts BGRA to RGBA.
    #[cfg(feature = "test-support")]
    pub(crate) fn render_to_image(
        &mut self,
        scene: &Scene,
        background_appearance: WindowBackgroundAppearance,
    ) -> Result<image::RgbaImage> {
        // A pending device-lost recovery (`skip_draws`) leaves the atlas holding
        // tile references from the previous device; drawing before the forced
        // re-render rebuilds them panics in `DirectXAtlasTextures::texture`.
        anyhow::ensure!(
            !self.skip_draws,
            "render_to_image unavailable while recovering from a lost device"
        );
        self.render(scene, background_appearance, false, None)?;
        self.read_render_target()
    }

    #[cfg(any(test, feature = "test-support"))]
    fn read_render_target(&self) -> Result<image::RgbaImage> {
        let devices = self.devices.as_ref().context("devices missing")?;
        let device = &devices.device;
        let context = &devices.device_context;
        let resources = self.resources.as_ref().context("resources missing")?;
        let render_target = resources
            .render_target
            .as_ref()
            .context("render target missing")?;

        // A CPU-readable copy of the render target.
        let mut desc = D3D11_TEXTURE2D_DESC::default();
        unsafe { render_target.GetDesc(&mut desc) };
        let width = desc.Width;
        let height = desc.Height;
        let staging_desc = D3D11_TEXTURE2D_DESC {
            Usage: D3D11_USAGE_STAGING,
            BindFlags: 0,
            CPUAccessFlags: D3D11_CPU_ACCESS_READ.0 as u32,
            MiscFlags: 0,
            MipLevels: 1,
            ArraySize: 1,
            SampleDesc: DXGI_SAMPLE_DESC {
                Count: 1,
                Quality: 0,
            },
            ..desc
        };
        let mut staging: Option<ID3D11Texture2D> = None;
        unsafe { device.CreateTexture2D(&staging_desc, None, Some(&mut staging))? };
        let staging = staging.context("creating staging texture")?;
        unsafe { context.CopyResource(&staging, render_target) };

        let mut mapped = D3D11_MAPPED_SUBRESOURCE::default();
        unsafe { context.Map(&staging, 0, D3D11_MAP_READ, 0, Some(&mut mapped))? };
        let row_bytes = (width as usize) * 4;
        let mut pixels = vec![0u8; row_bytes * height as usize];
        // SAFETY: `Map` succeeded, so `pData` points at `RowPitch * height`
        // readable bytes for as long as the mapping is held, and `RowPitch >=
        // row_bytes` (it only ever adds trailing padding). `pixels` is sized
        // `row_bytes * height`, so every copy stays in bounds on both sides,
        // and the regions cannot overlap (`pixels` is a fresh allocation).
        unsafe {
            let src = mapped.pData as *const u8;
            for row in 0..height as usize {
                let s = src.add(row * mapped.RowPitch as usize);
                let d = pixels.as_mut_ptr().add(row * row_bytes);
                std::ptr::copy_nonoverlapping(s, d, row_bytes);
            }
            context.Unmap(&staging, 0);
        }
        // The render target is BGRA; image::RgbaImage expects RGBA.
        for px in pixels.chunks_exact_mut(4) {
            px.swap(0, 2);
        }
        image::RgbaImage::from_raw(width, height, pixels)
            .context("Failed to build RgbaImage from staging readback")
    }

    pub(crate) fn chart_display_changed(&mut self) {
        if let Some(comp) = &mut self.direct_composition {
            comp.charts.display_changed();
        }
    }

    pub(crate) fn resize(&mut self, new_size: Size<DevicePixels>) -> Result<()> {
        let width = new_size.width.0.max(1) as u32;
        let height = new_size.height.0.max(1) as u32;
        if self.width == width && self.height == height {
            return Ok(());
        }
        self.atlas.finish_page_updates(false);
        self.release_path_targets();
        self.width = width;
        self.height = height;

        // Clear the render target before resizing
        let devices = self.devices.as_ref().context("devices missing")?;
        unsafe { devices.device_context.OMSetRenderTargets(None, None) };
        let resources = self.resources.as_mut().context("resources missing")?;
        resources.render_target.take();
        resources.render_target_view.take();

        // Resizing the swap chain requires a call to the underlying DXGI adapter, which can return the device removed error.
        // The app might have moved to a monitor that's attached to a different graphics device.
        // When a graphics device is removed or reset, the desktop resolution often changes, resulting in a window size change.
        // But here we just return the error, because we are handling device lost scenarios elsewhere.
        unsafe {
            resources
                .swap_chain
                .ResizeBuffers(
                    BUFFER_COUNT as u32,
                    width,
                    height,
                    RENDER_TARGET_FORMAT,
                    DXGI_SWAP_CHAIN_FLAG(0),
                )
                .context("Failed to resize swap chain")?;
        }

        resources.recreate_resources(devices, width, height)?;

        unsafe {
            devices
                .device_context
                .OMSetRenderTargets(Some(slice::from_ref(&resources.render_target_view)), None);
        }

        Ok(())
    }

    fn upload_scene_buffers(&mut self, scene: &Scene) -> Result<()> {
        let devices = self.devices.as_ref().context("devices missing")?;

        // Only quad and color-sprite bounds can change during retained animation.
        // New scenes and recreated GPU buffers always upload the complete data.
        let reuse_static = scene.paint_revision != 0
            && self.pipelines.uploaded_revision == Some(scene.paint_revision);
        if !reuse_static && !scene.shadows.is_empty() {
            self.pipelines.shadow_pipeline.update_buffer(
                &devices.device,
                &devices.device_context,
                &scene.shadows,
            )?;
        }

        let moving_quads = scene
            .animated_order
            .is_some_and(|order| scene.quads.last().is_some_and(|quad| quad.order >= order));
        if !scene.quads.is_empty() && (!reuse_static || moving_quads) {
            self.pipelines.quad_pipeline.update_buffer(
                &devices.device,
                &devices.device_context,
                &scene.quads,
            )?;
        }

        if !reuse_static && !scene.underlines.is_empty() {
            self.pipelines.underline_pipeline.update_buffer(
                &devices.device,
                &devices.device_context,
                &scene.underlines,
            )?;
        }

        if !reuse_static && !scene.monochrome_sprites.is_empty() {
            self.pipelines.mono_sprites.update_buffer(
                &devices.device,
                &devices.device_context,
                &scene.monochrome_sprites,
            )?;
        }

        if !reuse_static && !scene.subpixel_sprites.is_empty() {
            self.pipelines.subpixel_sprites.update_buffer(
                &devices.device,
                &devices.device_context,
                &scene.subpixel_sprites,
            )?;
        }

        if !scene.polychrome_sprites.is_empty() {
            self.pipelines.poly_sprites.update_buffer(
                &devices.device,
                &devices.device_context,
                &scene.polychrome_sprites,
            )?;
        }

        self.pipelines.uploaded_revision = Some(scene.paint_revision);
        Ok(())
    }

    fn draw_shadows(&mut self, start: usize, len: usize) -> Result<()> {
        if len == 0 {
            return Ok(());
        }
        let devices = self.devices.as_ref().context("devices missing")?;
        self.pipelines.shadow_pipeline.draw_range(
            &devices.device_context,
            &self.globals.batch_params,
            start as u32,
            len as u32,
        )
    }

    fn draw_quads(&mut self, start: usize, len: usize) -> Result<()> {
        if len == 0 {
            return Ok(());
        }
        let devices = self.devices.as_ref().context("devices missing")?;
        self.pipelines.quad_pipeline.draw_range(
            &devices.device_context,
            &self.globals.batch_params,
            start as u32,
            len as u32,
        )
    }

    fn draw_paths_to_intermediate(&mut self, paths: &[Path<ScaledPixels>]) -> Result<()> {
        if paths.is_empty() {
            return Ok(());
        }
        self.work.clears += 1;
        self.work.resolves += 1;
        self.work.resolve_pixels += u64::from(self.width) * u64::from(self.height);
        let devices = self.devices.as_ref().context("devices missing")?;
        let resources = self.resources.as_mut().context("resources missing")?;
        let targets = match &mut resources.paths {
            Some(targets) => targets,
            slot @ None => {
                let targets = PathTargets::new(&devices.device, self.width, self.height)?;
                path_target_trace("create", self.width, self.height);
                slot.insert(targets)
            }
        };
        // Clear intermediate MSAA texture
        unsafe {
            devices
                .device_context
                .ClearRenderTargetView(targets.rtv.as_ref().unwrap(), &[0.0; 4]);
            // Set intermediate MSAA texture as render target
            devices
                .device_context
                .OMSetRenderTargets(Some(slice::from_ref(&targets.rtv)), None);
        }

        // Collect all vertices and sprites for a single draw call
        let mut vertices = Vec::new();

        for path in paths {
            vertices.extend(path.vertices.iter().map(|v| PathRasterizationSprite {
                xy_position: v.xy_position,
                st_position: v.st_position,
                color: path.color,
                bounds: path.clipped_bounds(),
            }));
        }

        self.pipelines.path_rasterization_pipeline.update_buffer(
            &devices.device,
            &devices.device_context,
            &vertices,
        )?;

        self.pipelines.path_rasterization_pipeline.draw(
            &devices.device_context,
            D3D_PRIMITIVE_TOPOLOGY_TRIANGLELIST,
            vertices.len() as u32,
            1,
        )?;

        // Resolve MSAA to non-MSAA intermediate texture
        unsafe {
            devices.device_context.ResolveSubresource(
                &targets.resolved,
                0,
                &targets.msaa,
                0,
                RENDER_TARGET_FORMAT,
            );
            // Restore main render target
            devices
                .device_context
                .OMSetRenderTargets(Some(slice::from_ref(&resources.render_target_view)), None);
        }

        Ok(())
    }

    fn draw_paths_from_intermediate(&mut self, paths: &[Path<ScaledPixels>]) -> Result<()> {
        let Some(first_path) = paths.first() else {
            return Ok(());
        };

        // When copying paths from the intermediate texture to the drawable,
        // each pixel must only be copied once, in case of transparent paths.
        //
        // If all paths have the same draw order, then their bounds are all
        // disjoint, so we can copy each path's bounds individually. If this
        // batch combines different draw orders, we perform a single copy
        // for a minimal spanning rect.
        let sprites = if paths.last().unwrap().order == first_path.order {
            paths
                .iter()
                .map(|path| PathSprite {
                    bounds: path.clipped_bounds(),
                })
                .collect::<Vec<_>>()
        } else {
            let mut bounds = first_path.clipped_bounds();
            for path in paths.iter().skip(1) {
                bounds = bounds.union(&path.clipped_bounds());
            }
            vec![PathSprite { bounds }]
        };

        let devices = self.devices.as_ref().context("devices missing")?;
        let resources = self.resources.as_ref().context("resources missing")?;
        self.pipelines.path_sprite_pipeline.update_buffer(
            &devices.device,
            &devices.device_context,
            &sprites,
        )?;

        // Draw the sprites with the path texture
        self.pipelines.path_sprite_pipeline.draw_with_texture(
            &devices.device_context,
            slice::from_ref(
                &resources
                    .paths
                    .as_ref()
                    .context("path targets missing")?
                    .srv,
            ),
            slice::from_ref(&self.globals.sampler),
            sprites.len() as u32,
        )
    }

    fn draw_underlines(&mut self, start: usize, len: usize) -> Result<()> {
        if len == 0 {
            return Ok(());
        }
        let devices = self.devices.as_ref().context("devices missing")?;
        self.pipelines.underline_pipeline.draw_range(
            &devices.device_context,
            &self.globals.batch_params,
            start as u32,
            len as u32,
        )
    }

    fn draw_monochrome_sprites(
        &mut self,
        texture_id: AtlasTextureId,
        start: usize,
        len: usize,
    ) -> Result<()> {
        if len == 0 {
            return Ok(());
        }
        let devices = self.devices.as_ref().context("devices missing")?;
        let Some(texture_view) = self.atlas.get_texture_view(texture_id) else {
            return Ok(());
        };
        self.pipelines.mono_sprites.draw_range_with_texture(
            &devices.device_context,
            &texture_view,
            &self.globals.batch_params,
            slice::from_ref(&self.globals.sampler),
            start as u32,
            len as u32,
            None,
        )
    }

    fn draw_subpixel_sprites(
        &mut self,
        texture_id: AtlasTextureId,
        start: usize,
        len: usize,
    ) -> Result<()> {
        if len == 0 {
            return Ok(());
        }
        let devices = self.devices.as_ref().context("devices missing")?;
        let Some(texture_view) = self.atlas.get_texture_view(texture_id) else {
            return Ok(());
        };
        self.pipelines.subpixel_sprites.draw_range_with_texture(
            &devices.device_context,
            &texture_view,
            &self.globals.batch_params,
            slice::from_ref(&self.globals.sampler),
            start as u32,
            len as u32,
            None,
        )
    }

    fn draw_polychrome_sprites(
        &mut self,
        texture_id: AtlasTextureId,
        start: usize,
        len: usize,
    ) -> Result<()> {
        if len == 0 {
            return Ok(());
        }
        let devices = self.devices.as_ref().context("devices missing")?;
        let Some(texture_view) = self.atlas.get_texture_view(texture_id) else {
            return Ok(());
        };
        self.pipelines.poly_sprites.draw_range_with_texture(
            &devices.device_context,
            &texture_view,
            &self.globals.batch_params,
            slice::from_ref(&self.globals.sampler),
            start as u32,
            len as u32,
            self.resources
                .as_ref()
                .is_some_and(|r| r.compose_prefix)
                .then_some(&self.globals.replacement_blend),
        )
    }

    fn draw_surfaces(&mut self, surfaces: &[PaintSurface]) -> Result<()> {
        if surfaces.is_empty() {
            return Ok(());
        }
        Ok(())
    }

    pub(crate) fn gpu_specs(&self) -> Result<GpuSpecs> {
        let devices = self.devices.as_ref().context("devices missing")?;
        let desc = unsafe { devices.adapter.GetDesc1() }?;
        let is_software_emulated = (desc.Flags & DXGI_ADAPTER_FLAG_SOFTWARE.0 as u32) != 0;
        let device_name = String::from_utf16_lossy(&desc.Description)
            .trim_matches(char::from(0))
            .to_string();
        let driver_name = match desc.VendorId {
            0x10DE => "NVIDIA Corporation".to_string(),
            0x1002 => "AMD Corporation".to_string(),
            0x8086 => "Intel Corporation".to_string(),
            id => format!("Unknown Vendor (ID: {:#X})", id),
        };
        let driver_version = match desc.VendorId {
            0x10DE => nvidia::get_driver_version(),
            0x1002 => amd::get_driver_version(),
            // For Intel and other vendors, we use the DXGI API to get the driver version.
            _ => dxgi::get_driver_version(&devices.adapter),
        }
        .context("Failed to get gpu driver info")
        .log_err()
        .unwrap_or("Unknown Driver".to_string());
        Ok(GpuSpecs {
            is_software_emulated,
            device_name,
            driver_name,
            driver_info: driver_version,
        })
    }

    pub(crate) fn get_font_info() -> &'static FontInfo {
        static CACHED_FONT_INFO: OnceLock<FontInfo> = OnceLock::new();
        CACHED_FONT_INFO.get_or_init(|| unsafe {
            let factory: IDWriteFactory5 = DWriteCreateFactory(DWRITE_FACTORY_TYPE_SHARED).unwrap();
            let render_params: IDWriteRenderingParams1 =
                factory.CreateRenderingParams().unwrap().cast().unwrap();
            FontInfo {
                gamma_ratios: gpui::get_gamma_correction_ratios(render_params.GetGamma()),
                grayscale_enhanced_contrast: render_params.GetGrayscaleEnhancedContrast(),
                subpixel_enhanced_contrast: render_params.GetEnhancedContrast(),
                is_bgr: render_params.GetPixelGeometry() == DWRITE_PIXEL_GEOMETRY_BGR,
            }
        })
    }

    pub(crate) fn mark_drawable(&mut self) {
        self.skip_draws = false;
    }
}

impl DirectXResources {
    pub fn new(
        devices: &DirectXRendererDevices,
        width: u32,
        height: u32,
        hwnd: HWND,
        hwnd_swap_chain: bool,
    ) -> Result<Self> {
        let swap_chain = if hwnd_swap_chain {
            create_swap_chain(&devices.dxgi_factory, &devices.device, hwnd, width, height)?
        } else {
            create_swap_chain_for_composition(
                &devices.dxgi_factory,
                &devices.device,
                width,
                height,
            )?
        };

        let (render_target, render_target_view, viewport) =
            create_resources(devices, &swap_chain, width, height)?;
        if std::env::var_os("ARGUS_GPUI_DIAGNOSTICS").is_some()
            && let Ok(desc) = unsafe { swap_chain.GetDesc1() }
        {
            println!(
                "{{\"event\":\"gpu-window-surface\",\"hwndSwapChain\":{hwnd_swap_chain},\"format\":{},\"alphaMode\":{}}}",
                desc.Format.0, desc.AlphaMode.0
            );
        }
        let rasterizer = create_rasterizer_state(&devices.device, false)?;
        let prefix_scissor = create_rasterizer_state(&devices.device, true)?;
        unsafe {
            devices.device_context.RSSetState(&rasterizer);
        }

        Ok(Self {
            swap_chain,
            prefix: None,
            prefix_view: None,
            compose_prefix: false,
            prefix_key: None,
            prefix_snapshot: None,
            prefix_scissor,
            rasterizer,
            suffix_damage: None,
            suffix_snapshot: None,
            spare: Vec::new(),
            last: None,
            streak: false,
            frames: Frames::new(),
            present_dirty: None,
            render_target: Some(render_target),
            render_target_view,
            paths: None,
            scroll_copy: None,
            viewport,
        })
    }

    #[inline]
    fn recreate_resources(
        &mut self,
        devices: &DirectXRendererDevices,
        width: u32,
        height: u32,
    ) -> Result<()> {
        self.prefix = None;
        self.prefix_view = None;
        self.compose_prefix = false;
        self.forget();
        self.frames = Frames::new();
        self.present_dirty = None;
        let (render_target, render_target_view, viewport) =
            create_resources(devices, &self.swap_chain, width, height)?;
        self.render_target = Some(render_target);
        self.render_target_view = render_target_view;
        self.paths = None;
        self.scroll_copy = None;
        self.viewport = viewport;
        Ok(())
    }

    /// Nothing drawn before can be trusted: neither the render target's
    /// buffers nor the cached prefix.
    fn forget(&mut self) {
        self.prefix_key = None;
        self.prefix_snapshot = None;
        self.scroll_copy = None;
        self.forget_frames();
    }

    /// The ink of `batches`, in the buffers of a replaced snapshot.
    fn snapshot(
        &mut self,
        scene: &Scene,
        batches: &[PrimitiveBatch],
    ) -> Option<scene_cache::Snapshot> {
        let mut snapshot = self.spare.pop().unwrap_or_default();
        if snapshot.fill(scene, batches) {
            Some(snapshot)
        } else {
            self.spare.push(snapshot);
            None
        }
    }

    fn recycle(&mut self, snapshot: Option<scene_cache::Snapshot>) {
        if let Some(snapshot) = snapshot
            && self.spare.len() < 3
        {
            self.spare.push(snapshot);
        }
    }

    /// What the buffers hold and how the last frame was drawn are unknown;
    /// the cached prefix stays valid.
    fn forget_frames(&mut self) {
        self.suffix_damage = None;
        self.suffix_snapshot = None;
        self.last = None;
        self.streak = false;
        self.frames.forget();
    }
}

impl DirectXRenderPipelines {
    fn stats(&self) -> (u64, u64) {
        let values = [
            self.shadow_pipeline.stats(),
            self.quad_pipeline.stats(),
            self.path_rasterization_pipeline.stats(),
            self.path_sprite_pipeline.stats(),
            self.underline_pipeline.stats(),
            self.mono_sprites.stats(),
            self.subpixel_sprites.stats(),
            self.poly_sprites.stats(),
        ];
        values
            .into_iter()
            .fold((0, 0), |a, b| (a.0 + b.0, a.1 + b.1))
    }
    pub fn new(device: &ID3D11Device) -> Result<Self> {
        let shadow_pipeline = PipelineState::new(
            device,
            "shadow_pipeline",
            ShaderModule::Shadow,
            4,
            create_blend_state(device)?,
        )?;
        let quad_pipeline = PipelineState::new(
            device,
            "quad_pipeline",
            ShaderModule::Quad,
            64,
            create_blend_state(device)?,
        )?;
        let path_rasterization_pipeline = PipelineState::new(
            device,
            "path_rasterization_pipeline",
            ShaderModule::PathRasterization,
            32,
            create_blend_state_for_path_rasterization(device)?,
        )?;
        let path_sprite_pipeline = PipelineState::new(
            device,
            "path_sprite_pipeline",
            ShaderModule::PathSprite,
            4,
            create_blend_state_for_path_sprite(device)?,
        )?;
        let underline_pipeline = PipelineState::new(
            device,
            "underline_pipeline",
            ShaderModule::Underline,
            4,
            create_blend_state(device)?,
        )?;
        let mono_sprites = PipelineState::new(
            device,
            "monochrome_sprite_pipeline",
            ShaderModule::MonochromeSprite,
            512,
            create_blend_state(device)?,
        )?;
        let subpixel_sprites = PipelineState::new(
            device,
            "subpixel_sprite_pipeline",
            ShaderModule::SubpixelSprite,
            512,
            create_blend_state_for_subpixel_rendering(device)?,
        )?;
        let poly_sprites = PipelineState::new(
            device,
            "polychrome_sprite_pipeline",
            ShaderModule::PolychromeSprite,
            16,
            create_over_blend_state(device, D3D11_BLEND_ONE)?,
        )?;

        Ok(Self {
            uploaded_revision: None,
            shadow_pipeline,
            quad_pipeline,
            path_rasterization_pipeline,
            path_sprite_pipeline,
            underline_pipeline,
            mono_sprites,
            subpixel_sprites,
            poly_sprites,
        })
    }
}

impl DirectComposition {
    fn for_window(
        devices: &DirectXRendererDevices,
        hwnd: HWND,
        swap_chain: Option<&IDXGISwapChain1>,
    ) -> Result<Option<Self>> {
        Self::for_window_after(devices, hwnd, swap_chain, None, || {})
    }

    /// As `for_window`, taking a window over from another device.
    /// `before_target` runs once the composition device exists, right before
    /// the window's target is made, and releases the target it replaces.
    /// `previous` is the swap chain that showed the window so far: its frame
    /// covers the window while the compositor first reads `swap_chain`.
    fn for_window_after(
        devices: &DirectXRendererDevices,
        hwnd: HWND,
        swap_chain: Option<&IDXGISwapChain1>,
        previous: Option<&IDXGISwapChain1>,
        before_target: impl FnOnce(),
    ) -> Result<Option<Self>> {
        let Some(dxgi) = &devices.dxgi_device else {
            return Ok(None);
        };
        let mut charts = chart_composition::ChartComposition::new(dxgi, hwnd);
        if swap_chain.is_none() && !charts.enabled() {
            return Ok(None);
        }
        let create = || -> Result<Self> {
            let comp_device = if swap_chain.is_some() && charts.enabled() {
                unsafe { DCompositionCreateDevice3(dxgi) }.or_else(|_| {
                    charts.disable();
                    get_comp_device(dxgi)
                })?
            } else {
                get_comp_device(dxgi)?
            };
            before_target();
            unsafe {
                let comp_target = comp_device.CreateTargetForHwnd(hwnd, false)?;
                let swap_visual = if let Some(chain) = swap_chain {
                    let visual = comp_device.CreateVisual()?;
                    visual.SetContent(chain)?;
                    if let Some(previous) = previous {
                        // The compositor shows nothing of a swap chain for
                        // the first passes after it enters a tree; that of
                        // the software rasterizer stayed empty for two of
                        // them. The old frame covers the window meanwhile,
                        // with one pixel of the new chain over it (a corner
                        // of the window, the same in both frames) for the
                        // compositor to read the chain.
                        let cover = comp_device.CreateVisual()?;
                        let old = comp_device.CreateVisual()?;
                        old.SetContent(previous)?;
                        let first = comp_device.CreateVisual()?;
                        first.SetContent(chain)?;
                        let pixel = comp_device.CreateRectangleClip()?;
                        pixel.SetLeft2(0.)?;
                        pixel.SetTop2(0.)?;
                        pixel.SetRight2(1.)?;
                        pixel.SetBottom2(1.)?;
                        first.SetClip(&pixel)?;
                        cover.AddVisual(&old, false, None)?;
                        cover.AddVisual(&first, true, &old)?;
                        comp_target.SetRoot(&cover)?;
                        comp_device.Commit()?;
                        for _ in 0..TAKEOVER_PASSES {
                            if windows::Win32::Graphics::Dwm::DwmFlush().is_err() {
                                break;
                            }
                        }
                    }
                    comp_target.SetRoot(&visual)?;
                    comp_device.Commit()?;
                    Some(visual)
                } else {
                    // A native swap chain already presents into the HWND.
                    // Attach a visual only while a complete chart tree covers it.
                    None
                };
                Ok(Self {
                    comp_device,
                    comp_target,
                    swap_visual,
                    flat: None,
                    overlay: None,
                    overlay_unavailable: false,
                    charts,
                })
            }
        };
        match create() {
            Ok(comp) => Ok(Some(comp)),
            Err(error) if swap_chain.is_none() => {
                log::warn!("Opaque window chart layers unavailable: {error:#}");
                Ok(None)
            }
            Err(error) => Err(error),
        }
    }
}

impl DirectXGlobalElements {
    pub fn new(device: &ID3D11Device) -> Result<Self> {
        let mut replacement_blend = None;
        let mut replace = D3D11_BLEND_DESC::default();
        replace.RenderTarget[0].SrcBlend = D3D11_BLEND_ONE;
        replace.RenderTarget[0].DestBlend = D3D11_BLEND_ZERO;
        replace.RenderTarget[0].BlendOp = D3D11_BLEND_OP_ADD;
        replace.RenderTarget[0].SrcBlendAlpha = D3D11_BLEND_ONE;
        replace.RenderTarget[0].DestBlendAlpha = D3D11_BLEND_ZERO;
        replace.RenderTarget[0].BlendOpAlpha = D3D11_BLEND_OP_ADD;
        replace.RenderTarget[0].RenderTargetWriteMask = D3D11_COLOR_WRITE_ENABLE_ALL.0 as u8;
        unsafe {
            device.CreateBlendState(&replace, Some(&mut replacement_blend))?;
        }
        let global_params_buffer = create_constant_buffer::<GlobalParams>(device)?;
        let batch_params = BatchConstants::new(device)?;

        let sampler = unsafe {
            let desc = D3D11_SAMPLER_DESC {
                Filter: D3D11_FILTER_MIN_MAG_MIP_LINEAR,
                AddressU: D3D11_TEXTURE_ADDRESS_WRAP,
                AddressV: D3D11_TEXTURE_ADDRESS_WRAP,
                AddressW: D3D11_TEXTURE_ADDRESS_WRAP,
                MipLODBias: 0.0,
                MaxAnisotropy: 1,
                ComparisonFunc: D3D11_COMPARISON_ALWAYS,
                BorderColor: [0.0; 4],
                MinLOD: 0.0,
                MaxLOD: D3D11_FLOAT32_MAX,
            };
            let mut output = None;
            device.CreateSamplerState(&desc, Some(&mut output))?;
            output
        };

        Ok(Self {
            last_params: None,
            replacement_blend: replacement_blend.context("replacement blend state")?,
            global_params_buffer,
            batch_params,
            sampler,
        })
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
#[repr(C)]
struct GlobalParams {
    gamma_ratios: [f32; 4],
    viewport_size: [f32; 2],
    grayscale_enhanced_contrast: f32,
    subpixel_enhanced_contrast: f32,
    is_bgr: u32,
    compose_prefix: u32,
    linear_area_filter: u32,
    _pad: u32,
}

#[derive(Clone, Copy, Debug, Default)]
#[repr(C, align(16))]
struct BatchParams {
    start_index: u32,
    _padding: [u32; 3],
}

const _: () = assert!(std::mem::size_of::<BatchParams>() == 16);

struct PipelineState<T> {
    draw_calls: std::cell::Cell<u64>,
    upload_bytes: std::cell::Cell<u64>,
    label: &'static str,
    vertex: ID3D11VertexShader,
    fragment: ID3D11PixelShader,
    buffer: ID3D11Buffer,
    buffer_size: usize,
    view: Option<ID3D11ShaderResourceView>,
    blend_state: ID3D11BlendState,
    _marker: std::marker::PhantomData<T>,
}

impl<T> PipelineState<T> {
    fn stats(&self) -> (u64, u64) {
        (self.draw_calls.get(), self.upload_bytes.get())
    }
    fn new(
        device: &ID3D11Device,
        label: &'static str,
        shader_module: ShaderModule,
        buffer_size: usize,
        blend_state: ID3D11BlendState,
    ) -> Result<Self> {
        let vertex = {
            let raw_shader = RawShaderBytes::new(shader_module, ShaderTarget::Vertex)?;
            create_vertex_shader(device, raw_shader.as_bytes())?
        };
        let fragment = {
            let raw_shader = RawShaderBytes::new(shader_module, ShaderTarget::Fragment)?;
            create_fragment_shader(device, raw_shader.as_bytes())?
        };
        let buffer = create_buffer(device, std::mem::size_of::<T>(), buffer_size)?;
        let view = create_buffer_view(device, &buffer)?;

        Ok(PipelineState {
            draw_calls: std::cell::Cell::new(0),
            upload_bytes: std::cell::Cell::new(0),
            label,
            vertex,
            fragment,
            buffer,
            buffer_size,
            view,
            blend_state,
            _marker: std::marker::PhantomData,
        })
    }

    fn update_buffer(
        &mut self,
        device: &ID3D11Device,
        device_context: &ID3D11DeviceContext,
        data: &[T],
    ) -> Result<()> {
        if self.buffer_size < data.len() {
            let element_size = std::mem::size_of::<T>();
            let required_size = std::mem::size_of_val(data);
            anyhow::ensure!(
                required_size <= MAX_INSTANCE_BUFFER_SIZE,
                "{} buffer needs {required_size} bytes, above the maximum of {MAX_INSTANCE_BUFFER_SIZE}",
                self.label
            );
            let new_buffer_size = data
                .len()
                .next_power_of_two()
                .min(MAX_INSTANCE_BUFFER_SIZE / element_size);
            log::debug!(
                "Updating {} buffer size from {} to {}",
                self.label,
                self.buffer_size,
                new_buffer_size
            );
            let buffer = create_buffer(device, std::mem::size_of::<T>(), new_buffer_size)?;
            let view = create_buffer_view(device, &buffer)?;
            self.buffer = buffer;
            self.view = view;
            self.buffer_size = new_buffer_size;
        }
        update_buffer(device_context, &self.buffer, data)?;
        self.upload_bytes.set(
            self.upload_bytes
                .get()
                .wrapping_add(std::mem::size_of_val(data) as u64),
        );
        Ok(())
    }

    fn draw(
        &self,
        device_context: &ID3D11DeviceContext,
        topology: D3D_PRIMITIVE_TOPOLOGY,
        vertex_count: u32,
        instance_count: u32,
    ) -> Result<()> {
        set_pipeline_state(
            device_context,
            slice::from_ref(&self.view),
            topology,
            &self.vertex,
            &self.fragment,
            &self.blend_state,
        );
        unsafe {
            self.draw_calls.set(self.draw_calls.get().wrapping_add(1));
            device_context.DrawInstanced(vertex_count, instance_count, 0, 0);
        }
        Ok(())
    }

    fn draw_with_texture(
        &self,
        device_context: &ID3D11DeviceContext,
        texture: &[Option<ID3D11ShaderResourceView>],
        sampler: &[Option<ID3D11SamplerState>],
        instance_count: u32,
    ) -> Result<()> {
        set_pipeline_state(
            device_context,
            slice::from_ref(&self.view),
            D3D_PRIMITIVE_TOPOLOGY_TRIANGLESTRIP,
            &self.vertex,
            &self.fragment,
            &self.blend_state,
        );
        unsafe {
            device_context.PSSetSamplers(0, Some(sampler));
            device_context.VSSetShaderResources(0, Some(texture));
            device_context.PSSetShaderResources(0, Some(texture));

            self.draw_calls.set(self.draw_calls.get().wrapping_add(1));
            device_context.DrawInstanced(4, instance_count, 0, 0);
        }
        Ok(())
    }

    fn draw_range(
        &self,
        device_context: &ID3D11DeviceContext,
        batch_params: &BatchConstants,
        first_instance: u32,
        instance_count: u32,
    ) -> Result<()> {
        anyhow::ensure!(
            first_instance as usize + instance_count as usize <= self.buffer_size,
            "DirectX instance range exceeds the {} buffer",
            self.label
        );
        batch_params.bind(device_context, first_instance)?;
        set_pipeline_state(
            device_context,
            slice::from_ref(&self.view),
            D3D_PRIMITIVE_TOPOLOGY_TRIANGLESTRIP,
            &self.vertex,
            &self.fragment,
            &self.blend_state,
        );
        unsafe {
            self.draw_calls.set(self.draw_calls.get().wrapping_add(1));
            device_context.DrawInstanced(4, instance_count, 0, 0);
        }
        Ok(())
    }

    fn draw_range_with_texture(
        &self,
        device_context: &ID3D11DeviceContext,
        texture: &[Option<ID3D11ShaderResourceView>],
        batch_params: &BatchConstants,
        sampler: &[Option<ID3D11SamplerState>],
        first_instance: u32,
        instance_count: u32,
        blend: Option<&ID3D11BlendState>,
    ) -> Result<()> {
        anyhow::ensure!(
            first_instance as usize + instance_count as usize <= self.buffer_size,
            "DirectX instance range exceeds the {} buffer",
            self.label
        );
        batch_params.bind(device_context, first_instance)?;
        set_pipeline_state(
            device_context,
            slice::from_ref(&self.view),
            D3D_PRIMITIVE_TOPOLOGY_TRIANGLESTRIP,
            &self.vertex,
            &self.fragment,
            blend.unwrap_or(&self.blend_state),
        );
        unsafe {
            device_context.PSSetSamplers(0, Some(sampler));
            device_context.VSSetShaderResources(0, Some(texture));
            device_context.PSSetShaderResources(0, Some(texture));
            self.draw_calls.set(self.draw_calls.get().wrapping_add(1));
            device_context.DrawInstanced(4, instance_count, 0, 0);
        }
        Ok(())
    }
}

#[derive(Clone, Copy)]
#[repr(C)]
struct PathRasterizationSprite {
    xy_position: Point<ScaledPixels>,
    st_position: Point<f32>,
    color: Background,
    bounds: Bounds<ScaledPixels>,
}

#[derive(Clone, Copy)]
#[repr(C)]
struct PathSprite {
    bounds: Bounds<ScaledPixels>,
}

impl Drop for DirectXRenderer {
    fn drop(&mut self) {
        #[cfg(debug_assertions)]
        if let Some(devices) = &self.devices {
            report_live_objects(&devices.device).ok();
        }
    }
}

#[inline]
fn get_comp_device(dxgi_device: &IDXGIDevice) -> Result<IDCompositionDevice> {
    Ok(unsafe { DCompositionCreateDevice(dxgi_device)? })
}

fn create_swap_chain_for_composition(
    dxgi_factory: &IDXGIFactory6,
    device: &ID3D11Device,
    width: u32,
    height: u32,
) -> Result<IDXGISwapChain1> {
    let desc = DXGI_SWAP_CHAIN_DESC1 {
        Width: width,
        Height: height,
        Format: RENDER_TARGET_FORMAT,
        Stereo: false.into(),
        SampleDesc: DXGI_SAMPLE_DESC {
            Count: 1,
            Quality: 0,
        },
        BufferUsage: DXGI_USAGE_RENDER_TARGET_OUTPUT,
        BufferCount: BUFFER_COUNT as u32,
        // Composition SwapChains only support the DXGI_SCALING_STRETCH Scaling.
        Scaling: DXGI_SCALING_STRETCH,
        SwapEffect: DXGI_SWAP_EFFECT_FLIP_SEQUENTIAL,
        AlphaMode: DXGI_ALPHA_MODE_PREMULTIPLIED,
        Flags: 0,
    };
    Ok(unsafe { dxgi_factory.CreateSwapChainForComposition(device, &desc, None)? })
}

fn create_swap_chain(
    dxgi_factory: &IDXGIFactory6,
    device: &ID3D11Device,
    hwnd: HWND,
    width: u32,
    height: u32,
) -> Result<IDXGISwapChain1> {
    use windows::Win32::Graphics::Dxgi::DXGI_MWA_NO_ALT_ENTER;

    let desc = DXGI_SWAP_CHAIN_DESC1 {
        Width: width,
        Height: height,
        Format: RENDER_TARGET_FORMAT,
        Stereo: false.into(),
        SampleDesc: DXGI_SAMPLE_DESC {
            Count: 1,
            Quality: 0,
        },
        BufferUsage: DXGI_USAGE_RENDER_TARGET_OUTPUT,
        BufferCount: BUFFER_COUNT as u32,
        Scaling: DXGI_SCALING_NONE,
        SwapEffect: DXGI_SWAP_EFFECT_FLIP_SEQUENTIAL,
        AlphaMode: DXGI_ALPHA_MODE_IGNORE,
        Flags: 0,
    };
    let swap_chain =
        unsafe { dxgi_factory.CreateSwapChainForHwnd(device, hwnd, &desc, None, None) }?;
    unsafe { dxgi_factory.MakeWindowAssociation(hwnd, DXGI_MWA_NO_ALT_ENTER) }?;
    Ok(swap_chain)
}

#[inline]
fn create_resources(
    devices: &DirectXRendererDevices,
    swap_chain: &IDXGISwapChain1,
    width: u32,
    height: u32,
) -> Result<(
    ID3D11Texture2D,
    Option<ID3D11RenderTargetView>,
    D3D11_VIEWPORT,
)> {
    let (render_target, render_target_view) =
        create_render_target_and_its_view(swap_chain, &devices.device)?;
    let viewport = D3D11_VIEWPORT {
        TopLeftX: 0.0,
        TopLeftY: 0.0,
        Width: width as f32,
        Height: height as f32,
        MinDepth: 0.0,
        MaxDepth: 1.0,
    };
    Ok((render_target, render_target_view, viewport))
}

#[inline]
fn create_render_target_and_its_view(
    swap_chain: &IDXGISwapChain1,
    device: &ID3D11Device,
) -> Result<(ID3D11Texture2D, Option<ID3D11RenderTargetView>)> {
    let render_target: ID3D11Texture2D = unsafe { swap_chain.GetBuffer(0) }?;
    let mut render_target_view = None;
    unsafe { device.CreateRenderTargetView(&render_target, None, Some(&mut render_target_view))? };
    Ok((render_target, render_target_view))
}

#[inline]
fn create_path_intermediate_texture(
    device: &ID3D11Device,
    width: u32,
    height: u32,
) -> Result<(ID3D11Texture2D, Option<ID3D11ShaderResourceView>)> {
    let texture = unsafe {
        let mut output = None;
        let desc = D3D11_TEXTURE2D_DESC {
            Width: width,
            Height: height,
            MipLevels: 1,
            ArraySize: 1,
            Format: RENDER_TARGET_FORMAT,
            SampleDesc: DXGI_SAMPLE_DESC {
                Count: 1,
                Quality: 0,
            },
            Usage: D3D11_USAGE_DEFAULT,
            BindFlags: (D3D11_BIND_RENDER_TARGET.0 | D3D11_BIND_SHADER_RESOURCE.0) as u32,
            CPUAccessFlags: 0,
            MiscFlags: 0,
        };
        device.CreateTexture2D(&desc, None, Some(&mut output))?;
        output.unwrap()
    };

    let mut shader_resource_view = None;
    unsafe { device.CreateShaderResourceView(&texture, None, Some(&mut shader_resource_view))? };

    Ok((texture, Some(shader_resource_view.unwrap())))
}

#[inline]
fn create_path_intermediate_msaa_texture_and_view(
    device: &ID3D11Device,
    width: u32,
    height: u32,
) -> Result<(ID3D11Texture2D, Option<ID3D11RenderTargetView>)> {
    let msaa_texture = unsafe {
        let mut output = None;
        let desc = D3D11_TEXTURE2D_DESC {
            Width: width,
            Height: height,
            MipLevels: 1,
            ArraySize: 1,
            Format: RENDER_TARGET_FORMAT,
            SampleDesc: DXGI_SAMPLE_DESC {
                Count: PATH_MULTISAMPLE_COUNT,
                Quality: D3D11_STANDARD_MULTISAMPLE_PATTERN.0 as u32,
            },
            Usage: D3D11_USAGE_DEFAULT,
            BindFlags: D3D11_BIND_RENDER_TARGET.0 as u32,
            CPUAccessFlags: 0,
            MiscFlags: 0,
        };
        device.CreateTexture2D(&desc, None, Some(&mut output))?;
        output.unwrap()
    };
    let mut msaa_view = None;
    unsafe { device.CreateRenderTargetView(&msaa_texture, None, Some(&mut msaa_view))? };
    Ok((msaa_texture, Some(msaa_view.unwrap())))
}

#[inline]
fn create_rasterizer_state(device: &ID3D11Device, scissor: bool) -> Result<ID3D11RasterizerState> {
    let desc = D3D11_RASTERIZER_DESC {
        FillMode: D3D11_FILL_SOLID,
        CullMode: D3D11_CULL_NONE,
        FrontCounterClockwise: false.into(),
        DepthBias: 0,
        DepthBiasClamp: 0.0,
        SlopeScaledDepthBias: 0.0,
        DepthClipEnable: true.into(),
        ScissorEnable: scissor.into(),
        MultisampleEnable: true.into(),
        AntialiasedLineEnable: false.into(),
    };
    let rasterizer_state = unsafe {
        let mut state = None;
        device.CreateRasterizerState(&desc, Some(&mut state))?;
        state.unwrap()
    };
    Ok(rasterizer_state)
}

// https://learn.microsoft.com/en-us/windows/win32/api/d3d11/ns-d3d11-d3d11_blend_desc
#[inline]
fn create_blend_state(device: &ID3D11Device) -> Result<ID3D11BlendState> {
    create_over_blend_state(device, D3D11_BLEND_SRC_ALPHA)
}

fn create_over_blend_state(device: &ID3D11Device, source: D3D11_BLEND) -> Result<ID3D11BlendState> {
    let mut desc = D3D11_BLEND_DESC::default();
    desc.RenderTarget[0].BlendEnable = true.into();
    desc.RenderTarget[0].BlendOp = D3D11_BLEND_OP_ADD;
    desc.RenderTarget[0].BlendOpAlpha = D3D11_BLEND_OP_ADD;
    desc.RenderTarget[0].SrcBlend = source;
    desc.RenderTarget[0].SrcBlendAlpha = D3D11_BLEND_ONE;
    desc.RenderTarget[0].DestBlend = D3D11_BLEND_INV_SRC_ALPHA;
    desc.RenderTarget[0].DestBlendAlpha = D3D11_BLEND_INV_SRC_ALPHA;
    desc.RenderTarget[0].RenderTargetWriteMask = D3D11_COLOR_WRITE_ENABLE_ALL.0 as u8;
    unsafe {
        let mut state = None;
        device.CreateBlendState(&desc, Some(&mut state))?;
        Ok(state.unwrap())
    }
}

#[inline]
fn create_blend_state_for_subpixel_rendering(device: &ID3D11Device) -> Result<ID3D11BlendState> {
    let mut desc = D3D11_BLEND_DESC::default();
    desc.RenderTarget[0].BlendEnable = true.into();
    desc.RenderTarget[0].BlendOp = D3D11_BLEND_OP_ADD;
    desc.RenderTarget[0].BlendOpAlpha = D3D11_BLEND_OP_ADD;
    desc.RenderTarget[0].SrcBlend = D3D11_BLEND_SRC1_COLOR;
    desc.RenderTarget[0].DestBlend = D3D11_BLEND_INV_SRC1_COLOR;
    // It does not make sense to draw transparent subpixel-rendered text, since it cannot be meaningfully alpha-blended onto anything else.
    desc.RenderTarget[0].SrcBlendAlpha = D3D11_BLEND_ONE;
    desc.RenderTarget[0].DestBlendAlpha = D3D11_BLEND_ZERO;
    desc.RenderTarget[0].RenderTargetWriteMask =
        D3D11_COLOR_WRITE_ENABLE_ALL.0 as u8 & !D3D11_COLOR_WRITE_ENABLE_ALPHA.0 as u8;

    unsafe {
        let mut state = None;
        device.CreateBlendState(&desc, Some(&mut state))?;
        Ok(state.unwrap())
    }
}

#[inline]
fn create_blend_state_for_path_rasterization(device: &ID3D11Device) -> Result<ID3D11BlendState> {
    // If the feature level is set to greater than D3D_FEATURE_LEVEL_9_3, the display
    // device performs the blend in linear space, which is ideal.
    let mut desc = D3D11_BLEND_DESC::default();
    desc.RenderTarget[0].BlendEnable = true.into();
    desc.RenderTarget[0].BlendOp = D3D11_BLEND_OP_ADD;
    desc.RenderTarget[0].BlendOpAlpha = D3D11_BLEND_OP_ADD;
    desc.RenderTarget[0].SrcBlend = D3D11_BLEND_ONE;
    desc.RenderTarget[0].SrcBlendAlpha = D3D11_BLEND_ONE;
    desc.RenderTarget[0].DestBlend = D3D11_BLEND_INV_SRC_ALPHA;
    desc.RenderTarget[0].DestBlendAlpha = D3D11_BLEND_INV_SRC_ALPHA;
    desc.RenderTarget[0].RenderTargetWriteMask = D3D11_COLOR_WRITE_ENABLE_ALL.0 as u8;
    unsafe {
        let mut state = None;
        device.CreateBlendState(&desc, Some(&mut state))?;
        Ok(state.unwrap())
    }
}

#[inline]
fn create_blend_state_for_path_sprite(device: &ID3D11Device) -> Result<ID3D11BlendState> {
    // If the feature level is set to greater than D3D_FEATURE_LEVEL_9_3, the display
    // device performs the blend in linear space, which is ideal.
    let mut desc = D3D11_BLEND_DESC::default();
    desc.RenderTarget[0].BlendEnable = true.into();
    desc.RenderTarget[0].BlendOp = D3D11_BLEND_OP_ADD;
    desc.RenderTarget[0].BlendOpAlpha = D3D11_BLEND_OP_ADD;
    desc.RenderTarget[0].SrcBlend = D3D11_BLEND_ONE;
    desc.RenderTarget[0].SrcBlendAlpha = D3D11_BLEND_ONE;
    desc.RenderTarget[0].DestBlend = D3D11_BLEND_INV_SRC_ALPHA;
    desc.RenderTarget[0].DestBlendAlpha = D3D11_BLEND_INV_SRC_ALPHA;
    desc.RenderTarget[0].RenderTargetWriteMask = D3D11_COLOR_WRITE_ENABLE_ALL.0 as u8;
    unsafe {
        let mut state = None;
        device.CreateBlendState(&desc, Some(&mut state))?;
        Ok(state.unwrap())
    }
}

#[inline]
fn create_vertex_shader(device: &ID3D11Device, bytes: &[u8]) -> Result<ID3D11VertexShader> {
    unsafe {
        let mut shader = None;
        device.CreateVertexShader(bytes, None, Some(&mut shader))?;
        Ok(shader.unwrap())
    }
}

#[inline]
fn create_fragment_shader(device: &ID3D11Device, bytes: &[u8]) -> Result<ID3D11PixelShader> {
    unsafe {
        let mut shader = None;
        device.CreatePixelShader(bytes, None, Some(&mut shader))?;
        Ok(shader.unwrap())
    }
}

#[inline]
fn create_constant_buffer<T>(device: &ID3D11Device) -> Result<Option<ID3D11Buffer>> {
    const { assert!(std::mem::size_of::<T>() != 0 && std::mem::size_of::<T>().is_multiple_of(16)) };
    let desc = D3D11_BUFFER_DESC {
        ByteWidth: std::mem::size_of::<T>() as u32,
        Usage: D3D11_USAGE_DYNAMIC,
        BindFlags: D3D11_BIND_CONSTANT_BUFFER.0 as u32,
        CPUAccessFlags: D3D11_CPU_ACCESS_WRITE.0 as u32,
        MiscFlags: 0,
        StructureByteStride: 0,
    };
    let mut buffer = None;
    unsafe { device.CreateBuffer(&desc, None, Some(&mut buffer)) }?;
    Ok(buffer)
}

#[inline]
fn create_buffer(
    device: &ID3D11Device,
    element_size: usize,
    buffer_size: usize,
) -> Result<ID3D11Buffer> {
    let desc = D3D11_BUFFER_DESC {
        ByteWidth: (element_size * buffer_size) as u32,
        Usage: D3D11_USAGE_DYNAMIC,
        BindFlags: D3D11_BIND_SHADER_RESOURCE.0 as u32,
        CPUAccessFlags: D3D11_CPU_ACCESS_WRITE.0 as u32,
        MiscFlags: D3D11_RESOURCE_MISC_BUFFER_STRUCTURED.0 as u32,
        StructureByteStride: element_size as u32,
    };
    let mut buffer = None;
    unsafe { device.CreateBuffer(&desc, None, Some(&mut buffer)) }?;
    Ok(buffer.unwrap())
}

#[inline]
fn create_buffer_view(
    device: &ID3D11Device,
    buffer: &ID3D11Buffer,
) -> Result<Option<ID3D11ShaderResourceView>> {
    let mut view = None;
    unsafe { device.CreateShaderResourceView(buffer, None, Some(&mut view)) }?;
    Ok(view)
}

#[inline]
fn update_buffer<T>(
    device_context: &ID3D11DeviceContext,
    buffer: &ID3D11Buffer,
    data: &[T],
) -> Result<()> {
    unsafe {
        let mut dest = std::mem::zeroed();
        device_context.Map(buffer, 0, D3D11_MAP_WRITE_DISCARD, 0, Some(&mut dest))?;
        std::ptr::copy_nonoverlapping(data.as_ptr(), dest.pData as _, data.len());
        device_context.Unmap(buffer, 0);
    }
    Ok(())
}

#[inline]
fn update_batch_start(
    device_context: &ID3D11DeviceContext,
    buffer: &ID3D11Buffer,
    first_instance: u32,
) -> Result<()> {
    update_buffer(
        device_context,
        buffer,
        &[BatchParams {
            start_index: first_instance,
            _padding: [0; 3],
        }],
    )
}

#[inline]
fn set_pipeline_state(
    device_context: &ID3D11DeviceContext,
    buffer_view: &[Option<ID3D11ShaderResourceView>],
    topology: D3D_PRIMITIVE_TOPOLOGY,
    vertex_shader: &ID3D11VertexShader,
    fragment_shader: &ID3D11PixelShader,
    blend_state: &ID3D11BlendState,
) {
    unsafe {
        device_context.VSSetShaderResources(1, Some(buffer_view));
        device_context.PSSetShaderResources(1, Some(buffer_view));
        device_context.IASetPrimitiveTopology(topology);
        device_context.VSSetShader(vertex_shader, None);
        device_context.PSSetShader(fragment_shader, None);
        device_context.OMSetBlendState(blend_state, None, 0xFFFFFFFF);
    }
}

#[cfg(debug_assertions)]
fn report_live_objects(device: &ID3D11Device) -> Result<()> {
    let debug_device: ID3D11Debug = device.cast()?;
    unsafe {
        debug_device.ReportLiveDeviceObjects(D3D11_RLDO_DETAIL)?;
    }
    Ok(())
}

const BUFFER_COUNT: usize = 3;

pub(crate) mod shader_resources {
    use anyhow::Result;

    #[cfg(debug_assertions)]
    use windows::{
        Win32::Graphics::Direct3D::{
            Fxc::{D3DCOMPILE_DEBUG, D3DCOMPILE_OPTIMIZATION_LEVEL3, D3DCompileFromFile},
            ID3DBlob,
        },
        core::{HSTRING, PCSTR},
    };

    #[derive(Copy, Clone, Debug, Eq, PartialEq)]
    pub(crate) enum ShaderModule {
        Quad,
        Shadow,
        Underline,
        PathRasterization,
        PathSprite,
        MonochromeSprite,
        SubpixelSprite,
        PolychromeSprite,
        ChartFilter,
        ChartBackdrop,
    }

    #[derive(Copy, Clone, Debug, Eq, PartialEq)]
    pub(crate) enum ShaderTarget {
        Vertex,
        Fragment,
    }

    pub(crate) struct RawShaderBytes<'t> {
        inner: &'t [u8],

        #[cfg(debug_assertions)]
        _blob: ID3DBlob,
    }

    impl<'t> RawShaderBytes<'t> {
        pub(crate) fn new(module: ShaderModule, target: ShaderTarget) -> Result<Self> {
            #[cfg(not(debug_assertions))]
            {
                Ok(Self::from_bytes(module, target))
            }
            #[cfg(debug_assertions)]
            {
                let blob = build_shader_blob(module, target)?;
                let inner = unsafe {
                    std::slice::from_raw_parts(
                        blob.GetBufferPointer() as *const u8,
                        blob.GetBufferSize(),
                    )
                };
                Ok(Self { inner, _blob: blob })
            }
        }

        pub(crate) fn as_bytes(&'t self) -> &'t [u8] {
            self.inner
        }

        #[cfg(not(debug_assertions))]
        fn from_bytes(module: ShaderModule, target: ShaderTarget) -> Self {
            let bytes = match module {
                ShaderModule::Quad => match target {
                    ShaderTarget::Vertex => QUAD_VERTEX_BYTES,
                    ShaderTarget::Fragment => QUAD_FRAGMENT_BYTES,
                },
                ShaderModule::Shadow => match target {
                    ShaderTarget::Vertex => SHADOW_VERTEX_BYTES,
                    ShaderTarget::Fragment => SHADOW_FRAGMENT_BYTES,
                },
                ShaderModule::Underline => match target {
                    ShaderTarget::Vertex => UNDERLINE_VERTEX_BYTES,
                    ShaderTarget::Fragment => UNDERLINE_FRAGMENT_BYTES,
                },
                ShaderModule::PathRasterization => match target {
                    ShaderTarget::Vertex => PATH_RASTERIZATION_VERTEX_BYTES,
                    ShaderTarget::Fragment => PATH_RASTERIZATION_FRAGMENT_BYTES,
                },
                ShaderModule::PathSprite => match target {
                    ShaderTarget::Vertex => PATH_SPRITE_VERTEX_BYTES,
                    ShaderTarget::Fragment => PATH_SPRITE_FRAGMENT_BYTES,
                },
                ShaderModule::MonochromeSprite => match target {
                    ShaderTarget::Vertex => MONOCHROME_SPRITE_VERTEX_BYTES,
                    ShaderTarget::Fragment => MONOCHROME_SPRITE_FRAGMENT_BYTES,
                },
                ShaderModule::SubpixelSprite => match target {
                    ShaderTarget::Vertex => SUBPIXEL_SPRITE_VERTEX_BYTES,
                    ShaderTarget::Fragment => SUBPIXEL_SPRITE_FRAGMENT_BYTES,
                },
                ShaderModule::ChartFilter => match target {
                    ShaderTarget::Vertex => CHART_FILTER_VERTEX_BYTES,
                    ShaderTarget::Fragment => CHART_FILTER_FRAGMENT_BYTES,
                },
                ShaderModule::ChartBackdrop => match target {
                    ShaderTarget::Vertex => CHART_BACKDROP_VERTEX_BYTES,
                    ShaderTarget::Fragment => CHART_BACKDROP_FRAGMENT_BYTES,
                },
                ShaderModule::PolychromeSprite => match target {
                    ShaderTarget::Vertex => POLYCHROME_SPRITE_VERTEX_BYTES,
                    ShaderTarget::Fragment => POLYCHROME_SPRITE_FRAGMENT_BYTES,
                },
            };
            Self { inner: bytes }
        }
    }

    #[cfg(debug_assertions)]
    pub(super) fn build_shader_blob(entry: ShaderModule, target: ShaderTarget) -> Result<ID3DBlob> {
        unsafe {
            use windows::Win32::Graphics::{
                Direct3D::ID3DInclude, Hlsl::D3D_COMPILE_STANDARD_FILE_INCLUDE,
            };

            let entry = format!(
                "{}_{}\0",
                entry.as_str(),
                match target {
                    ShaderTarget::Vertex => "vertex",
                    ShaderTarget::Fragment => "fragment",
                }
            );
            let target = match target {
                ShaderTarget::Vertex => "vs_4_1\0",
                ShaderTarget::Fragment => "ps_4_1\0",
            };

            let mut compile_blob = None;
            let mut error_blob = None;
            let shader_path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("src/shaders.hlsl")
                .canonicalize()?;

            let entry_point = PCSTR::from_raw(entry.as_ptr());
            let target_cstr = PCSTR::from_raw(target.as_ptr());

            // really dirty trick because winapi bindings are unhappy otherwise
            let include_handler = &std::mem::transmute::<usize, ID3DInclude>(
                D3D_COMPILE_STANDARD_FILE_INCLUDE as usize,
            );

            let ret = D3DCompileFromFile(
                &HSTRING::from(shader_path.to_str().unwrap()),
                None,
                include_handler,
                entry_point,
                target_cstr,
                D3DCOMPILE_DEBUG | D3DCOMPILE_OPTIMIZATION_LEVEL3,
                0,
                &mut compile_blob,
                Some(&mut error_blob),
            );
            if ret.is_err() {
                let Some(error_blob) = error_blob else {
                    return Err(anyhow::anyhow!("{ret:?}"));
                };

                let error_string =
                    std::ffi::CStr::from_ptr(error_blob.GetBufferPointer() as *const i8)
                        .to_string_lossy();
                log::error!("Shader compile error: {}", error_string);
                return Err(anyhow::anyhow!("Compile error: {}", error_string));
            }
            Ok(compile_blob.unwrap())
        }
    }

    #[cfg(not(debug_assertions))]
    include!(concat!(env!("OUT_DIR"), "/shaders_bytes.rs"));

    #[cfg(debug_assertions)]
    impl ShaderModule {
        pub fn as_str(self) -> &'static str {
            match self {
                ShaderModule::Quad => "quad",
                ShaderModule::Shadow => "shadow",
                ShaderModule::Underline => "underline",
                ShaderModule::PathRasterization => "path_rasterization",
                ShaderModule::PathSprite => "path_sprite",
                ShaderModule::MonochromeSprite => "monochrome_sprite",
                ShaderModule::SubpixelSprite => "subpixel_sprite",
                ShaderModule::PolychromeSprite => "polychrome_sprite",
                ShaderModule::ChartFilter => "chart_filter",
                ShaderModule::ChartBackdrop => "chart_backdrop",
            }
        }
    }
}

mod nvidia {
    use std::{
        ffi::CStr,
        os::raw::{c_char, c_int, c_uint},
    };

    use anyhow::Result;
    use windows::{Win32::System::LibraryLoader::GetProcAddress, core::s};

    use crate::with_dll_library;

    // https://github.com/NVIDIA/nvapi/blob/7cb76fce2f52de818b3da497af646af1ec16ce27/nvapi_lite_common.h#L180
    const NVAPI_SHORT_STRING_MAX: usize = 64;

    // https://github.com/NVIDIA/nvapi/blob/7cb76fce2f52de818b3da497af646af1ec16ce27/nvapi_lite_common.h#L235
    #[allow(non_camel_case_types)]
    type NvAPI_ShortString = [c_char; NVAPI_SHORT_STRING_MAX];

    // https://github.com/NVIDIA/nvapi/blob/7cb76fce2f52de818b3da497af646af1ec16ce27/nvapi_lite_common.h#L447
    #[allow(non_camel_case_types)]
    type NvAPI_SYS_GetDriverAndBranchVersion_t = unsafe extern "C" fn(
        driver_version: *mut c_uint,
        build_branch_string: *mut NvAPI_ShortString,
    ) -> c_int;

    pub(super) fn get_driver_version() -> Result<String> {
        #[cfg(target_pointer_width = "64")]
        let nvidia_dll_name = s!("nvapi64.dll");
        #[cfg(target_pointer_width = "32")]
        let nvidia_dll_name = s!("nvapi.dll");

        with_dll_library(nvidia_dll_name, |nvidia_dll| unsafe {
            let nvapi_query_addr = GetProcAddress(nvidia_dll, s!("nvapi_QueryInterface"))
                .ok_or_else(|| anyhow::anyhow!("Failed to get nvapi_QueryInterface address"))?;
            let nvapi_query: extern "C" fn(u32) -> *mut () = std::mem::transmute(nvapi_query_addr);

            // https://github.com/NVIDIA/nvapi/blob/7cb76fce2f52de818b3da497af646af1ec16ce27/nvapi_interface.h#L41
            let nvapi_get_driver_version_ptr = nvapi_query(0x2926aaad);
            if nvapi_get_driver_version_ptr.is_null() {
                anyhow::bail!("Failed to get NVIDIA driver version function pointer");
            }
            let nvapi_get_driver_version: NvAPI_SYS_GetDriverAndBranchVersion_t =
                std::mem::transmute(nvapi_get_driver_version_ptr);

            let mut driver_version: c_uint = 0;
            let mut build_branch_string: NvAPI_ShortString = [0; NVAPI_SHORT_STRING_MAX];
            let result = nvapi_get_driver_version(
                &mut driver_version as *mut c_uint,
                &mut build_branch_string as *mut NvAPI_ShortString,
            );

            if result != 0 {
                anyhow::bail!(
                    "Failed to get NVIDIA driver version, error code: {}",
                    result
                );
            }
            let major = driver_version / 100;
            let minor = driver_version % 100;
            let branch_string = CStr::from_ptr(build_branch_string.as_ptr());
            Ok(format!(
                "{}.{} {}",
                major,
                minor,
                branch_string.to_string_lossy()
            ))
        })
    }
}

mod amd {
    use std::os::raw::{c_char, c_int, c_void};

    use anyhow::Result;
    use windows::{Win32::System::LibraryLoader::GetProcAddress, core::s};

    use crate::with_dll_library;

    // https://github.com/GPUOpen-LibrariesAndSDKs/AGS_SDK/blob/5d8812d703d0335741b6f7ffc37838eeb8b967f7/ags_lib/inc/amd_ags.h#L145
    const AGS_CURRENT_VERSION: i32 = (6 << 22) | (3 << 12);

    // https://github.com/GPUOpen-LibrariesAndSDKs/AGS_SDK/blob/5d8812d703d0335741b6f7ffc37838eeb8b967f7/ags_lib/inc/amd_ags.h#L204
    // This is an opaque type, using struct to represent it properly for FFI
    #[repr(C)]
    struct AGSContext {
        _private: [u8; 0],
    }

    #[repr(C)]
    pub struct AGSGPUInfo {
        pub driver_version: *const c_char,
        pub radeon_software_version: *const c_char,
        pub num_devices: c_int,
        pub devices: *mut c_void,
    }

    // https://github.com/GPUOpen-LibrariesAndSDKs/AGS_SDK/blob/5d8812d703d0335741b6f7ffc37838eeb8b967f7/ags_lib/inc/amd_ags.h#L429
    #[allow(non_camel_case_types)]
    type agsInitialize_t = unsafe extern "C" fn(
        version: c_int,
        config: *const c_void,
        context: *mut *mut AGSContext,
        gpu_info: *mut AGSGPUInfo,
    ) -> c_int;

    // https://github.com/GPUOpen-LibrariesAndSDKs/AGS_SDK/blob/5d8812d703d0335741b6f7ffc37838eeb8b967f7/ags_lib/inc/amd_ags.h#L436
    #[allow(non_camel_case_types)]
    type agsDeInitialize_t = unsafe extern "C" fn(context: *mut AGSContext) -> c_int;

    pub(super) fn get_driver_version() -> Result<String> {
        #[cfg(target_pointer_width = "64")]
        let amd_dll_name = s!("amd_ags_x64.dll");
        #[cfg(target_pointer_width = "32")]
        let amd_dll_name = s!("amd_ags_x86.dll");

        with_dll_library(amd_dll_name, |amd_dll| unsafe {
            let ags_initialize_addr = GetProcAddress(amd_dll, s!("agsInitialize"))
                .ok_or_else(|| anyhow::anyhow!("Failed to get agsInitialize address"))?;
            let ags_deinitialize_addr = GetProcAddress(amd_dll, s!("agsDeInitialize"))
                .ok_or_else(|| anyhow::anyhow!("Failed to get agsDeInitialize address"))?;

            let ags_initialize: agsInitialize_t = std::mem::transmute(ags_initialize_addr);
            let ags_deinitialize: agsDeInitialize_t = std::mem::transmute(ags_deinitialize_addr);

            let mut context: *mut AGSContext = std::ptr::null_mut();
            let mut gpu_info: AGSGPUInfo = AGSGPUInfo {
                driver_version: std::ptr::null(),
                radeon_software_version: std::ptr::null(),
                num_devices: 0,
                devices: std::ptr::null_mut(),
            };

            let result = ags_initialize(
                AGS_CURRENT_VERSION,
                std::ptr::null(),
                &mut context,
                &mut gpu_info,
            );
            if result != 0 {
                anyhow::bail!("Failed to initialize AMD AGS, error code: {}", result);
            }

            // Vulkan actually returns this as the driver version
            let software_version = if !gpu_info.radeon_software_version.is_null() {
                std::ffi::CStr::from_ptr(gpu_info.radeon_software_version)
                    .to_string_lossy()
                    .into_owned()
            } else {
                "Unknown Radeon Software Version".to_string()
            };

            let driver_version = if !gpu_info.driver_version.is_null() {
                std::ffi::CStr::from_ptr(gpu_info.driver_version)
                    .to_string_lossy()
                    .into_owned()
            } else {
                "Unknown Radeon Driver Version".to_string()
            };

            ags_deinitialize(context);
            Ok(format!("{} ({})", software_version, driver_version))
        })
    }
}

mod dxgi {
    use windows::{
        Win32::Graphics::Dxgi::{IDXGIAdapter1, IDXGIDevice},
        core::Interface,
    };

    pub(super) fn get_driver_version(adapter: &IDXGIAdapter1) -> anyhow::Result<String> {
        let number = unsafe { adapter.CheckInterfaceSupport(&IDXGIDevice::IID as _) }?;
        Ok(format!(
            "{}.{}.{}.{}",
            number >> 48,
            (number >> 32) & 0xFFFF,
            (number >> 16) & 0xFFFF,
            number & 0xFFFF
        ))
    }
}
