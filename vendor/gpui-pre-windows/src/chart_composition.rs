//! Retain chart bitmaps in the compositor between complete GPUI paints.
use super::*;
use windows::Wdk::Graphics::Direct3D::{
    D3DKMT_CLOSEADAPTER, D3DKMT_OPENADAPTERFROMLUID, D3DKMT_QUERYADAPTERINFO, D3DKMT_WDDM_2_7_CAPS,
    D3DKMTCloseAdapter, D3DKMTOpenAdapterFromLuid, D3DKMTQueryAdapterInfo,
    KMTQAITYPE_WDDM_2_7_CAPS,
};
use windows::Win32::Foundation::POINT;
use windows::Win32::Graphics::Gdi::{MONITOR_DEFAULTTONEAREST, MonitorFromWindow};
use windows_numerics::Matrix3x2;
#[path = "chart_backdrop.rs"]
mod backdrop;
use backdrop::Strip;

#[derive(Clone, Copy, PartialEq, Eq)]
struct FrameKey {
    revision: u64,
    order: DrawOrder,
    size: (u32, u32),
    opaque: bool,
}
const BAR_PHASES: u32 = 128;
const MAX_BAR_BYTES: u64 = 48 * 1024 * 1024;
const MAX_TREE_BYTES: u64 = 256 * 1024 * 1024;

#[derive(Default)]
pub(super) struct ChartComposition {
    tree: Option<Tree>,
    output: Option<ChartOutput>,
    skipped: Option<FrameKey>,
    unavailable: bool,
    standard_sdr: Option<bool>,
    tree_budget: u64,
    used_since_hide: bool,
    filter: Option<Filter>,
    backdrop_scan: Option<backdrop::Scan>,
    bars: Vec<(BarKey, IDCompositionSurface)>,
}

// Query the active scheduler. A saved registry setting may still await reboot.
fn software_scheduled(device: &IDXGIDevice) -> bool {
    unsafe {
        let Ok(desc) = device.GetAdapter().and_then(|a| a.GetDesc()) else {
            return false;
        };
        let mut adapter = D3DKMT_OPENADAPTERFROMLUID {
            AdapterLuid: desc.AdapterLuid,
            hAdapter: 0,
        };
        if D3DKMTOpenAdapterFromLuid(&mut adapter).is_err() {
            return false;
        }
        let mut caps = D3DKMT_WDDM_2_7_CAPS::default();
        let status = D3DKMTQueryAdapterInfo(&mut D3DKMT_QUERYADAPTERINFO {
            hAdapter: adapter.hAdapter,
            Type: KMTQAITYPE_WDDM_2_7_CAPS,
            pPrivateDriverData: (&raw mut caps).cast(),
            PrivateDriverDataSize: std::mem::size_of_val(&caps) as u32,
        });
        let _ = D3DKMTCloseAdapter(&D3DKMT_CLOSEADAPTER {
            hAdapter: adapter.hAdapter,
        });
        // HwSchSupported is bit zero, HwSchEnabled is bit one.
        let test_layers = std::env::var_os("ARGUS_GPUI_TEST_CHART_LAYERS").is_some()
            && std::env::args_os().any(|arg| arg == "--test");
        status.is_ok()
            && caps.Anonymous.Value & 1 != 0
            && (caps.Anonymous.Value & 2 == 0 || test_layers)
    }
}

fn standard_sdr_window(hwnd: HWND) -> bool {
    unsafe {
        let monitor = MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST);
        // A fresh factory observes advanced-color changes on the same monitor.
        let Ok(factory) = CreateDXGIFactory1::<IDXGIFactory1>() else {
            return false;
        };
        let mut adapter_index = 0;
        while let Ok(adapter) = factory.EnumAdapters1(adapter_index) {
            let mut output_index = 0;
            while let Ok(output) = adapter.EnumOutputs(output_index) {
                if let Ok(desc) = output.cast::<IDXGIOutput6>().and_then(|o| o.GetDesc1())
                    && desc.Monitor == monitor
                    && desc.AttachedToDesktop.as_bool()
                {
                    return desc.ColorSpace == DXGI_COLOR_SPACE_RGB_FULL_G22_NONE_P709;
                }
                output_index += 1;
            }
            adapter_index += 1;
        }
        false
    }
}

struct Tree {
    key: FrameKey,
    layers: Vec<Layer>,
}

/// The backing bitmap and root stay attached while a marked chart alternates
/// between a complete paint and compositor-driven motion.
#[derive(Clone)]
struct ChartOutput {
    size: (u32, u32),
    root: IDCompositionVisual,
    bitmap: IDCompositionSurface,
}

struct Layer {
    visual: IDCompositionVisual,
    content: LayerContent,
    position: Option<(f32, f32)>,
}

enum LayerContent {
    Image {
        index: usize,
        y_phase: f32,
        first_row: u32,
        strip: Option<Strip>,
    },
    Bar {
        index: usize,
        key: BarKey,
        left: f32,
        bottom: f32,
    },
}

#[derive(Clone, Copy, PartialEq)]
struct BarKey {
    width: u32,
    height: u32,
    color: Hsla,
    corners: Corners<ScaledPixels>,
}

impl BarKey {
    fn new(bar: RetainedBar, quad: &Quad) -> Option<Self> {
        let width = bar.bounds.size.width.0;
        let height = bar.bounds.bottom().0 - bar.ceiling.0.floor();
        if !(1. ..=64.).contains(&width)
            || !(1. ..=120.).contains(&height)
            || bar.bounds.top().0 < bar.ceiling.0.floor() - 1e-3
        {
            return None;
        }
        let key = Self {
            width: width as u32,
            height: height.round() as u32,
            color: bar.color,
            corners: quad.corner_radii,
        };
        (key.bytes() <= MAX_BAR_BYTES).then_some(key)
    }

    fn bytes(self) -> u64 {
        4 * u64::from(self.width * BAR_PHASES) * u64::from((self.height + 1) * self.height)
    }
}

fn key(
    scene: &Scene,
    width: u32,
    height: u32,
    background: WindowBackgroundAppearance,
) -> Option<FrameKey> {
    let order = scene.animated_order?;
    Some(FrameKey {
        revision: scene.paint_revision,
        order,
        size: (width, height),
        opaque: matches!(background, WindowBackgroundAppearance::Opaque),
    })
}

fn clip(bounds: Bounds<ScaledPixels>, width: u32, height: u32) -> RECT {
    // GPUI's clip distances select pixels by their centers.
    let pixel = |v: ScaledPixels, max: u32| (v.0 - 0.5).ceil().clamp(0., max as f32) as i32;
    RECT {
        left: pixel(bounds.left(), width),
        top: pixel(bounds.top(), height),
        right: pixel(bounds.right(), width),
        bottom: pixel(bounds.bottom(), height),
    }
}

fn supported_image(s: &PolychromeSprite) -> bool {
    matches!(
        s.pad,
        PolychromeSprite::PREMULTIPLIED_AREA_FILTER | PolychromeSprite::IN_SCENE_AREA_FILTER
    ) && s.tile.bounds.size.width.0 > 0
        && s.tile.bounds.size.width.0 < 8192
        && (s.bounds.bottom().0 - 0.5).ceil() > (s.bounds.top().0 - 0.5).ceil()
        && s.opacity == 1.
        && s.grayscale == false.into()
        && s.corner_radii == Corners::default()
        && (s.bounds.size.width.0 * 2. - s.tile.bounds.size.width.0 as f32).abs() < 0.01
        && (s.bounds.size.height.0 * 2. - s.tile.bounds.size.height.0 as f32).abs() < 0.01
}

// The first sampled pixel changes at a half-pixel boundary, including when
// two origins have almost identical fractional parts on opposite sides.
fn row_phase(top: f32) -> f32 {
    (top - 0.5).ceil() + 0.5 - top
}

impl Layer {
    fn compatible(&self, scene: &Scene) -> bool {
        match self.content {
            LayerContent::Image {
                index,
                y_phase,
                strip,
                ..
            } => scene.polychrome_sprites.get(index).is_some_and(|s| {
                (row_phase(s.bounds.top().0) - y_phase).abs() <= 1e-5
                    && strip.is_none_or(|strip| strip.fits(s.bounds))
            }),
            LayerContent::Bar {
                index,
                key,
                left,
                bottom,
            } => scene.retained_bars.get(&index).is_some_and(|bar| {
                bar.bounds.left().0 == left
                    && (bar.bounds.bottom().0 - bottom).abs() < 1e-3
                    && bar.bounds.size.width.0 == key.width as f32
            }),
        }
    }

    fn place(&mut self, scene: &Scene) -> Result<bool> {
        let (x, y) = match self.content {
            LayerContent::Image {
                index,
                first_row,
                strip,
                ..
            } => {
                let b = scene.polychrome_sprites[index].bounds;
                (
                    b.left().0 - 0.125 + strip.map_or(0., |s| s.first as f32 * 0.25),
                    (b.top().0 - 0.5).ceil() + first_row as f32,
                )
            }
            LayerContent::Bar { index, key, .. } => {
                let bar = scene.retained_bars[&index];
                let phase = (bar.bounds.size.height.0 * BAR_PHASES as f32)
                    .round()
                    .clamp(0., (key.height * BAR_PHASES) as f32) as u32;
                (
                    bar.bounds.left().0 - (phase % BAR_PHASES * key.width) as f32,
                    bar.ceiling.0.floor() - (phase / BAR_PHASES * key.height) as f32,
                )
            }
        };
        let old = self.position;
        unsafe {
            if old.map(|p| p.0) != Some(x) {
                self.visual.SetOffsetX2(x)?;
            }
            if old.map(|p| p.1) != Some(y) {
                self.visual.SetOffsetY2(y)?;
            }
        }
        self.position = Some((x, y));
        Ok(old != self.position)
    }
}

// Destination pixels integrate [y, y+1] in the original chart's coordinate
// system. Retain a transparent guard row on either side for interpolation.
fn filtered_rows(
    top: f32,
    height: f32,
    source_height: u32,
    ink: std::ops::Range<u32>,
) -> std::ops::Range<u32> {
    if ink.is_empty() || source_height == 0 {
        return 0..0;
    }
    let first = (top - 0.5).ceil();
    let last = (top + height - 0.5).ceil();
    let scale = height / source_height as f32;
    let start = ((top + ink.start as f32 * scale).floor() - 1.).clamp(first, last);
    let end = ((top + ink.end as f32 * scale).ceil() + 1.).clamp(first, last);
    (start - first) as u32..(end - first) as u32
}

fn surface(
    comp: &IDCompositionDevice,
    devices: &DirectXRendererDevices,
    width: u32,
    height: u32,
    format: DXGI_FORMAT,
    alpha_mode: DXGI_ALPHA_MODE,
    paint: impl FnOnce(&ID3D11Texture2D, POINT) -> Result<()>,
) -> Result<IDCompositionSurface> {
    let surface = unsafe { comp.CreateSurface(width, height, format, alpha_mode)? };
    draw_surface(&surface, devices, None, paint)?;
    Ok(surface)
}

fn draw_surface(
    surface: &IDCompositionSurface,
    devices: &DirectXRendererDevices,
    rect: Option<RECT>,
    paint: impl FnOnce(&ID3D11Texture2D, POINT) -> Result<()>,
) -> Result<()> {
    unsafe {
        let mut offset = POINT::default();
        let update: IDXGISurface =
            surface.BeginDraw(rect.as_ref().map(std::ptr::from_ref), &mut offset)?;
        let result = update
            .cast()
            .map_err(anyhow::Error::from)
            .and_then(|target| paint(&target, offset));
        devices.device_context.OMSetRenderTargets(None, None);
        drop(update);
        let ended = surface.EndDraw();
        result?;
        ended?;
        Ok(())
    }
}

fn copy_scene(
    surface: &IDCompositionSurface,
    devices: &DirectXRendererDevices,
    source: &ID3D11Texture2D,
    rect: Option<RECT>,
) -> Result<()> {
    let region = rect.map(|r| D3D11_BOX {
        left: r.left as u32,
        top: r.top as u32,
        right: r.right as u32,
        bottom: r.bottom as u32,
        front: 0,
        back: 1,
    });
    draw_surface(surface, devices, rect, |target, at| unsafe {
        devices.device_context.CopySubresourceRegion(
            target,
            0,
            at.x as u32,
            at.y as u32,
            0,
            source,
            0,
            region.as_ref().map(std::ptr::from_ref),
        );
        Ok(())
    })
}

fn viewport(x: f32, y: f32, w: u32, h: u32) -> D3D11_VIEWPORT {
    D3D11_VIEWPORT {
        TopLeftX: x,
        TopLeftY: y,
        Width: w as f32,
        Height: h as f32,
        MinDepth: 0.,
        MaxDepth: 1.,
    }
}

struct Filter {
    vertex: ID3D11VertexShader,
    pixel: ID3D11PixelShader,
    columns: ID3D11PixelShader,
    params: ID3D11Buffer,
}

impl Filter {
    fn new(device: &ID3D11Device) -> Result<Self> {
        let vertex = RawShaderBytes::new(ShaderModule::ChartFilter, ShaderTarget::Vertex)?;
        let pixel = RawShaderBytes::new(ShaderModule::ChartFilter, ShaderTarget::Fragment)?;
        let columns = RawShaderBytes::new(ShaderModule::ChartBackdrop, ShaderTarget::Fragment)?;
        Ok(Self {
            vertex: create_vertex_shader(device, vertex.as_bytes())?,
            pixel: create_fragment_shader(device, pixel.as_bytes())?,
            columns: create_fragment_shader(device, columns.as_bytes())?,
            params: create_constant_buffer::<[i32; 12]>(device)?
                .context("chart filter constants")?,
        })
    }

    fn image(
        &self,
        comp: &IDCompositionDevice,
        devices: &DirectXRendererDevices,
        atlas: &DirectXAtlas,
        sprite: &PolychromeSprite,
        rows: &std::ops::Range<u32>,
        matte: Option<(Strip, &ID3D11ShaderResourceView)>,
    ) -> Result<IDCompositionSurface> {
        let tile = sprite.tile.bounds;
        let w = matte.map_or(2 * tile.size.width.0 as u32 + 1, |(s, _)| s.width);
        let top = sprite.bounds.top().0;
        let h = rows.end - rows.start;
        let y_scale = tile.size.height.0 as f32 / sprite.bounds.size.height.0;
        let view = atlas
            .get_texture_view(sprite.tile.texture_id)
            .context("chart atlas texture")?;
        surface(
            comp,
            devices,
            w,
            h,
            if matte.is_some() {
                RENDER_TARGET_FORMAT
            } else {
                DXGI_FORMAT_R16G16B16A16_FLOAT
            },
            if matte.is_some() {
                DXGI_ALPHA_MODE_IGNORE
            } else {
                DXGI_ALPHA_MODE_PREMULTIPLIED
            },
            |target, offset| unsafe {
                let ctx = &devices.device_context;
                let mut rtv = None;
                devices
                    .device
                    .CreateRenderTargetView(target, None, Some(&mut rtv))?;
                ctx.OMSetRenderTargets(Some(&[rtv]), None);
                ctx.RSSetViewports(Some(&[viewport(offset.x as f32, offset.y as f32, w, h)]));
                update_buffer(
                    ctx,
                    &self.params,
                    &[
                        tile.origin.x.0,
                        tile.origin.y.0,
                        tile.size.width.0,
                        tile.size.height.0,
                        offset.x,
                        offset.y - rows.start as i32,
                        (row_phase(top) * y_scale).to_bits() as i32,
                        y_scale.to_bits() as i32,
                        matte.map_or(0, |(s, _)| s.first),
                        matte.map_or(0, |(s, _)| s.clip.left),
                        (top - 0.5).ceil() as i32,
                        i32::from(matte.is_some()),
                    ],
                )?;
                ctx.PSSetConstantBuffers(2, Some(&[Some(self.params.clone())]));
                ctx.IASetPrimitiveTopology(D3D_PRIMITIVE_TOPOLOGY_TRIANGLESTRIP);
                ctx.VSSetShader(&self.vertex, None);
                ctx.PSSetShader(&self.pixel, None);
                ctx.OMSetBlendState(None, None, u32::MAX);
                ctx.PSSetShaderResources(0, Some(&view));
                ctx.PSSetShaderResources(2, Some(&[matte.map(|(_, prefix)| prefix.clone())]));
                ctx.Draw(4, 0);
                ctx.PSSetShaderResources(0, Some(&[None]));
                ctx.PSSetShaderResources(2, Some(&[None]));
                Ok(())
            },
        )
    }
}

/// Each cell is painted by the same quad shader as the swap chain. A moving
/// leading edge selects a cell, to within 1/256 of a physical pixel.
fn bar_sheet(
    comp: &IDCompositionDevice,
    devices: &DirectXRendererDevices,
    pipeline: &mut PipelineState<Quad>,
    key: BarKey,
    template: Quad,
) -> Result<IDCompositionSurface> {
    let w = key.width * BAR_PHASES;
    let h = key.height * (key.height + 1);
    surface(
        comp,
        devices,
        w,
        h,
        RENDER_TARGET_FORMAT,
        DXGI_ALPHA_MODE_PREMULTIPLIED,
        |target, offset| unsafe {
            let ctx = &devices.device_context;
            let mut desc = D3D11_TEXTURE2D_DESC::default();
            target.GetDesc(&mut desc);
            let mut view = None;
            devices
                .device
                .CreateRenderTargetView(target, None, Some(&mut view))?;
            let view = view.context("bar sheet render target")?;
            let rect = RECT {
                left: offset.x,
                top: offset.y,
                right: offset.x + w as i32,
                bottom: offset.y + h as i32,
            };
            ctx.cast::<ID3D11DeviceContext1>()?
                .ClearView(&view, &[0.; 4], Some(&[rect]));
            ctx.OMSetRenderTargets(Some(&[Some(view)]), None);
            ctx.RSSetViewports(Some(&[viewport(0., 0., desc.Width, desc.Height)]));
            let global =
                create_constant_buffer::<GlobalParams>(&devices.device)?.context("bar viewport")?;
            update_buffer(
                ctx,
                &global,
                &[GlobalParams {
                    viewport_size: [desc.Width as f32, desc.Height as f32],
                    ..Default::default()
                }],
            )?;
            ctx.VSSetConstantBuffers(0, Some(&[Some(global.clone())]));
            ctx.PSSetConstantBuffers(0, Some(&[Some(global)]));
            let batch =
                create_constant_buffer::<BatchParams>(&devices.device)?.context("bar batch")?;
            update_batch_start(ctx, &batch, 0)?;
            ctx.VSSetConstantBuffers(1, Some(&[Some(batch)]));
            let mut quads = Vec::with_capacity((key.height * BAR_PHASES) as usize);
            for n in 1..=key.height * BAR_PHASES {
                let left = (offset.x as u32 + n % BAR_PHASES * key.width) as f32;
                let ceiling = (offset.y as u32 + n / BAR_PHASES * key.height) as f32;
                let bottom = ceiling + key.height as f32;
                let bounds = Bounds::from_corners(
                    point(
                        ScaledPixels(left),
                        ScaledPixels(bottom - n as f32 / BAR_PHASES as f32),
                    ),
                    point(ScaledPixels(left + key.width as f32), ScaledPixels(bottom)),
                );
                let bar = RetainedBar {
                    bounds,
                    ceiling: ScaledPixels(ceiling),
                    color: key.color,
                };
                if let Some((bounds, background)) = bar.paint() {
                    quads.push(Quad {
                        bounds,
                        background,
                        content_mask: ContentMask {
                            bounds: Bounds::from_corners(
                                point(ScaledPixels(left), ScaledPixels(ceiling)),
                                bar.bounds.bottom_right(),
                            ),
                        },
                        ..template
                    });
                }
            }
            pipeline.update_buffer(&devices.device, ctx, &quads)?;
            pipeline.draw(
                ctx,
                D3D_PRIMITIVE_TOPOLOGY_TRIANGLESTRIP,
                4,
                quads.len() as u32,
            )
        },
    )
}

fn child(
    comp: &IDCompositionDevice,
    root: &IDCompositionVisual,
    mask: RECT,
    bitmap: &IDCompositionSurface,
) -> Result<IDCompositionVisual> {
    unsafe {
        let clip = comp.CreateVisual()?;
        let rectangle = comp.CreateRectangleClip()?;
        rectangle.SetLeft2(mask.left as f32)?;
        rectangle.SetTop2(mask.top as f32)?;
        rectangle.SetRight2(mask.right as f32)?;
        rectangle.SetBottom2(mask.bottom as f32)?;
        clip.SetClip(&rectangle)?;
        let visual = comp.CreateVisual()?;
        visual.SetContent(bitmap)?;
        clip.AddVisual(&visual, false, None)?;
        root.AddVisual(&clip, false, None)?;
        Ok(visual)
    }
}

impl DirectXRenderer {
    /// A cached tree and its next fully painted frame must enter the same
    /// compositor transaction stream. Reattaching the flip chain here can
    /// expose its last presented chart position for one refresh.
    pub(super) fn present_chart_frame(&mut self, scene: &Scene) -> Result<bool> {
        let Some(comp) = &mut self.direct_composition else {
            return Ok(false);
        };
        let Some(output) = &comp.charts.output else {
            return Ok(false);
        };
        if comp.charts.unavailable
            || comp.charts.standard_sdr != Some(true)
            || output.size != (self.width, self.height)
            || !scene.surfaces.is_empty()
            || !scene
                .polychrome_sprites
                .iter()
                .any(|s| s.pad == PolychromeSprite::IN_SCENE_AREA_FILTER)
        {
            return Ok(false);
        }
        let r = self
            .resources
            .as_ref()
            .context("missing chart frame resources")?;
        let source = r
            .render_target
            .as_ref()
            .context("missing completed chart frame")?;
        let devices = self
            .devices
            .as_ref()
            .context("missing chart frame device")?;
        let tree = comp.charts.tree.as_ref();
        let previous = tree.map(|t| t.key);
        // The tree's backing contains just the prefix. A second flat frame
        // can update the dirty regions of the preceding completed frame.
        let dirty = if tree.is_none() {
            r.present_dirty.as_deref()
        } else {
            None
        };
        if let Some(dirty) = dirty {
            for rect in dirty {
                copy_scene(&output.bitmap, devices, source, Some(*rect))?;
            }
        } else {
            copy_scene(&output.bitmap, devices, source, None)?;
        }
        unsafe {
            if let Some(tree) = tree {
                output.root.RemoveAllVisuals()?;
                output.root.SetContent(&output.bitmap)?;
                if comp.charts.skipped == Some(tree.key) {
                    comp.charts.skipped = None;
                }
            }
            comp.comp_device.Commit()?;
        }
        comp.charts.tree = None;
        crate::frame_trace::record(8, u64::from(previous.is_some()));
        if diagnostics()
            && let Some(key) = previous
        {
            println!(
                "{{\"event\":\"gpu-chart-handoff\",\"revision\":{}}}",
                key.revision
            );
        }
        Ok(true)
    }

    pub(crate) fn release_hidden_chart_layers(&mut self) -> Result<bool> {
        let Some(comp) = &mut self.direct_composition else {
            return Ok(false);
        };
        // A full paint just before hiding may already have detached the tree.
        // Its releases still need to reach the compositor before the idle period.
        comp.charts.backdrop_scan = None;
        if !comp.charts.used_since_hide {
            return Ok(false);
        }
        if comp.charts.active() {
            unsafe {
                comp.comp_target.SetRoot(comp.swap_visual.as_ref())?;
                comp.comp_device.Commit()?;
            }
        }
        comp.charts.deactivate();
        comp.charts.bars.clear();
        if let Some(r) = self.resources.as_mut() {
            // The next visible draw must reconstruct the entire swap-chain buffer.
            r.prefix_key = None;
            r.prefix_view = None;
            r.prefix = None;
        }
        if let Some(devices) = &self.devices {
            unsafe {
                devices
                    .device_context
                    .PSSetShaderResources(2, Some(&[None]));
            }
        }
        unsafe {
            comp.comp_device.Commit()?;
        }
        comp.charts.used_since_hide = false;
        Ok(true)
    }

    pub(super) fn update_chart_tree(
        &mut self,
        scene: &Scene,
        background: WindowBackgroundAppearance,
    ) -> Result<bool> {
        let Some(comp) = &mut self.direct_composition else {
            return Ok(false);
        };
        let Some(tree) = &mut comp.charts.tree else {
            return Ok(false);
        };
        if Some(tree.key) != key(scene, self.width, self.height, background)
            || comp.charts.unavailable
            || comp.charts.standard_sdr != Some(true)
        {
            return Ok(false);
        }
        if !tree.layers.iter().all(|layer| layer.compatible(scene)) {
            return Ok(false);
        }
        let mut changed = false;
        for layer in &mut tree.layers {
            changed |= layer.place(scene)?;
        }
        if changed {
            unsafe {
                comp.comp_device.Commit()?;
            }
        }
        Ok(true)
    }

    pub(super) fn install_chart_tree(
        &mut self,
        scene: &Scene,
        background: WindowBackgroundAppearance,
    ) -> Result<bool> {
        let Some(key) = key(scene, self.width, self.height, background) else {
            return Ok(false);
        };
        let Some(comp) = &mut self.direct_composition else {
            return Ok(false);
        };
        if comp.charts.unavailable || comp.charts.skipped == Some(key) {
            return Ok(false);
        }
        comp.charts.skipped = Some(key);
        if !*comp
            .charts
            .standard_sdr
            .get_or_insert_with(|| standard_sdr_window(self.hwnd))
        {
            return Ok(false);
        }
        let r = self.resources.as_ref().context("missing chart resources")?;
        if r.prefix_key != Some((key.revision, key.order as usize, key.opaque)) {
            return Ok(false);
        }
        let batches = split_batches(scene, Some(key.order));
        let suffix: Vec<_> = batches
            .iter()
            .filter(|b| batch_last_order(scene, b) >= key.order)
            .collect();
        let mut bar_keys = Vec::new();
        let mut images = 0;
        let mut image_bytes = 0u64;
        let mut untrimmed_bytes = 0u64;
        let mut image_rows = Vec::new();
        let mut masks = Vec::new();
        let mut marked_masks = Vec::new();
        let mut scan_regions = Vec::new();
        for b in &suffix {
            match b {
                PrimitiveBatch::PolychromeSprites { range, .. } => {
                    for i in range.clone() {
                        let s = &scene.polychrome_sprites[i];
                        if !supported_image(s)
                            || self.atlas.get_texture_view(s.tile.texture_id).is_none()
                        {
                            return Ok(false);
                        }
                        let mut rows = filtered_rows(
                            s.bounds.top().0,
                            s.bounds.size.height.0,
                            s.tile.bounds.size.height.0 as u32,
                            self.atlas.chart_rows(&s.tile),
                        );
                        let mask = clip(s.content_mask.bounds, self.width, self.height);
                        if s.pad == PolychromeSprite::IN_SCENE_AREA_FILTER {
                            // Interaction/full paints use the same source-over operation in the
                            // scene. Promote only pixels that we can finish on an opaque backdrop.
                            static DISABLED: OnceLock<bool> = OnceLock::new();
                            if *DISABLED.get_or_init(|| {
                                std::env::var_os("ARGUS_GPUI_DISABLE_CHART_BACKDROP_CACHE")
                                    .is_some()
                            }) {
                                return Ok(false);
                            }
                            let top = (s.bounds.top().0 - 0.5).ceil() as i32;
                            rows.start = rows.start.max(mask.top.saturating_sub(top).max(0) as u32);
                            rows.end = rows.end.min(mask.bottom.saturating_sub(top).max(0) as u32);
                            if rows.start > rows.end || mask.left >= mask.right {
                                rows = 0..0;
                            }
                            if !rows.is_empty() {
                                marked_masks.push(masks.len());
                                scan_regions.push((
                                    i,
                                    RECT {
                                        top: top + rows.start as i32,
                                        bottom: top + rows.end as i32,
                                        ..mask
                                    },
                                ));
                            }
                        } else {
                            let width = 2 * s.tile.bounds.size.width.0 as u64 + 1;
                            image_bytes += width * u64::from(rows.end - rows.start) * 8;
                            let full_height = (s.bounds.bottom().0 - 0.5).ceil()
                                - (s.bounds.top().0 - 0.5).ceil();
                            untrimmed_bytes += width * full_height as u64 * 8;
                        }
                        masks.push(if rows.is_empty() {
                            RECT::default()
                        } else {
                            mask
                        });
                        images += usize::from(!rows.is_empty());
                        image_rows.push(rows);
                    }
                }
                PrimitiveBatch::Quads(range) => {
                    for i in range.clone() {
                        let Some(bar) = scene.retained_bars.get(&i) else {
                            return Ok(false);
                        };
                        let Some(k) = BarKey::new(*bar, &scene.quads[i]) else {
                            return Ok(false);
                        };
                        let region = Bounds::from_corners(
                            point(bar.bounds.left(), ScaledPixels(bar.ceiling.0.floor())),
                            bar.bounds.bottom_right(),
                        );
                        masks.push(clip(
                            region.intersect(&scene.quads[i].content_mask.bounds),
                            self.width,
                            self.height,
                        ));
                        if !bar_keys.contains(&k) {
                            bar_keys.push(k);
                        }
                    }
                }
                _ => return Ok(false),
            }
        }
        let Some(area) = masks
            .iter()
            .copied()
            .filter(|r| r.left < r.right && r.top < r.bottom)
            .reduce(|a, b| RECT {
                left: a.left.min(b.left),
                top: a.top.min(b.top),
                right: a.right.max(b.right),
                bottom: a.bottom.max(b.bottom),
            })
        else {
            return Ok(false);
        };
        // Reserve space for the compositor's intermediate as well as our bitmaps.
        let group_bytes = if comp.swap_visual.is_some() {
            (area.right - area.left) as u64 * (area.bottom - area.top) as u64 * 8
        } else {
            0
        };
        let bar_bytes = bar_keys.iter().map(|k| k.bytes()).sum::<u64>();
        let devices = self.devices.as_ref().context("missing chart device")?;
        let prefix_view = r.prefix_view.as_ref().context("chart prefix view")?;
        let mut composed = std::collections::BTreeMap::new();
        let mut opaque_strips = 0;
        if comp.charts.filter.is_none() {
            comp.charts.filter = Some(Filter::new(&devices.device)?);
        }
        if !scan_regions.is_empty() {
            // An opaque part would erase another animated primitive if they overlapped.
            for &i in &marked_masks {
                let a = masks[i];
                if masks.iter().enumerate().any(|(j, b)| {
                    i != j
                        && a.left < b.right
                        && b.left < a.right
                        && a.top < b.bottom
                        && b.top < a.bottom
                }) {
                    return Ok(false);
                }
            }
            let filter = comp.charts.filter.as_ref().context("chart filter")?;
            let pending = &mut comp.charts.backdrop_scan;
            if !pending
                .as_ref()
                .is_some_and(|s| s.key == key && s.regions == scan_regions)
            {
                *pending = Some(backdrop::Scan::start(
                    key,
                    scan_regions,
                    devices,
                    filter,
                    prefix_view,
                )?);
            }
            let result = pending
                .as_ref()
                .context("chart backdrop scan")?
                .poll(&devices.device_context)?;
            let std::task::Poll::Ready(regions) = result else {
                comp.charts.skipped = None;
                return Ok(false);
            };
            *pending = None;
            let Some(regions) = regions else {
                if diagnostics() {
                    println!(
                        "{{\"event\":\"gpu-chart-backdrop-rejected\",\"revision\":{}}}",
                        key.revision
                    );
                }
                return Ok(false);
            };
            for (i, bands) in regions {
                let s = &scene.polychrome_sprites[i];
                let mut parts = Vec::with_capacity(bands.len());
                let full_height =
                    (s.bounds.bottom().0 - 0.5).ceil() - (s.bounds.top().0 - 0.5).ceil();
                for band in bands {
                    let Some(strip) = Strip::new(band, s.bounds) else {
                        return Ok(false);
                    };
                    image_bytes += u64::from(strip.width) * (band.bottom - band.top) as u64 * 4;
                    untrimmed_bytes += u64::from(strip.width) * full_height as u64 * 4;
                    parts.push(Some(strip));
                }
                opaque_strips += parts.len();
                composed.insert(i, parts);
            }
        } else {
            comp.charts.backdrop_scan = None;
        }
        let tree_bytes = image_bytes
            + bar_bytes
            + group_bytes
            + u64::from(self.width) * u64::from(self.height) * 4;
        if (image_rows.is_empty() && bar_keys.is_empty())
            || bar_bytes > MAX_BAR_BYTES
            || tree_bytes > comp.charts.tree_budget
        {
            if diagnostics() {
                println!(
                    "{{\"event\":\"gpu-chart-budget\",\"bytes\":{tree_bytes},\"limit\":{}}}",
                    comp.charts.tree_budget
                );
            }
            return Ok(false);
        }
        let dev = &comp.comp_device;
        let charts = &mut comp.charts;
        charts.bars.retain(|(k, _)| bar_keys.contains(k));
        let filter = charts.filter.as_ref().context("chart filter")?;
        let root = unsafe { dev.CreateVisual()? };
        let prefix = r.prefix.as_ref().context("missing chart background")?;
        let alpha = if comp.swap_visual.is_none() {
            DXGI_ALPHA_MODE_IGNORE
        } else {
            DXGI_ALPHA_MODE_PREMULTIPLIED
        };
        let previous = charts
            .output
            .as_ref()
            .filter(|frame| opaque_strips > 0 && frame.size == key.size);
        let background = match previous {
            Some(frame) => frame.bitmap.clone(),
            None => unsafe {
                dev.CreateSurface(self.width, self.height, RENDER_TARGET_FORMAT, alpha)?
            },
        };
        let output = if opaque_strips == 0 {
            None
        } else if let Some(frame) = previous {
            Some(frame.clone())
        } else {
            Some(ChartOutput {
                size: key.size,
                root: unsafe { dev.CreateVisual()? },
                bitmap: background.clone(),
            })
        };
        // Only the chart region needs an off-screen group. Keep the backdrop
        // and antialiased window corners outside it, painting each prefix pixel once.
        let viewport = RECT {
            left: 0,
            top: 0,
            right: self.width as i32,
            bottom: self.height as i32,
        };
        let group = if comp.swap_visual.is_none() {
            unsafe {
                root.SetContent(&background)?;
            }
            root.clone()
        } else {
            unsafe {
                for outside in subtract_rect(viewport, area) {
                    child(dev, &root, outside, &background)?;
                }
                let group = dev.CreateVisual()?;
                child(dev, &group, area, &background)?;
                let output = dev
                    .cast::<IDCompositionDevice3>()?
                    .CreateColorMatrixEffect()?;
                output.SetClampOutput(true)?;
                group.SetEffect(&output)?;
                root.AddVisual(&group, false, None)?;
                group
            }
        };
        let mut layers = Vec::new();
        let mut image_rows = image_rows.into_iter();
        let mut masks = masks.into_iter();
        for b in suffix {
            match b {
                PrimitiveBatch::PolychromeSprites { range, .. } => {
                    for i in range.clone() {
                        let s = &scene.polychrome_sprites[i];
                        let rows = image_rows.next().context("missing chart rows")?;
                        let mask = masks.next().context("missing chart clip")?;
                        if rows.is_empty() {
                            continue;
                        }
                        for &strip in composed.get(&i).map(Vec::as_slice).unwrap_or(&[None]) {
                            let bitmap = filter.image(
                                dev,
                                devices,
                                &self.atlas,
                                s,
                                &rows,
                                strip.map(|s| (s, prefix_view)),
                            )?;
                            let visual =
                                child(dev, &group, strip.map_or(mask, |s| s.clip), &bitmap)?;
                            unsafe {
                                visual.SetBitmapInterpolationMode(
                                    DCOMPOSITION_BITMAP_INTERPOLATION_MODE_LINEAR,
                                )?;
                                visual.SetTransform2(&Matrix3x2 {
                                    M11: 0.25,
                                    M22: 1.0,
                                    ..Default::default()
                                })?;
                            }
                            layers.push(Layer {
                                visual,
                                position: None,
                                content: LayerContent::Image {
                                    index: i,
                                    y_phase: row_phase(s.bounds.top().0),
                                    first_row: rows.start,
                                    strip,
                                },
                            });
                        }
                    }
                }
                PrimitiveBatch::Quads(range) => {
                    for i in range.clone() {
                        let q = scene.quads[i];
                        let bar = scene.retained_bars[&i];
                        let k = BarKey::new(bar, &q).context("bar geometry")?;
                        if !charts.bars.iter().any(|(key, _)| *key == k) {
                            self.pipelines.uploaded_revision = None;
                            let bitmap =
                                bar_sheet(dev, devices, &mut self.pipelines.quad_pipeline, k, q)?;
                            charts.bars.push((k, bitmap));
                        }
                        let bitmap = &charts
                            .bars
                            .iter()
                            .find(|(key, _)| *key == k)
                            .context("bar sheet")?
                            .1;
                        let visual = child(
                            dev,
                            &group,
                            masks.next().context("missing bar clip")?,
                            bitmap,
                        )?;
                        layers.push(Layer {
                            visual,
                            position: None,
                            content: LayerContent::Bar {
                                index: i,
                                key: k,
                                left: bar.bounds.left().0,
                                bottom: bar.bounds.bottom().0,
                            },
                        });
                    }
                }
                _ => unreachable!(),
            }
        }
        for layer in &mut layers {
            layer.place(scene)?;
        }
        copy_scene(&background, devices, prefix, None)?;
        unsafe {
            if let Some(frame) = &output {
                frame.root.RemoveAllVisuals()?;
                frame.root.SetContent(None::<&windows::core::IUnknown>)?;
                frame.root.AddVisual(&root, false, None)?;
                if previous.is_none() {
                    comp.comp_target.SetRoot(&frame.root)?;
                }
            } else {
                comp.comp_target.SetRoot(&root)?;
            }
            dev.Commit()?;
        }
        crate::frame_trace::record(7, opaque_strips as u64);
        if diagnostics() {
            println!(
                "{{\"event\":\"gpu-chart-layers\",\"images\":{images},\"layers\":{},\"bytes\":{tree_bytes},\"opaqueStrips\":{opaque_strips},\"imageBytes\":{},\"untrimmedImageBytes\":{},\"barBytes\":{bar_bytes},\"groupBytes\":{group_bytes},\"revision\":{}}}",
                layers.len(),
                image_bytes,
                untrimmed_bytes,
                key.revision
            );
        }
        charts.output = output;
        charts.tree = Some(Tree { key, layers });
        charts.used_since_hide = true;
        Ok(true)
    }
}

fn diagnostics() -> bool {
    static ENABLED: OnceLock<bool> = OnceLock::new();
    *ENABLED.get_or_init(|| std::env::var_os("ARGUS_GPUI_DIAGNOSTICS").is_some())
}

#[cfg(test)]
mod tests {
    use super::{filtered_rows, row_phase};

    #[test]
    fn a_half_pixel_crossing_needs_a_new_filtered_first_row() {
        let top = 40.5_f32;
        let next = f32::from_bits(top.to_bits() + 1);
        assert!((top - next).abs() < 1e-5);
        assert!((row_phase(top) - row_phase(next)).abs() > 0.99);
        assert_eq!(row_phase(-0.25), row_phase(2.75));
    }

    #[test]
    fn trimmed_filter_covers_all_ink_at_fractional_origins() {
        for top in [-3.75_f32, 0., 0.499, 20.5, 200.875] {
            let height = 100.;
            let first = (top - 0.5).ceil();
            let full = filtered_rows(top, height, 200, 0..200);
            let ink = 71..85;
            let cropped = filtered_rows(top, height, 200, ink.clone());
            assert!(cropped.len() <= 10);
            for row in full {
                let low = (first + row as f32 - top) * 2.;
                if low < ink.end as f32 && low + 2. > ink.start as f32 {
                    assert!(cropped.contains(&row));
                }
            }
            assert!(filtered_rows(top, height, 200, 0..0).is_empty());
        }
    }
}

impl ChartComposition {
    pub(super) fn enabled(&self) -> bool {
        !self.unavailable
    }
    pub(super) fn new(device: &IDXGIDevice) -> Self {
        // Bound the whole resident tree, including the window background. A
        // flat cap sized for a 1440p client rejects a normal 4K graph on a
        // 16 GiB adapter; small cards should not reserve a desktop-card budget.
        let dedicated = unsafe { device.GetAdapter().and_then(|a| a.GetDesc()) }
            .map_or(0, |d| d.DedicatedVideoMemory as u64);
        let tree_budget = if dedicated == 0 {
            48 * 1024 * 1024
        } else {
            (dedicated / 64).min(MAX_TREE_BYTES)
        };
        Self {
            tree_budget,
            unavailable: std::env::var_os("ARGUS_GPUI_DISABLE_CHART_LAYERS").is_some()
                || !software_scheduled(device),
            ..Self::default()
        }
    }
    pub(super) fn display_changed(&mut self) {
        self.backdrop_scan = None;
        self.standard_sdr = None;
        self.skipped = None;
    }
    pub(super) fn active(&self) -> bool {
        self.tree.is_some() || self.output.is_some()
    }
    pub(super) fn moving_tree(&self) -> bool {
        self.tree.is_some()
    }
    pub(super) fn disable(&mut self) {
        self.backdrop_scan = None;
        self.unavailable = true;
        self.bars.clear();
    }
    pub(super) fn deactivate(&mut self) {
        self.output = None;
        self.tree = None;
        self.skipped = None;
    }
}
