//! Rasterize a curve revision once; continuous frames only translate its texture.
use super::*;
use std::hash::{Hash, Hasher};
use std::sync::atomic::{AtomicU64, Ordering};

static BUILDS: AtomicU64 = AtomicU64::new(0);
/// Dash phase (0..period) at each sample time, per series; kept between rasterizations.
pub(in crate::frontend) type DashPhases = Arc<std::sync::Mutex<Vec<HashMap<u64, f32>>>>;
/// Dash and gap lengths of a secondary series, logical pixels.
const DASH: f32 = 4.;
const DASH_GAP: f32 = 3.;
static BUILD_MICROS: AtomicU64 = AtomicU64::new(0);
/// How long a chart's first paint waits for its curve. Every chart's raster
/// starts in prepaint, so they draw in parallel and one wait covers them all;
/// a raster that takes longer appears when it is done.
const FIRST_PAINT_WAIT: Duration = Duration::from_millis(50);

pub(super) fn metrics() -> (u64, u64) {
    (
        BUILDS.load(Ordering::Relaxed),
        BUILD_MICROS.load(Ordering::Relaxed),
    )
}

#[derive(Default)]
struct Cache {
    key: Option<u64>,
    requested: Option<u64>,
    /// The series the latest request shows; a texture of other series is never kept.
    requested_content: Option<u64>,
    pending: Option<Pending>,
    texture: Option<Texture>,
}
/// The raster in flight.
#[derive(Clone)]
struct Pending {
    revision: u64,
    /// The series it draws.
    content: u64,
    started: Instant,
    slot: Arc<Slot>,
}
/// A background raster's result, which a first paint may wait for.
#[derive(Default)]
struct Slot {
    result: std::sync::Mutex<Option<Option<Texture>>>,
    ready: std::sync::Condvar,
}
impl Cache {
    /// Takes a finished raster in, unless the request moved on to other series.
    /// A texture of the same series is kept even when newer data or a new size
    /// was requested meanwhile: it is placed by its own time window and scaled
    /// to the plot until the redraw that follows.
    fn settle(&mut self, slot: &Arc<Slot>, window: &mut Window) {
        let Some(pending) = &self.pending else {
            return;
        };
        if !Arc::ptr_eq(&pending.slot, slot) {
            return;
        }
        let Some(texture) = slot.result.lock().unwrap_or_else(|e| e.into_inner()).take() else {
            return;
        };
        let revision = pending.revision;
        self.pending = None;
        let current = self.requested == Some(revision);
        let same = texture
            .as_ref()
            .is_some_and(|texture| Some(texture.content) == self.requested_content);
        if current || same {
            if let Some(previous) = self.texture.take() {
                let _ = window.drop_image(previous.image);
            }
            self.texture = texture;
            self.key = Some(revision);
        }
    }
    /// Whether a texture of these series is at hand.
    fn shows(&self, content: u64) -> bool {
        self.texture
            .as_ref()
            .is_some_and(|texture| texture.content == content)
    }
}
struct Texture {
    image: Arc<RenderImage>,
    offset: Point<Pixels>,
    size: Size<Pixels>,
    plot_size: Size<Pixels>,
    geometry_start: f64,
    span: f64,
    max: f64,
    content: u64,
}
pub(in crate::frontend) struct ImageSurface {
    owner: EntityId,
    handle: Option<PaintedPrimitive>,
    _image: Arc<RenderImage>,
    offset: Point<Pixels>,
    size: Size<Pixels>,
    plot_size: Size<Pixels>,
    geometry_start: f64,
    span: f64,
    max: f64,
}
impl ImageSurface {
    fn bounds_at(&self, plot: &TimePlot, bounds: Bounds<Pixels>, end: f64) -> Bounds<Pixels> {
        let span = (plot.end - plot.start).max(1.);
        let sx = f32::from(bounds.size.width) / f32::from(self.plot_size.width)
            * (self.span / span) as f32;
        // A rescale easing in stretches the texture about the baseline.
        let k = plot.rescale_factor(self.max, Instant::now());
        let sy = f32::from(bounds.size.height) / f32::from(self.plot_size.height) * k;
        let origin = bounds.origin
            + point(self.offset.x * sx, self.offset.y * sy)
            + point(px(0.), bounds.size.height * (1. - k))
            + point(
                px(((self.geometry_start - (end - span)) / span) as f32
                    * f32::from(bounds.size.width)),
                px(0.),
            );
        Bounds::new(origin, size(self.size.width * sx, self.size.height * sy))
    }
    pub(super) fn translate(
        &self,
        plot: &TimePlot,
        bounds: Bounds<Pixels>,
        end: f64,
        window: &mut Window,
    ) -> bool {
        self.handle.as_ref().is_some_and(|handle| {
            window.update_painted_bounds(handle, self.bounds_at(plot, bounds, end))
        })
    }
}
struct Input {
    series: Vec<Series>,
    geometry_start: f64,
    span: f64,
    min: f64,
    max: f64,
    fill: bool,
    size: Size<Pixels>,
    density: f32,
    stroke: f32,
    smooth: bool,
    phases: DashPhases,
    content: u64,
}
impl Input {
    fn new(plot: &TimePlot, size: Size<Pixels>, density: f32) -> Self {
        Self {
            series: plot.series.clone(),
            geometry_start: plot.geometry_start,
            span: (plot.end - plot.start).max(1.),
            min: plot.min,
            max: plot.max,
            fill: plot.fill,
            size,
            density,
            stroke: f32::from(theme::dimension("chartStrokeWidth")),
            smooth: plot.smooth,
            phases: plot.dash_phases.clone(),
            content: plot.content(),
        }
    }
}

pub(super) fn geometry_key(runs: &[Vec<(f64, f64)>]) -> u64 {
    let mut key = std::collections::hash_map::DefaultHasher::new();
    for run in runs {
        run.len().hash(&mut key);
        for (x, y) in run {
            x.to_bits().hash(&mut key);
            y.to_bits().hash(&mut key);
        }
    }
    key.finish()
}

fn key(plot: &TimePlot, size: Size<Pixels>, density: f32) -> u64 {
    let mut key = std::collections::hash_map::DefaultHasher::new();
    for value in [
        plot.geometry_start,
        plot.end - plot.start,
        plot.min,
        plot.max,
    ] {
        value.to_bits().hash(&mut key);
    }
    for value in [
        f32::from(size.width),
        f32::from(size.height),
        density,
        f32::from(theme::dimension("chartStrokeWidth")),
    ] {
        value.to_bits().hash(&mut key);
    }
    plot.fill.hash(&mut key);
    plot.smooth.hash(&mut key);
    plot.content().hash(&mut key);
    for series in &plot.series {
        series.geometry_key.hash(&mut key);
        series.dashed.hash(&mut key);
        series.fill.to_bits().hash(&mut key);
        for value in [
            series.color.h,
            series.color.s,
            series.color.l,
            series.color.a,
        ] {
            value.to_bits().hash(&mut key);
        }
    }
    key.finish()
}

fn density(window: &Window) -> f32 {
    window.scale_factor()
        * theme::design()["layout"]["chartRasterScale"]
            .as_f64()
            .expect("chart raster scale") as f32
}

fn cache(plot: &TimePlot, window: &mut Window, cx: &mut App) -> Entity<Cache> {
    window.use_keyed_state("argus-curve-texture", cx, |window, cx| {
        let handle = window.window_handle();
        let surface = Rc::downgrade(&plot.image_surface);
        let owner = cx.entity().entity_id();
        cx.on_release(move |cache: &mut Cache, cx| {
            if let Some(surface) = surface.upgrade() {
                let owned = surface
                    .borrow()
                    .as_ref()
                    .is_some_and(|image| image.owner == owner);
                if owned {
                    surface.borrow_mut().take();
                }
            }
            if let Some(texture) = cache.texture.take() {
                cx.defer(move |cx| {
                    let _ = handle.update(cx, |_, window, cx| {
                        cx.drop_image(texture.image, Some(window))
                    });
                });
            }
        })
        .detach();
        Cache::default()
    })
}

/// Requests the texture for the plot as laid out now: a raster of a new revision
/// starts in the background, unless one of these series is already in flight
/// (one of other series, such as the previous device's, is superseded and its
/// result dropped). Called from prepaint, so every chart on screen starts
/// before the first one paints.
pub(super) fn prepare(plot: &TimePlot, bounds: Bounds<Pixels>, window: &mut Window, cx: &mut App) {
    let density = density(window);
    let revision = key(plot, bounds.size, density);
    let cache = cache(plot, window, cx);
    cache.update(cx, |cache, cx| {
        let content = plot.content();
        cache.requested = Some(revision);
        cache.requested_content = Some(content);
        if cache.key == Some(revision)
            || cache
                .pending
                .as_ref()
                .is_some_and(|pending| pending.content == content)
        {
            return;
        }
        let slot = Arc::new(Slot::default());
        cache.pending = Some(Pending {
            revision,
            content,
            started: Instant::now(),
            slot: slot.clone(),
        });
        let input = Input::new(plot, bounds.size, density);
        let landing = slot.clone();
        let task = cx.background_executor().spawn(async move {
            let start = Instant::now();
            let texture = rasterize(&input);
            BUILDS.fetch_add(1, Ordering::Relaxed);
            BUILD_MICROS.fetch_add(start.elapsed().as_micros() as u64, Ordering::Relaxed);
            *landing.result.lock().unwrap_or_else(|e| e.into_inner()) = Some(texture);
            landing.ready.notify_all();
        });
        cx.spawn_in(window, async move |cache, cx| {
            task.await;
            let _ = cache.update_in(cx, |cache, window, cx| {
                cache.settle(&slot, window);
                cx.notify();
            });
        })
        .detach();
    });
}

pub(super) fn paint(plot: &TimePlot, bounds: Bounds<Pixels>, window: &mut Window, cx: &mut App) {
    prepare(plot, bounds, window, cx);
    let cache = cache(plot, window, cx);
    cache.update(cx, |cache, cx| {
        // Nothing of these series yet (a first display, another device): wait a
        // little for the raster rather than show the chart without its curve.
        if !cache.shows(plot.content())
            && let Some(Pending { started, slot, .. }) = cache.pending.clone()
        {
            let deadline = started + FIRST_PAINT_WAIT;
            let mut result = slot.result.lock().unwrap_or_else(|e| e.into_inner());
            while result.is_none() {
                let left = deadline.saturating_duration_since(Instant::now());
                if left.is_zero() {
                    break;
                }
                result = slot
                    .ready
                    .wait_timeout(result, left)
                    .unwrap_or_else(|e| e.into_inner())
                    .0;
            }
            drop(result);
            cache.settle(&slot, window);
        }
        let Some(texture) = cache
            .texture
            .as_ref()
            .filter(|texture| texture.content == plot.content())
        else {
            // Nothing of these series yet; never show another device's curve.
            plot.image_surface.borrow_mut().take();
            return;
        };
        let mut surface = ImageSurface {
            owner: cx.entity().entity_id(),
            handle: None,
            _image: texture.image.clone(),
            offset: texture.offset,
            size: texture.size,
            plot_size: texture.plot_size,
            geometry_start: texture.geometry_start,
            span: texture.span,
            max: texture.max,
        };
        let image_bounds = surface.bounds_at(plot, bounds, plot.end);
        window.with_content_mask(Some(ContentMask { bounds }), |window| {
            match window.paint_image_subpixel(image_bounds, texture.image.clone(), 0) {
                Ok(handle) => {
                    surface.handle = handle;
                }
                Err(error) => {
                    argus_core::diagnostics::warn(
                        "chart texture paint",
                        argus_core::diagnostics::error_chain(error.as_ref()),
                    );
                }
            }
        });
        *plot.image_surface.borrow_mut() = Some(surface);
    });
}

fn curve(points: &[(f64, f64)], baseline: Option<f32>, smooth: bool) -> Option<tiny_skia::Path> {
    if points.len() < 2 {
        return None;
    }
    let tangent = if smooth {
        super::curve_tangents(points)
    } else {
        Vec::new()
    };
    let mut path = tiny_skia::PathBuilder::new();
    let (x, y) = points[0];
    if let Some(base) = baseline {
        path.move_to(x as f32, base);
        path.line_to(x as f32, y as f32);
    } else {
        path.move_to(x as f32, y as f32);
    }
    for i in 0..points.len() - 1 {
        let (x, y) = points[i];
        let (next_x, next_y) = points[i + 1];
        if !smooth {
            path.line_to(next_x as f32, next_y as f32);
            continue;
        }
        let third = (next_x - x) / 3.;
        path.cubic_to(
            (x + third) as f32,
            (y + tangent[i] * third) as f32,
            (next_x - third) as f32,
            (next_y - tangent[i + 1] * third) as f32,
            next_x as f32,
            next_y as f32,
        );
    }
    if let Some(base) = baseline {
        path.line_to(points.last()?.0 as f32, base);
        path.close();
    }
    path.finish()
}

/// Approximate arc length of each cubic segment between consecutive points.
fn segment_lengths(points: &[(f64, f64)]) -> Vec<f32> {
    if points.len() < 2 {
        return Vec::new();
    }
    let tangent = super::curve_tangents(points);
    points
        .windows(2)
        .enumerate()
        .map(|(i, pair)| {
            let ((x0, y0), (x1, y1)) = (pair[0], pair[1]);
            let third = (x1 - x0) / 3.;
            let (c1, c2) = (y0 + tangent[i] * third, y1 - tangent[i + 1] * third);
            let mut length = 0.;
            let mut previous = (x0, y0);
            const STEPS: usize = 8;
            for step in 1..=STEPS {
                let t = step as f64 / STEPS as f64;
                let u = 1. - t;
                let x = x0 + (x1 - x0) * t;
                let y = u * u * u * y0 + 3. * u * u * t * c1 + 3. * u * t * t * c2 + t * t * t * y1;
                length += ((x - previous.0).powi(2) + (y - previous.1).powi(2)).sqrt();
                previous = (x, y);
            }
            length as f32
        })
        .collect()
}

fn rasterize(plot: &Input) -> Option<Texture> {
    let size = plot.size;
    let density = plot.density;
    let width = f32::from(size.width);
    let height = f32::from(size.height);
    if width <= 0. || height <= 0. {
        return None;
    }
    let span = plot.span;
    let scale = (plot.max - plot.min).max(f64::EPSILON);
    let stroke = plot.stroke;
    let left = plot
        .series
        .iter()
        .flat_map(|s| s.geometry.iter().filter_map(|r| r.first()))
        .map(|p| ((p.0 - plot.geometry_start) / span) as f32 * width)
        .fold(0., f32::min)
        - stroke;
    let right = plot
        .series
        .iter()
        .flat_map(|s| s.geometry.iter().filter_map(|r| r.last()))
        .map(|p| ((p.0 - plot.geometry_start) / span) as f32 * width)
        .fold(width, f32::max)
        + stroke;
    let pixel_width = ((right - left) * density).ceil() as u32;
    let pixel_height = (height * density).ceil() as u32;
    let mut pixels = tiny_skia::Pixmap::new(pixel_width, pixel_height)?;
    let transform = tiny_skia::Transform::from_scale(density, density);
    let top = stroke / 2.;
    // Areas fade from their opacity at the chart's top to nothing at its bottom:
    // a solid fill through this row mask, which costs a fraction of a gradient
    // shader's per-pixel work.
    let fade = if plot.fill && plot.series.iter().any(|series| series.fill > 0.) {
        let mut mask = tiny_skia::Mask::new(pixel_width, pixel_height)?;
        let row = pixel_width as usize;
        for (index, line) in mask.data_mut().chunks_exact_mut(row).enumerate() {
            let y = (index as f32 + 0.5) / density;
            let level = ((height - y) / (height - top)).clamp(0., 1.);
            // Down to nothing at the bottom, as the original's gradient. The
            // renderer averages premultiplied texels (see `paint_image_subpixel`),
            // so the noisy straight colour of the faintest rows stays invisible.
            line.fill((level * 255. + 0.5) as u8);
        }
        Some(mask)
    } else {
        None
    };
    let mut phases = plot.phases.lock().unwrap_or_else(|e| e.into_inner());
    phases.resize_with(plot.series.len(), HashMap::new);
    for (index, series) in plot.series.iter().enumerate() {
        let color: Rgba = series.color.into();
        let solid = tiny_skia::Color::from_rgba(color.r, color.g, color.b, color.a)?;
        let mut next_phases = HashMap::new();
        for run in series.geometry.iter() {
            let points = run
                .iter()
                .map(|&(at, value)| {
                    (
                        ((at - plot.geometry_start) / span) * width as f64 - left as f64,
                        height as f64 - (value - plot.min) / scale * (height - top) as f64,
                    )
                })
                .collect::<Vec<_>>();
            if let Some(fade) = &fade
                && series.fill > 0.
                && let Some(path) = curve(&points, Some(height), plot.smooth)
            {
                let area = tiny_skia::Color::from_rgba(
                    color.r,
                    color.g,
                    color.b,
                    color.a * series.fill.clamp(0., 1.),
                )?;
                pixels.fill_path(
                    &path,
                    &tiny_skia::Paint {
                        shader: tiny_skia::Shader::SolidColor(area),
                        anti_alias: true,
                        ..Default::default()
                    },
                    tiny_skia::FillRule::Winding,
                    transform,
                    Some(fade),
                );
            }
            let dash = if series.dashed {
                // Dashes run along the curve. Start the pattern so that a sample drawn
                // last time keeps its phase; otherwise they would crawl as the oldest
                // sample drops off.
                let period = DASH + DASH_GAP;
                let lengths = segment_lengths(&points);
                let mut start = 0.;
                let mut along = 0.;
                for (i, &(at, _)) in run.iter().enumerate() {
                    if let Some(phase) = phases[index].get(&at.to_bits()) {
                        start = phase - along;
                        break;
                    }
                    along += lengths.get(i).copied().unwrap_or(0.);
                }
                let start = start.rem_euclid(period);
                let mut along = start;
                for (i, &(at, _)) in run.iter().enumerate() {
                    next_phases.insert(at.to_bits(), along.rem_euclid(period));
                    along += lengths.get(i).copied().unwrap_or(0.);
                }
                tiny_skia::StrokeDash::new(vec![DASH, DASH_GAP], start)
            } else {
                None
            };
            if let Some(path) = curve(&points, None, plot.smooth) {
                pixels.stroke_path(
                    &path,
                    &tiny_skia::Paint {
                        shader: tiny_skia::Shader::SolidColor(solid),
                        anti_alias: true,
                        ..Default::default()
                    },
                    &tiny_skia::Stroke {
                        width: stroke,
                        line_join: tiny_skia::LineJoin::Round,
                        dash,
                        ..Default::default()
                    },
                    transform,
                    None,
                );
            }
        }
        if series.dashed {
            phases[index] = next_phases;
        }
    }
    drop(phases);
    let buffer =
        image::RgbaImage::from_raw(pixel_width, pixel_height, straight_bgra(pixels.data()))?;
    Some(Texture {
        image: Arc::new(RenderImage::new(vec![image::Frame::new(buffer)])),
        offset: point(px(left), px(0.)),
        size: gpui_kit::gpui::size(
            px(pixel_width as f32 / density),
            px(pixel_height as f32 / density),
        ),
        plot_size: size,
        geometry_start: plot.geometry_start,
        span,
        max: plot.max,
        content: plot.content,
    })
}

/// tiny-skia's premultiplied RGBA as the straight BGRA a RenderImage holds.
/// Most pixels are empty or repeat their neighbour (an area's row shares one
/// colour), so the division runs only where the colour changes.
fn straight_bgra(rgba: &[u8]) -> Vec<u8> {
    let mut bgra = vec![0; rgba.len()];
    let mut last = ([0u8; 4], [0u8; 4]);
    let (pixels, _) = rgba.as_chunks::<4>();
    for (out, &pixel) in bgra.as_chunks_mut::<4>().0.iter_mut().zip(pixels) {
        if pixel[3] == 0 {
            continue;
        }
        if pixel != last.0 {
            // The same rounding as tiny-skia's own demultiply.
            let alpha = f64::from(pixel[3]) / 255.;
            let straight = |c: u8| (f64::from(c) / alpha + 0.5) as u8;
            last = (
                pixel,
                [
                    straight(pixel[2]),
                    straight(pixel[1]),
                    straight(pixel[0]),
                    pixel[3],
                ],
            );
        }
        *out = last.1;
    }
    bgra
}

#[cfg(test)]
mod tests {
    use super::{Input, key, rasterize, straight_bgra};
    #[test]
    fn straight_alpha_matches_tiny_skia() {
        for alpha in 0..=255u8 {
            for value in 0..=alpha {
                let premultiplied =
                    tiny_skia::PremultipliedColorU8::from_rgba(value, 0, value / 2, alpha).unwrap();
                let expected = premultiplied.demultiply();
                let pixel = [
                    premultiplied.red(),
                    premultiplied.green(),
                    premultiplied.blue(),
                    alpha,
                ];
                assert_eq!(
                    straight_bgra(&pixel),
                    [
                        expected.blue(),
                        expected.green(),
                        expected.red(),
                        expected.alpha()
                    ],
                    "value {value} alpha {alpha}"
                );
            }
        }
    }
    use crate::frontend::charts::{MetricFormat, Series, TimePlot};
    use gpui_kit::gpui::{px, rgb, size};
    use std::{cell::Cell, rc::Rc};
    fn plot(points: Vec<(f64, Option<f64>)>) -> TimePlot {
        TimePlot {
            id: "raster-test".into(),
            series: vec![Series::new(
                String::new(),
                points,
                rgb(0x28b8e0).into(),
                MetricFormat::Percent,
            )],
            start: 0.,
            end: 100.,
            max: 100.,
            min: 0.,
            interactive: false,
            grid: false,
            fill: false,
            bars: false,
            background_separate: false,
            smooth: true,
            ticks: None,
            geometry_start: 0.,
            measured_width: Rc::new(Cell::new(100.)),
            live_clock: None,
            image_surface: Default::default(),
            bar_surfaces: Default::default(),
            bar_glides: Default::default(),
            dash_phases: Default::default(),
            rescale: Default::default(),
        }
    }
    #[test]
    fn continuous_scroll_reuses_texture_while_data_and_dpi_invalidate_it() {
        let mut p = plot(vec![(0., Some(30.)), (100., Some(60.))]);
        let size = size(px(100.), px(40.));
        let before = key(&p, size, 2.);
        p.start += 0.125;
        p.end += 0.125;
        assert_eq!(before, key(&p, size, 2.));
        assert_ne!(before, key(&p, size, 3.));
        p.series = vec![Series::new(
            String::new(),
            vec![(0., Some(30.)), (100., Some(65.))],
            rgb(0x28b8e0).into(),
            MetricFormat::Percent,
        )];
        assert_ne!(before, key(&p, size, 2.));
    }
    #[test]
    fn raster_keeps_unknown_spans_transparent() {
        let p = plot(vec![
            (0., Some(50.)),
            (20., Some(50.)),
            (40., None),
            (60., Some(50.)),
            (80., Some(50.)),
        ]);
        let texture = rasterize(&Input::new(&p, size(px(100.), px(40.)), 2.)).unwrap();
        let width = texture.image.size(0).width.0 as usize;
        let bytes = texture.image.as_bytes(0).unwrap();
        let alpha = |x: f32| {
            let x = ((x - f32::from(texture.offset.x)) * 2.) as usize;
            (39..44)
                .map(|y| bytes[(y * width + x) * 4 + 3])
                .max()
                .unwrap()
        };
        assert!(alpha(10.) > 128);
        assert_eq!(alpha(40.), 0);
        assert!(alpha(70.) > 128);
    }
}
