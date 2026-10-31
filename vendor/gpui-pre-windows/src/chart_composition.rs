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
    skipped: Option<FrameKey>,
    unavailable: bool,
    standard_sdr: Option<bool>,
    tree_budget: u64,
    filter: Option<Filter>,
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
        status.is_ok() && caps.Anonymous.Value & 3 == 1
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

enum Layer {
    Image(usize, IDCompositionVisual, f32),
    Bar(usize, IDCompositionVisual, BarKey),
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
            height: height as u32,
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
    Some(FrameKey {
        revision: scene.paint_revision,
        order: scene.animated_order?,
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
    s.pad == PolychromeSprite::PREMULTIPLIED_AREA_FILTER
        && s.tile.bounds.size.width.0 > 0
        && s.tile.bounds.size.width.0 < 8192
        && (s.bounds.bottom().0 - 0.5).ceil() > (s.bounds.top().0 - 0.5).ceil()
        && s.opacity == 1.
        && s.grayscale == false.into()
        && s.corner_radii == Corners::default()
        && (s.bounds.size.width.0 * 2. - s.tile.bounds.size.width.0 as f32).abs() < 0.01
        && (s.bounds.size.height.0 * 2. - s.tile.bounds.size.height.0 as f32).abs() < 0.01
}

fn place(layer: &Layer, scene: &Scene) -> Result<()> {
    unsafe {
        match layer {
            Layer::Image(i, visual, _) => {
                let b = scene.polychrome_sprites[*i].bounds;
                // The first filtered texel is centered on the original tile boundary.
                visual.SetOffsetX2(b.left().0 - 0.125)?;
                visual.SetOffsetY2((b.top().0 - 0.5).ceil())?;
            }
            Layer::Bar(i, visual, key) => {
                let bar = scene.retained_bars[i];
                let b = bar.bounds;
                let phase = (b.size.height.0 * BAR_PHASES as f32)
                    .round()
                    .clamp(0., (key.height * BAR_PHASES) as f32) as u32;
                visual.SetOffsetX2(b.left().0 - (phase % BAR_PHASES * key.width) as f32)?;
                visual.SetOffsetY2(
                    bar.ceiling.0.floor() - (phase / BAR_PHASES * key.height) as f32,
                )?;
            }
        }
        Ok(())
    }
}

fn surface(
    comp: &IDCompositionDevice,
    devices: &DirectXRendererDevices,
    width: u32,
    height: u32,
    format: DXGI_FORMAT,
    paint: impl FnOnce(&ID3D11Texture2D, POINT) -> Result<()>,
) -> Result<IDCompositionSurface> {
    unsafe {
        let surface = comp.CreateSurface(width, height, format, DXGI_ALPHA_MODE_PREMULTIPLIED)?;
        let mut offset = POINT::default();
        let update: IDXGISurface = surface.BeginDraw(None, &mut offset)?;
        let result = update
            .cast()
            .map_err(anyhow::Error::from)
            .and_then(|target| paint(&target, offset));
        devices.device_context.OMSetRenderTargets(None, None);
        drop(update);
        let ended = surface.EndDraw();
        result?;
        ended?;
        Ok(surface)
    }
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
    params: ID3D11Buffer,
}

impl Filter {
    fn new(device: &ID3D11Device) -> Result<Self> {
        let vertex = RawShaderBytes::new(ShaderModule::ChartFilter, ShaderTarget::Vertex)?;
        let pixel = RawShaderBytes::new(ShaderModule::ChartFilter, ShaderTarget::Fragment)?;
        Ok(Self {
            vertex: create_vertex_shader(device, vertex.as_bytes())?,
            pixel: create_fragment_shader(device, pixel.as_bytes())?,
            params: create_constant_buffer::<[i32; 8]>(device)?
                .context("chart filter constants")?,
        })
    }

    fn image(
        &self,
        comp: &IDCompositionDevice,
        devices: &DirectXRendererDevices,
        atlas: &DirectXAtlas,
        sprite: &PolychromeSprite,
    ) -> Result<IDCompositionSurface> {
        let tile = sprite.tile.bounds;
        let w = 2 * tile.size.width.0 as u32 + 1;
        let top = sprite.bounds.top().0;
        let first_row = (top - 0.5).ceil();
        let h = ((sprite.bounds.bottom().0 - 0.5).ceil() - first_row) as u32;
        let y_scale = tile.size.height.0 as f32 / sprite.bounds.size.height.0;
        let view = atlas
            .get_texture_view(sprite.tile.texture_id)
            .context("chart atlas texture")?;
        surface(
            comp,
            devices,
            w,
            h,
            DXGI_FORMAT_R16G16B16A16_FLOAT,
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
                        offset.y,
                        ((first_row + 0.5 - top) * y_scale).to_bits() as i32,
                        y_scale.to_bits() as i32,
                    ],
                )?;
                ctx.PSSetConstantBuffers(2, Some(&[Some(self.params.clone())]));
                ctx.IASetPrimitiveTopology(D3D_PRIMITIVE_TOPOLOGY_TRIANGLESTRIP);
                ctx.VSSetShader(&self.vertex, None);
                ctx.PSSetShader(&self.pixel, None);
                ctx.OMSetBlendState(None, None, u32::MAX);
                ctx.PSSetShaderResources(0, Some(&view));
                ctx.Draw(4, 0);
                ctx.PSSetShaderResources(0, Some(&[None]));
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
    pub(super) fn update_chart_tree(
        &mut self,
        scene: &Scene,
        background: WindowBackgroundAppearance,
    ) -> Result<bool> {
        let Some(comp) = &mut self.direct_composition else {
            return Ok(false);
        };
        let Some(tree) = &comp.charts.tree else {
            return Ok(false);
        };
        if Some(tree.key) != key(scene, self.width, self.height, background)
            || comp.charts.unavailable
            || comp.charts.standard_sdr != Some(true)
        {
            return Ok(false);
        }
        if tree.layers.iter().any(|layer| match layer {
            Layer::Image(i, _, y_phase) => {
                (scene.polychrome_sprites[*i].bounds.top().0.fract() - y_phase).abs() > 1e-5
            }
            Layer::Bar(..) => false,
        }) {
            return Ok(false);
        }
        for layer in &tree.layers {
            place(layer, scene)?;
        }
        unsafe {
            comp.comp_device.Commit()?;
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
        static ENABLED: OnceLock<bool> = OnceLock::new();
        if comp.charts.unavailable
            || comp.charts.skipped == Some(key)
            || !*ENABLED
                .get_or_init(|| std::env::var_os("ARGUS_GPUI_DISABLE_CHART_LAYERS").is_none())
        {
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
        let mut pixels = 0u64;
        for b in &suffix {
            match b {
                PrimitiveBatch::PolychromeSprites { range, .. } => {
                    for s in &scene.polychrome_sprites[range.clone()] {
                        if !supported_image(s)
                            || self.atlas.get_texture_view(s.tile.texture_id).is_none()
                        {
                            return Ok(false);
                        }
                        images += 1;
                        pixels += (2 * s.tile.bounds.size.width.0 as u64 + 1)
                            * ((s.bounds.bottom().0 - 0.5).ceil() - (s.bounds.top().0 - 0.5).ceil())
                                as u64;
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
                        if !bar_keys.contains(&k) {
                            bar_keys.push(k);
                        }
                    }
                }
                _ => return Ok(false),
            }
        }
        let bar_bytes = bar_keys.iter().map(|k| k.bytes()).sum::<u64>();
        let tree_bytes =
            pixels * 8 + bar_bytes + u64::from(self.width) * u64::from(self.height) * 4;
        if images == 0 || bar_bytes > MAX_BAR_BYTES || tree_bytes > comp.charts.tree_budget {
            if diagnostics() {
                println!(
                    "{{\"event\":\"gpu-chart-budget\",\"bytes\":{tree_bytes},\"limit\":{}}}",
                    comp.charts.tree_budget
                );
            }
            return Ok(false);
        }
        let devices = self.devices.as_ref().context("missing chart device")?;
        let dev = &comp.comp_device;
        let charts = &mut comp.charts;
        charts.bars.retain(|(k, _)| bar_keys.contains(k));
        if charts.filter.is_none() {
            charts.filter = Some(Filter::new(&devices.device)?);
        }
        let filter = charts.filter.as_ref().context("chart filter")?;
        let root = unsafe { dev.CreateVisual()? };
        let prefix = r.prefix.as_ref().context("missing chart background")?;
        let background = surface(
            dev,
            devices,
            self.width,
            self.height,
            RENDER_TARGET_FORMAT,
            |target, at| unsafe {
                devices.device_context.CopySubresourceRegion(
                    target,
                    0,
                    at.x as u32,
                    at.y as u32,
                    0,
                    prefix,
                    0,
                    None,
                );
                Ok(())
            },
        )?;
        unsafe {
            root.SetContent(&background)?;
        }
        let mut layers = Vec::new();
        for b in suffix {
            match b {
                PrimitiveBatch::PolychromeSprites { range, .. } => {
                    for i in range.clone() {
                        let s = &scene.polychrome_sprites[i];
                        let bitmap = filter.image(dev, devices, &self.atlas, s)?;
                        let visual = child(
                            dev,
                            &root,
                            clip(s.content_mask.bounds, self.width, self.height),
                            &bitmap,
                        )?;
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
                        layers.push(Layer::Image(i, visual, s.bounds.top().0.fract()));
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
                        let region = Bounds::from_corners(
                            point(bar.bounds.left(), ScaledPixels(bar.ceiling.0.floor())),
                            bar.bounds.bottom_right(),
                        );
                        let visual = child(
                            dev,
                            &root,
                            clip(
                                region.intersect(&q.content_mask.bounds),
                                self.width,
                                self.height,
                            ),
                            bitmap,
                        )?;
                        layers.push(Layer::Bar(i, visual, k));
                    }
                }
                _ => unreachable!(),
            }
        }
        for layer in &layers {
            place(layer, scene)?;
        }
        unsafe {
            comp.comp_target.SetRoot(&root)?;
            dev.Commit()?;
        }
        if diagnostics() {
            println!(
                "{{\"event\":\"gpu-chart-layers\",\"images\":{images},\"layers\":{},\"bytes\":{tree_bytes},\"revision\":{}}}",
                layers.len(),
                key.revision
            );
        }
        charts.tree = Some(Tree { key, layers });
        Ok(true)
    }
}

fn diagnostics() -> bool {
    static ENABLED: OnceLock<bool> = OnceLock::new();
    *ENABLED.get_or_init(|| std::env::var_os("ARGUS_GPUI_DIAGNOSTICS").is_some())
}

impl ChartComposition {
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
            unavailable: !software_scheduled(device),
            ..Self::default()
        }
    }
    pub(super) fn display_changed(&mut self) {
        self.standard_sdr = None;
        self.skipped = None;
    }
    pub(super) fn active(&self) -> bool {
        self.tree.is_some()
    }
    pub(super) fn disable(&mut self) {
        self.unavailable = true;
    }
    pub(super) fn deactivate(&mut self) {
        self.tree = None;
        self.skipped = None;
    }
}
