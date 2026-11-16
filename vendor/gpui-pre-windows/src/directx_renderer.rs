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

#[path = "scene_cache.rs"]
mod scene_cache;

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

    // Cached viewport
    viewport: D3D11_VIEWPORT,
}

struct ForegroundTexture {
    size: (u32, u32),
    origin: windows::Win32::Foundation::POINT,
    texture: ID3D11Texture2D,
    target: ID3D11RenderTargetView,
    view: ID3D11ShaderResourceView,
}

struct PathTargets {
    resolved: ID3D11Texture2D,
    srv: Option<ID3D11ShaderResourceView>,
    msaa: ID3D11Texture2D,
    rtv: Option<ID3D11RenderTargetView>,
}

impl PathTargets {
    fn new(device: &ID3D11Device, width: u32, height: u32) -> Result<Self> {
        let (resolved, srv) = create_path_intermediate_texture(device, width, height)?;
        let (msaa, rtv) = create_path_intermediate_msaa_texture_and_view(device, width, height)?;
        Ok(Self {
            resolved,
            srv,
            msaa,
            rtv,
        })
    }
}

fn split_batches(scene: &Scene, boundary: Option<DrawOrder>) -> Vec<PrimitiveBatch> {
    split_primitive_batches(scene, scene.batches(), boundary)
}

fn split_primitive_batches(
    scene: &Scene,
    source: impl Iterator<Item = PrimitiveBatch>,
    boundary: Option<DrawOrder>,
) -> Vec<PrimitiveBatch> {
    let Some(boundary) = boundary else {
        return source.collect();
    };
    let mut batches = Vec::new();
    for batch in source {
        macro_rules! split {
            ($items:expr, $range:expr, $make:expr) => {{
                let range = $range;
                let middle = range.start
                    + $items[range.clone()].partition_point(|item| item.order < boundary);
                if middle > range.start {
                    batches.push(($make)(range.start..middle));
                }
                if middle < range.end {
                    batches.push(($make)(middle..range.end));
                }
            }};
        }
        match batch {
            PrimitiveBatch::Shadows(range) => split!(scene.shadows, range, PrimitiveBatch::Shadows),
            PrimitiveBatch::Quads(range) => split!(scene.quads, range, PrimitiveBatch::Quads),
            PrimitiveBatch::Paths(range) => split!(scene.paths, range, PrimitiveBatch::Paths),
            PrimitiveBatch::Underlines(range) => {
                split!(scene.underlines, range, PrimitiveBatch::Underlines)
            }
            PrimitiveBatch::Surfaces(range) => {
                split!(scene.surfaces, range, PrimitiveBatch::Surfaces)
            }
            PrimitiveBatch::MonochromeSprites { texture_id, range } => {
                split!(scene.monochrome_sprites, range, |range| {
                    PrimitiveBatch::MonochromeSprites { texture_id, range }
                })
            }
            PrimitiveBatch::SubpixelSprites { texture_id, range } => {
                split!(scene.subpixel_sprites, range, |range| {
                    PrimitiveBatch::SubpixelSprites { texture_id, range }
                })
            }
            PrimitiveBatch::PolychromeSprites { texture_id, range } => {
                split!(scene.polychrome_sprites, range, |range| {
                    PrimitiveBatch::PolychromeSprites { texture_id, range }
                })
            }
        }
    }
    batches
}

fn suffix_damage(
    scene: &Scene,
    batches: &[PrimitiveBatch],
    width: u32,
    height: u32,
) -> Option<Vec<RECT>> {
    let mut regions = Vec::new();
    let mut include = |bounds: Bounds<ScaledPixels>, mask: ContentMask<ScaledPixels>| {
        let clipped = bounds.intersect(&mask.bounds);
        if !clipped.is_empty() {
            let rect = RECT {
                left: (clipped.left().as_f32().floor() as i32 - 1).clamp(0, width as i32),
                top: (clipped.top().as_f32().floor() as i32 - 1).clamp(0, height as i32),
                right: (clipped.right().as_f32().ceil() as i32 + 1).clamp(0, width as i32),
                bottom: (clipped.bottom().as_f32().ceil() as i32 + 1).clamp(0, height as i32),
            };
            if rect.right > rect.left && rect.bottom > rect.top {
                merge_damage(&mut regions, rect);
            }
        }
    };
    for batch in batches {
        match batch {
            // Shadows can extend beyond primitive bounds; use full presentation.
            PrimitiveBatch::Shadows(_) | PrimitiveBatch::Surfaces(_) => return None,
            PrimitiveBatch::Quads(range) => {
                for item in &scene.quads[range.clone()] {
                    // Animated bars can shrink, including to zero. Restore the
                    // whole mask so pixels from previous frames are erased.
                    include(item.content_mask.bounds, item.content_mask);
                }
            }
            PrimitiveBatch::Paths(range) => {
                for item in &scene.paths[range.clone()] {
                    include(item.bounds, item.content_mask);
                }
            }
            PrimitiveBatch::Underlines(range) => {
                for item in &scene.underlines[range.clone()] {
                    include(item.bounds, item.content_mask);
                }
            }
            PrimitiveBatch::MonochromeSprites { range, .. } => {
                for item in &scene.monochrome_sprites[range.clone()] {
                    include(item.bounds, item.content_mask);
                }
            }
            PrimitiveBatch::SubpixelSprites { range, .. } => {
                for item in &scene.subpixel_sprites[range.clone()] {
                    include(item.bounds, item.content_mask);
                }
            }
            PrimitiveBatch::PolychromeSprites { range, .. } => {
                for item in &scene.polychrome_sprites[range.clone()] {
                    include(item.content_mask.bounds, item.content_mask);
                }
            }
        }
    }
    (!regions.is_empty()).then_some(regions)
}

// Keep distant charts separate, but combine aligned strips to avoid a GPU copy
// for every core chart. Merge overlaps before restoring and blending the suffix.
fn merge_damage(regions: &mut Vec<RECT>, mut rect: RECT) {
    let mut index = 0;
    while index < regions.len() {
        let other = regions[index];
        if (rect.left <= other.right
            && other.left <= rect.right
            && rect.top <= other.bottom
            && other.top <= rect.bottom)
            || (rect.top == other.top && rect.bottom == other.bottom)
            || (rect.left == other.left && rect.right == other.right)
        {
            regions.swap_remove(index);
            rect.left = rect.left.min(other.left);
            rect.top = rect.top.min(other.top);
            rect.right = rect.right.max(other.right);
            rect.bottom = rect.bottom.max(other.bottom);
            index = 0;
        } else {
            index += 1;
        }
    }
    regions.push(rect);
}

fn overlaps(a: &RECT, b: &RECT) -> bool {
    a.left < b.right && b.left < a.right && a.top < b.bottom && b.top < a.bottom
}

/// Regions of the render target, or all of it.
#[derive(Clone, Debug, PartialEq)]
enum Damage {
    Full,
    Rects(Vec<RECT>),
}

impl Damage {
    const NONE: Self = Self::Rects(Vec::new());

    fn add(&mut self, rect: RECT) {
        if let Self::Rects(rects) = self
            && rect.left < rect.right
            && rect.top < rect.bottom
        {
            merge_damage(rects, rect);
        }
    }

    fn extend(&mut self, other: &Self) {
        match other {
            Self::Full => *self = Self::Full,
            Self::Rects(rects) => {
                for rect in rects {
                    self.add(*rect);
                }
            }
        }
    }

    fn reaches(&self, rects: &[RECT]) -> bool {
        match self {
            Self::Full => !rects.is_empty(),
            Self::Rects(own) => own.iter().any(|a| rects.iter().any(|b| overlaps(a, b))),
        }
    }

    fn pixels(&self, width: u32, height: u32) -> u64 {
        match self {
            Self::Full => u64::from(width) * u64::from(height),
            Self::Rects(rects) => rects
                .iter()
                .map(|r| (r.right - r.left) as u64 * (r.bottom - r.top) as u64)
                .sum(),
        }
    }
}

/// What the swap chain's buffers hold. A flip-model chain hands back the
/// buffer it presented `BUFFER_COUNT` presents ago, and DWM may compose any
/// part of a presented buffer again later, not only its dirty rectangles: a
/// buffer is brought up to date in full before it is presented again.
struct Frames {
    /// The number of the last frame drawn into the render target.
    drawn: u64,
    /// The frame each buffer last received, in the order the swap chain
    /// returns them, the current back buffer first; `None` while unknown.
    buffers: [Option<u64>; BUFFER_COUNT],
    /// What each recent frame changed from the one before it, oldest first.
    changes: VecDeque<(u64, Damage)>,
    /// What changed since a frame last reached the screen.
    unpresented: Damage,
}

impl Frames {
    fn new() -> Self {
        Self {
            drawn: 0,
            buffers: [None; BUFFER_COUNT],
            changes: VecDeque::new(),
            unpresented: Damage::Full,
        }
    }

    /// What the back buffer lacks of the last drawn frame.
    fn stale(&self) -> Damage {
        let Some(held) = self.buffers[0] else {
            return Damage::Full;
        };
        if held == self.drawn {
            return Damage::NONE;
        }
        if self
            .changes
            .front()
            .is_none_or(|(frame, _)| *frame > held + 1)
        {
            return Damage::Full;
        }
        let mut stale = Damage::NONE;
        for (_, change) in self.changes.iter().filter(|(frame, _)| *frame > held) {
            stale.extend(change);
        }
        stale
    }

    fn drew(&mut self, change: Damage) {
        self.drawn += 1;
        self.unpresented.extend(&change);
        self.changes.push_back((self.drawn, change));
        while self.changes.len() > 2 * BUFFER_COUNT {
            self.changes.pop_front();
        }
        self.buffers[0] = Some(self.drawn);
    }

    /// The last drawn frame reached the screen.
    fn presented(&mut self) {
        self.unpresented = Damage::NONE;
    }

    /// The swap chain presented its back buffer and returns the next one.
    fn flipped(&mut self) {
        self.buffers.rotate_left(1);
    }

    fn forget(&mut self) {
        self.buffers = [None; BUFFER_COUNT];
        self.changes.clear();
        self.unpresented = Damage::Full;
    }
}

/// How the last frame was drawn.
enum Last {
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
enum Base<'a> {
    /// Cleared to the window background.
    Clear,
    /// As the last frame left it.
    Keep,
    /// With the cached prefix copied into these regions.
    Restore(&'a Damage),
}

/// What a frame did, for the GPU timing samples.
#[derive(Default)]
struct FrameReport {
    path: &'static str,
    reused: bool,
    repaired_pixels: u64,
    suffix: bool,
    /// The primitives a local repaint drew, out of the scene's.
    local: Option<(usize, usize)>,
    /// CPU time at the end of each step of the frame, with GPU timing.
    started: Option<std::time::Instant>,
    steps: Vec<(&'static str, f64)>,
}

impl FrameReport {
    fn step(&mut self, name: &'static str) {
        if let Some(started) = self.started {
            self.steps
                .push((name, started.elapsed().as_secs_f64() * 1_000_000.));
        }
    }
}

/// The regions `Present1` reports, `None` for the whole window. All of them
/// must be up to date in the buffer; an unchanged frame reports one pixel, as
/// no rectangles would mean all of them.
fn present_regions(partial: bool, unpresented: &Damage) -> Option<Vec<RECT>> {
    match unpresented {
        _ if !partial => None,
        Damage::Full => None,
        Damage::Rects(rects) if rects.is_empty() => Some(vec![RECT {
            left: 0,
            top: 0,
            right: 1,
            bottom: 1,
        }]),
        Damage::Rects(rects) => Some(rects.clone()),
    }
}

/// The scissored passes for `rects`: more than four merge into their bounds.
fn passes(rects: &[RECT]) -> Vec<RECT> {
    if rects.len() <= 4 {
        return rects.to_vec();
    }
    vec![rects.iter().skip(1).fold(rects[0], |a, r| RECT {
        left: a.left.min(r.left),
        top: a.top.min(r.top),
        right: a.right.max(r.right),
        bottom: a.bottom.max(r.bottom),
    })]
}

fn primitive_count(scene: &Scene) -> usize {
    scene.quads.len()
        + scene.underlines.len()
        + scene.monochrome_sprites.len()
        + scene.subpixel_sprites.len()
        + scene.polychrome_sprites.len()
}

fn batch_last_order(scene: &Scene, batch: &PrimitiveBatch) -> DrawOrder {
    match batch {
        PrimitiveBatch::Shadows(range) => scene.shadows[range.end - 1].order,
        PrimitiveBatch::Quads(range) => scene.quads[range.end - 1].order,
        PrimitiveBatch::Paths(range) => scene.paths[range.end - 1].order,
        PrimitiveBatch::Underlines(range) => scene.underlines[range.end - 1].order,
        PrimitiveBatch::MonochromeSprites { range, .. } => {
            scene.monochrome_sprites[range.end - 1].order
        }
        PrimitiveBatch::SubpixelSprites { range, .. } => {
            scene.subpixel_sprites[range.end - 1].order
        }
        PrimitiveBatch::PolychromeSprites { range, .. } => {
            scene.polychrome_sprites[range.end - 1].order
        }
        PrimitiveBatch::Surfaces(range) => scene.surfaces[range.end - 1].order,
    }
}

// A single sprite can restore its own background while painting. Only disjoint
// masks are eligible: overlapping layers must retain the ordinary blend order.
struct PrefixComposition {
    /// Everything the composed suffix writes. Frames present what changed
    /// instead, which covers this whenever the suffix changed.
    #[cfg_attr(not(test), allow(dead_code))]
    dirty: Vec<RECT>,
    restore: Vec<RECT>,
}

fn composition_rejected(reason: &'static str) -> Option<PrefixComposition> {
    static ENABLED: OnceLock<bool> = OnceLock::new();
    if *ENABLED.get_or_init(|| std::env::var_os("ARGUS_GPUI_DIAGNOSTICS").is_some()) {
        static COUNT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let count = COUNT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        if count < 3 || count.is_multiple_of(400) {
            println!("{{\"event\":\"gpu-composition-reject\",\"reason\":\"{reason}\"}}");
        }
    }
    None
}

fn subtract_rect(rect: RECT, cover: RECT) -> impl Iterator<Item = RECT> {
    let left = rect.left.max(cover.left);
    let top = rect.top.max(cover.top);
    let right = rect.right.min(cover.right);
    let bottom = rect.bottom.min(cover.bottom);
    let pieces = if left >= right || top >= bottom {
        [rect, RECT::default(), RECT::default(), RECT::default()]
    } else {
        [
            RECT {
                bottom: top,
                ..rect
            },
            RECT {
                top: bottom,
                ..rect
            },
            RECT {
                top,
                bottom,
                right: left,
                ..rect
            },
            RECT {
                top,
                bottom,
                left: right,
                ..rect
            },
        ]
    };
    pieces
        .into_iter()
        .filter(|piece| piece.left < piece.right && piece.top < piece.bottom)
}

fn independent_sprite_regions(
    scene: &Scene,
    batches: &[PrimitiveBatch],
    width: u32,
    height: u32,
) -> Option<PrefixComposition> {
    let mut regions: Vec<RECT> = Vec::new();
    let mut quads = Vec::new();
    let mut images = Vec::new();
    for batch in batches {
        if let PrimitiveBatch::Quads(range) = batch {
            quads.push(PrimitiveBatch::Quads(range.clone()));
            continue;
        }
        let PrimitiveBatch::PolychromeSprites { range, .. } = batch else {
            return composition_rejected("unsupported-batch");
        };
        for sprite in &scene.polychrome_sprites[range.clone()] {
            if !sprite.bounds.size.width.as_f32().is_finite()
                || !sprite.bounds.size.height.as_f32().is_finite()
                || sprite.bounds.size.width.as_f32() <= 0.
                || sprite.bounds.size.height.as_f32() <= 0.
            {
                return composition_rejected("invalid-bounds");
            }
            let mask = sprite.content_mask.bounds;
            let rect = RECT {
                left: (mask.left().as_f32().floor() as i32).clamp(0, width as i32),
                top: (mask.top().as_f32().floor() as i32).clamp(0, height as i32),
                right: (mask.right().as_f32().ceil() as i32).clamp(0, width as i32),
                bottom: (mask.bottom().as_f32().ceil() as i32).clamp(0, height as i32),
            };
            if rect.left >= rect.right || rect.top >= rect.bottom {
                continue;
            }
            // The composed vertex shader covers the complete mask; fragments
            // outside the image return the cached background. A rounded mask
            // may extend a pixel beyond an image at fractional display scales.
            if regions.iter().any(|other| {
                rect.left < other.right
                    && other.left < rect.right
                    && rect.top < other.bottom
                    && other.top < rect.bottom
            }) {
                return composition_rejected("overlapping-images");
            }
            regions.push(rect);
            images.push((rect, sprite.order));
        }
    }
    if regions.is_empty() {
        return composition_rejected("empty-regions");
    }
    let mut restore = Vec::new();
    for batch in quads {
        let PrimitiveBatch::Quads(range) = batch else {
            unreachable!()
        };
        for index in range {
            let Some(damage) = suffix_damage(
                scene,
                &[PrimitiveBatch::Quads(index..index + 1)],
                width,
                height,
            ) else {
                continue;
            };
            // Earlier images restore their own masks before this bar is drawn.
            // Copy only the bar's remaining background, including when its
            // restoration padding crosses the edge of a chart.
            let mut uncovered = damage;
            for (image, order) in &images {
                if *order < scene.quads[index].order
                    && uncovered.iter().any(|rect| {
                        rect.left < image.right
                            && image.left < rect.right
                            && rect.top < image.bottom
                            && image.top < rect.bottom
                    })
                {
                    uncovered = uncovered
                        .into_iter()
                        .flat_map(|rect| subtract_rect(rect, *image))
                        .collect();
                }
            }
            for rect in uncovered {
                merge_damage(&mut restore, rect);
            }
        }
    }
    if restore.iter().any(|quad| {
        regions.iter().any(|image| {
            quad.left < image.right
                && image.left < quad.right
                && quad.top < image.bottom
                && image.top < quad.bottom
        })
    }) {
        return composition_rejected("quad-restore-overlaps-image");
    }
    regions.extend_from_slice(&restore);
    Some(PrefixComposition {
        dirty: regions,
        restore,
    })
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
    charts: chart_composition::ChartComposition,
}

struct FlatBitmap {
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
fn composed_present_requested() -> bool {
    static REQUESTED: OnceLock<bool> = OnceLock::new();
    *REQUESTED.get_or_init(|| std::env::var_os("ARGUS_GPUI_COMPOSED_PRESENT").is_some())
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

    /// Copies the frame into the bitmap of the flat visual and commits it, in
    /// place of a flip. The render target is then never presented, as while a
    /// chart tree is shown, and keeps the last frame for partial paints.
    fn present_composed(&mut self) -> Result<()> {
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
        let result = match (prefix_end, animated_order) {
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

    /// A scene without an animated suffix. Only what differs from the frame
    /// the back buffer holds is drawn, from the primitives that reach it.
    fn render_whole(
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
    fn render_cached(
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
    fn draw_region(
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

    fn restore_rasterizer(&self) {
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

    fn prepare_batches(&mut self, batches: &[PrimitiveBatch]) -> Result<()> {
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
    fn draw_batches(
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

    fn scene_cache_event(
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

    fn paint_foreground(
        &mut self,
        scene: &Scene,
        batches: &[PrimitiveBatch],
        bounds: RECT,
        previous: Option<ForegroundTexture>,
        backdrop: Option<&ID3D11Texture2D>,
    ) -> Result<ForegroundTexture> {
        let devices = self.devices.clone().context("foreground device")?;
        let size = (
            (bounds.right - bounds.left).max(1) as u32,
            (bounds.bottom - bounds.top).max(1) as u32,
        );
        let size = (size.0.next_multiple_of(16), size.1.next_multiple_of(16));
        let mut foreground = match previous.filter(|f| f.size == size) {
            Some(foreground) => foreground,
            None => unsafe {
                let desc = D3D11_TEXTURE2D_DESC {
                    Width: size.0,
                    Height: size.1,
                    MipLevels: 1,
                    ArraySize: 1,
                    Format: RENDER_TARGET_FORMAT,
                    SampleDesc: DXGI_SAMPLE_DESC {
                        Count: 1,
                        Quality: 0,
                    },
                    Usage: D3D11_USAGE_DEFAULT,
                    BindFlags: (D3D11_BIND_RENDER_TARGET | D3D11_BIND_SHADER_RESOURCE).0 as u32,
                    ..Default::default()
                };
                let (mut texture, mut target, mut view) = (None, None, None);
                devices
                    .device
                    .CreateTexture2D(&desc, None, Some(&mut texture))?;
                let texture = texture.context("foreground texture")?;
                devices
                    .device
                    .CreateRenderTargetView(&texture, None, Some(&mut target))?;
                devices
                    .device
                    .CreateShaderResourceView(&texture, None, Some(&mut view))?;
                ForegroundTexture {
                    size,
                    origin: Default::default(),
                    texture,
                    target: target.context("foreground target")?,
                    view: view.context("foreground view")?,
                }
            },
        };
        foreground.origin = windows::Win32::Foundation::POINT {
            x: bounds.left,
            y: bounds.top,
        };
        let offset = gpui::point(
            ScaledPixels(-(bounds.left as f32)),
            ScaledPixels(-(bounds.top as f32)),
        );
        let mut local = Scene::default();
        macro_rules! copy {
            ($field:ident, $range:expr) => {
                for item in &scene.$field[$range.clone()] {
                    let mut item = *item;
                    item.bounds = item.bounds + offset;
                    item.content_mask.bounds = item.content_mask.bounds + offset;
                    local.$field.push(item);
                }
            };
        }
        for batch in batches {
            match batch {
                PrimitiveBatch::Quads(range) => copy!(quads, range),
                PrimitiveBatch::Underlines(range) => copy!(underlines, range),
                PrimitiveBatch::MonochromeSprites { range, .. } => {
                    for item in &scene.monochrome_sprites[range.clone()] {
                        let mut item = *item;
                        item.transformation.translation[0] += offset.x.0;
                        item.transformation.translation[1] += offset.y.0;
                        item.content_mask.bounds = item.content_mask.bounds + offset;
                        local.monochrome_sprites.push(item);
                    }
                }
                PrimitiveBatch::PolychromeSprites { range, .. } => copy!(polychrome_sprites, range),
                _ => anyhow::bail!("unsupported cached foreground batch"),
            }
        }
        let batches = local.batches().collect::<Vec<_>>();
        let resources = self.resources.as_mut().context("foreground resources")?;
        let target = resources.render_target.replace(foreground.texture.clone());
        let view = resources
            .render_target_view
            .replace(foreground.target.clone());
        let compose = std::mem::replace(&mut resources.compose_prefix, false);
        let viewport = std::mem::replace(
            &mut resources.viewport,
            D3D11_VIEWPORT {
                Width: size.0 as f32,
                Height: size.1 as f32,
                MinDepth: 0.,
                MaxDepth: 1.,
                ..Default::default()
            },
        );
        let work = self.work;
        self.pipelines.uploaded_revision = None;
        let result = (|| {
            self.pre_draw(&[0.; 4], Base::Clear)?;
            if let Some(backdrop) = backdrop {
                let source = D3D11_BOX {
                    left: bounds.left as u32,
                    top: bounds.top as u32,
                    front: 0,
                    right: bounds.right as u32,
                    bottom: bounds.bottom as u32,
                    back: 1,
                };
                unsafe {
                    devices.device_context.CopySubresourceRegion(
                        &foreground.texture,
                        0,
                        0,
                        0,
                        0,
                        backdrop,
                        0,
                        Some(&source),
                    );
                }
            }
            self.upload_scene_buffers(&local)?;
            self.globals.batch_params.prepare(
                &devices.device,
                &devices.device_context,
                &batches,
            )?;
            for (index, batch) in batches.iter().enumerate() {
                self.globals.batch_params.active = index;
                match batch {
                    PrimitiveBatch::Quads(range) => self.draw_quads(range.start, range.len()),
                    PrimitiveBatch::Underlines(range) => {
                        self.draw_underlines(range.start, range.len())
                    }
                    PrimitiveBatch::MonochromeSprites { texture_id, range } => {
                        self.draw_monochrome_sprites(*texture_id, range.start, range.len())
                    }
                    PrimitiveBatch::PolychromeSprites { texture_id, range } => {
                        self.draw_polychrome_sprites(*texture_id, range.start, range.len())
                    }
                    _ => anyhow::bail!("unsupported cached foreground batch"),
                }?;
            }
            Ok(())
        })();
        let resources = self.resources.as_mut().context("foreground resources")?;
        resources.render_target = target;
        resources.render_target_view = view;
        resources.compose_prefix = compose;
        resources.viewport = viewport;
        unsafe {
            devices
                .device_context
                .OMSetRenderTargets(Some(slice::from_ref(&resources.render_target_view)), None);
        }
        self.globals.last_params = None;
        self.pipelines.uploaded_revision = None;
        self.work = work;
        result?;
        Ok(foreground)
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

    fn release_path_targets(&mut self) {
        if let Some(resources) = self.resources.as_mut()
            && resources.paths.take().is_some()
            && let Some(devices) = &self.devices
        {
            // The path sprite's SRV may still be bound after its last draw.
            unsafe {
                devices
                    .device_context
                    .VSSetShaderResources(0, Some(&[None]));
                devices
                    .device_context
                    .PSSetShaderResources(0, Some(&[None]));
            }
            path_target_trace("release", self.width, self.height);
        }
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
        self.viewport = viewport;
        Ok(())
    }

    /// Nothing drawn before can be trusted: neither the render target's
    /// buffers nor the cached prefix.
    fn forget(&mut self) {
        self.prefix_key = None;
        self.prefix_snapshot = None;
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

// D3D11.1 constant-buffer windows are aligned to 16 shader constants (256 bytes).
// Keep each buffer within the 4096-constant shader limit, regardless of scene size.
const CONSTANTS_PER_BATCH: usize = 16;
const BATCHES_PER_BUFFER: usize =
    D3D11_REQ_CONSTANT_BUFFER_ELEMENT_COUNT as usize / CONSTANTS_PER_BATCH;
#[derive(Clone, Copy)]
#[repr(C, align(16))]
struct BatchSlot {
    start_index: u32,
    padding: [u32; CONSTANTS_PER_BATCH * 4 - 1],
}
struct BatchConstants {
    legacy: Option<ID3D11Buffer>,
    context: Option<ID3D11DeviceContext1>,
    buffers: Vec<Option<ID3D11Buffer>>,
    indices: Vec<u32>,
    staging: Vec<BatchSlot>,
    active: usize,
}
impl BatchConstants {
    fn new(device: &ID3D11Device) -> Result<Self> {
        let legacy = create_constant_buffer::<BatchParams>(device)?;
        let mut options = D3D11_FEATURE_DATA_D3D11_OPTIONS::default();
        let offsetting = unsafe {
            device.CheckFeatureSupport(
                D3D11_FEATURE_D3D11_OPTIONS,
                (&mut options as *mut D3D11_FEATURE_DATA_D3D11_OPTIONS).cast(),
                std::mem::size_of_val(&options) as u32,
            )
        }
        .is_ok()
            && options.ConstantBufferOffsetting.as_bool();
        let context = if offsetting {
            unsafe { device.GetImmediateContext() }
                .ok()
                .and_then(|context| context.cast().ok())
        } else {
            None
        };
        if std::env::var_os("ARGUS_GPUI_DIAGNOSTICS").is_some() {
            println!(
                "{{\"event\":\"gpu-batch-constants\",\"offsetting\":{}}}",
                context.is_some()
            );
        }
        Ok(Self {
            legacy,
            context,
            buffers: Vec::new(),
            indices: Vec::new(),
            staging: Vec::new(),
            active: 0,
        })
    }
    fn prepare(
        &mut self,
        device: &ID3D11Device,
        context: &ID3D11DeviceContext,
        batches: &[PrimitiveBatch],
    ) -> Result<()> {
        if self.context.is_none() {
            return Ok(());
        }
        let indices = batches
            .iter()
            .map(|batch| match batch {
                PrimitiveBatch::Shadows(range)
                | PrimitiveBatch::Quads(range)
                | PrimitiveBatch::Underlines(range) => range.start as u32,
                PrimitiveBatch::MonochromeSprites { range, .. }
                | PrimitiveBatch::SubpixelSprites { range, .. }
                | PrimitiveBatch::PolychromeSprites { range, .. } => range.start as u32,
                PrimitiveBatch::Paths(_) | PrimitiveBatch::Surfaces(_) => 0,
            })
            .collect::<Vec<_>>();
        if self.indices == indices {
            return Ok(());
        }
        self.staging.clear();
        self.staging
            .extend(indices.iter().map(|&start_index| BatchSlot {
                start_index,
                padding: [0; CONSTANTS_PER_BATCH * 4 - 1],
            }));
        for (index, slots) in self.staging.chunks(BATCHES_PER_BUFFER).enumerate() {
            if self.buffers.len() <= index {
                self.buffers
                    .push(create_constant_buffer::<[BatchSlot; BATCHES_PER_BUFFER]>(
                        device,
                    )?);
            }
            update_buffer(
                context,
                self.buffers[index]
                    .as_ref()
                    .context("batch constant buffer missing")?,
                slots,
            )?;
        }
        self.indices = indices;
        Ok(())
    }
    fn bind(&self, context: &ID3D11DeviceContext, first_instance: u32) -> Result<()> {
        if let Some(context1) = &self.context {
            anyhow::ensure!(
                self.indices.get(self.active) == Some(&first_instance),
                "batch constant index mismatch"
            );
            let buffer = &self.buffers[self.active / BATCHES_PER_BUFFER];
            let first = ((self.active % BATCHES_PER_BUFFER) * CONSTANTS_PER_BATCH) as u32;
            let count = CONSTANTS_PER_BATCH as u32;
            unsafe {
                context1.VSSetConstantBuffers1(
                    1,
                    1,
                    Some(buffer as *const Option<ID3D11Buffer>),
                    Some(&first),
                    Some(&count),
                );
            }
            Ok(())
        } else {
            update_batch_start(
                context,
                self.legacy
                    .as_ref()
                    .context("batch constant buffer missing")?,
                first_instance,
            )
        }
    }
}

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

fn path_target_trace(action: &str, width: u32, height: u32) {
    static ENABLED: OnceLock<bool> = OnceLock::new();
    if *ENABLED.get_or_init(|| std::env::var_os("ARGUS_GPUI_DIAGNOSTICS").is_some()) {
        let bytes =
            u64::from(width) * u64::from(height) * 4 * u64::from(1 + PATH_MULTISAMPLE_COUNT);
        println!("{{\"event\":\"gpu-path-targets\",\"action\":\"{action}\",\"bytes\":{bytes}}}");
    }
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
