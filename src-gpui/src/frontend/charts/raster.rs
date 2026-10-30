//! Rasterize a curve revision once; continuous frames only translate its texture.
use super::*;
use std::hash::{Hash, Hasher};
use std::sync::atomic::{AtomicU64, Ordering};

static BUILDS: AtomicU64 = AtomicU64::new(0);
/// Where each sample sits in the dash pattern, by sample time, per series:
/// unwrapped, in logical pixels along the pattern (see `run_phases`). Kept
/// between rasterizations, so that the dashes stay where they were drawn.
pub(in crate::frontend) type DashPhases = Arc<std::sync::Mutex<Vec<HashMap<u64, f64>>>>;
/// Dash and gap lengths of a secondary series, logical pixels.
const DASH: f64 = 4.;
const DASH_GAP: f64 = 3.;
const DASH_PERIOD: f64 = DASH + DASH_GAP;
/// How far a segment's dashes may stretch or shrink to keep the phases its
/// samples were drawn at, when the segment changed length: it bends anew
/// when a neighbouring sample arrives or drops off, and every segment does
/// when the scale changes. Beyond this the segment takes its own length.
const DASH_STRETCH: f64 = 0.25;
static BUILD_MICROS: AtomicU64 = AtomicU64::new(0);
/// A chart shown for the first time (entering a page, another device or view)
/// waits in paint for its curve, so that the curve is part of the first frame
/// of the page around it. The first texture is needed before that
/// frame. Every chart's raster starts in prepaint and they draw in parallel,
/// a large one on several threads (`thread_count`), so one short wait covers
/// them all. This only bounds a raster that does not land.
const FIRST_PAINT_WAIT: Duration = Duration::from_millis(250);

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
    min: f64,
    max: f64,
    /// Where the top of the range was drawn, below the plot's top edge.
    top: f32,
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
    min: f64,
    max: f64,
    top: f32,
}
impl ImageSurface {
    /// The height on screen at which the curve this texture holds passes
    /// through `value`: laid out as the raster laid it out, then moved and
    /// stretched as `bounds_at` places the texture (a rescale easing in).
    pub(super) fn curve_y(&self, plot: &TimePlot, bounds: Bounds<Pixels>, value: f64) -> Pixels {
        let k = plot.rescale_factor(self.max, Instant::now());
        let sy = f32::from(bounds.size.height) / f32::from(self.plot_size.height) * k;
        let height = f64::from(f32::from(self.plot_size.height));
        let drawn = height
            - (value - self.min) / (self.max - self.min).max(f64::EPSILON)
                * (height - f64::from(self.top));
        bounds.top() + self.offset.y * sy + bounds.size.height * (1. - k) + px(drawn as f32 * sy)
    }
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
        // Nothing of these series yet (a first display, another device): wait
        // for the raster rather than show the chart without its curve.
        if !cache.shows(plot.content())
            && let Some(Pending { started, slot, .. }) = cache.pending.clone()
        {
            let deadline = started + FIRST_PAINT_WAIT;
            let waited = Instant::now();
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
            let landed = result.is_some();
            drop(result);
            if gpui_kit::gpui::profiler::phase_trace_enabled() {
                println!(
                    "{}",
                    serde_json::json!({"event":"chart-wait","us":waited.elapsed().as_micros(),
                    "landed":landed,"atQpc":gpui_kit::gpui::profiler::trace_clock_100ns()})
                );
            }
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
            min: texture.min,
            max: texture.max,
            top: texture.top,
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

/// The hover readout's dot: 9 px across, the series colour inside a 2 px ring
/// of the tooltip's colour, as the interface's `.chart-dot`.
const DOT: f32 = 9.;
const DOT_RING: f32 = 2.;

/// Paints a readout dot centred exactly at `center`. The curve under it moves
/// by fractions of a device pixel from frame to frame, while laid-out boxes
/// and quads snap to whole device pixels: a dot drawn that way steps a pixel
/// at a time along a gently sloping curve and wobbles about the line. Drawn
/// instead as an image at the curve texture's density and sampled by area
/// like it (`paint_image_subpixel`), the dot keeps its exact place and moves
/// as smoothly as the curve.
pub(super) fn paint_dot(window: &mut Window, center: Point<Pixels>, fill: Hsla, ring: Hsla) {
    let density = density(window);
    let Some(image) = dot_image(fill, ring, density) else {
        return;
    };
    let side = px(image.size(0).width.0 as f32 / density);
    let bounds = Bounds::new(center - point(side / 2., side / 2.), size(side, side));
    if let Err(error) = window.paint_image_subpixel(bounds, image, 0) {
        argus_core::diagnostics::warn(
            "chart dot paint",
            argus_core::diagnostics::error_chain(error.as_ref()),
        );
    }
}

/// A dot's image, drawn once per colour pair and density: with a clear margin
/// of a pixel, so that area sampling at any offset never cuts its edge.
fn dot_image(fill: Hsla, ring: Hsla, density: f32) -> Option<Arc<RenderImage>> {
    thread_local! {
        static DOTS: RefCell<HashMap<u64, Arc<RenderImage>>> = RefCell::new(HashMap::new());
    }
    let (fill, ring): (Rgba, Rgba) = (fill.into(), ring.into());
    let key = super::hash_of(
        [
            fill.r, fill.g, fill.b, fill.a, ring.r, ring.g, ring.b, ring.a, density,
        ]
        .map(f32::to_bits),
    );
    if let Some(image) = DOTS.with_borrow(|dots| dots.get(&key).cloned()) {
        return Some(image);
    }
    let side = ((DOT + 2.) * density).ceil() as u32;
    let mut pixmap = tiny_skia::Pixmap::new(side, side)?;
    let middle = side as f32 / 2.;
    let outer = DOT / 2. * density;
    let inner = (DOT / 2. - DOT_RING) * density;
    let paint = |color: Rgba| -> Option<tiny_skia::Paint<'static>> {
        Some(tiny_skia::Paint {
            shader: tiny_skia::Shader::SolidColor(tiny_skia::Color::from_rgba(
                color.r, color.g, color.b, color.a,
            )?),
            anti_alias: true,
            ..Default::default()
        })
    };
    let disc = tiny_skia::PathBuilder::from_circle(middle, middle, outer)?;
    // The fill runs under the ring, which is drawn over it, as a CSS border
    // over its box's background.
    let ring_path = {
        let mut path = tiny_skia::PathBuilder::new();
        path.push_circle(middle, middle, outer);
        path.push_circle(middle, middle, inner);
        path.finish()?
    };
    for (path, color, rule) in [
        (&disc, fill, tiny_skia::FillRule::Winding),
        (&ring_path, ring, tiny_skia::FillRule::EvenOdd),
    ] {
        pixmap.fill_path(
            path,
            &paint(color)?,
            rule,
            tiny_skia::Transform::identity(),
            None,
        );
    }
    let mut pixels = pixmap.take();
    straight_bgra(&mut pixels);
    let buffer = image::RgbaImage::from_raw(side, side, pixels)?;
    let image = Arc::new(RenderImage::new(vec![image::Frame::new(buffer)]));
    DOTS.with_borrow_mut(|dots| dots.insert(key, image.clone()));
    Some(image)
}

/// One piece of a curve between two samples, as a cubic Bezier (start,
/// control, control, end) in texture coordinates.
#[derive(Clone, Copy)]
struct Cubic([(f64, f64); 4]);
impl Cubic {
    /// The pieces of the curve through `points`: the monotone cubic, whose
    /// control points sit at thirds of each segment so that x runs linearly
    /// with the parameter, or straight lines.
    fn through(points: &[(f64, f64)], smooth: bool) -> Vec<Self> {
        if points.len() < 2 {
            return Vec::new();
        }
        let tangent = if smooth {
            super::curve_tangents(points)
        } else {
            vec![0.; points.len()]
        };
        points
            .windows(2)
            .enumerate()
            .map(|(i, pair)| {
                let ((x, y), (next_x, next_y)) = (pair[0], pair[1]);
                let third = (next_x - x) / 3.;
                let (c1, c2) = if smooth {
                    (y + tangent[i] * third, next_y - tangent[i + 1] * third)
                } else {
                    (y + (next_y - y) / 3., next_y - (next_y - y) / 3.)
                };
                Self([
                    (x, y),
                    (x + third, c1),
                    (next_x - third, c2),
                    (next_x, next_y),
                ])
            })
            .collect()
    }
    fn at(&self, t: f64) -> (f64, f64) {
        let [p0, p1, p2, p3] = self.0;
        let u = 1. - t;
        let (a, b, c, d) = (u * u * u, 3. * u * u * t, 3. * u * t * t, t * t * t);
        (
            a * p0.0 + b * p1.0 + c * p2.0 + d * p3.0,
            a * p0.1 + b * p1.1 + c * p2.1 + d * p3.1,
        )
    }
    /// Both halves at `t` (de Casteljau).
    fn split(&self, t: f64) -> (Self, Self) {
        let lerp = |a: (f64, f64), b: (f64, f64)| (a.0 + (b.0 - a.0) * t, a.1 + (b.1 - a.1) * t);
        let [p0, p1, p2, p3] = self.0;
        let (a, b, c) = (lerp(p0, p1), lerp(p1, p2), lerp(p2, p3));
        let (d, e) = (lerp(a, b), lerp(b, c));
        let f = lerp(d, e);
        (Self([p0, a, d, f]), Self([f, e, c, p3]))
    }
    /// The part from `t0` to `t1`: the same curve, as a cubic of its own.
    fn part(&self, t0: f64, t1: f64) -> Self {
        let head = if t1 < 1. { self.split(t1).0 } else { *self };
        if t0 <= 0. || t1 <= 0. {
            head
        } else {
            head.split((t0 / t1).min(1.)).1
        }
    }
}

/// A cubic and its arc length at evenly spaced parameters about a texel
/// apart, to cut dashes by length. The dashes are cut here rather than by
/// tiny-skia, whose own length estimate differs by a fraction of a pixel:
/// phases measured here then repeat exactly when the curve is drawn again.
struct Measured {
    cubic: Cubic,
    lengths: Vec<f64>,
}
impl Measured {
    fn new(cubic: Cubic, density: f64) -> Self {
        let [p0, p1, p2, p3] = cubic.0;
        let distance = |a: (f64, f64), b: (f64, f64)| (b.0 - a.0).hypot(b.1 - a.1);
        let hull = distance(p0, p1) + distance(p1, p2) + distance(p2, p3);
        let pieces = ((hull * density).ceil() as usize).clamp(4, 4096);
        let mut lengths = Vec::with_capacity(pieces + 1);
        lengths.push(0.);
        let mut previous = p0;
        let mut length = 0.;
        for step in 1..=pieces {
            let point = cubic.at(step as f64 / pieces as f64);
            length += distance(previous, point);
            lengths.push(length);
            previous = point;
        }
        Self { cubic, lengths }
    }
    fn length(&self) -> f64 {
        self.lengths.last().copied().unwrap_or(0.)
    }
    /// The parameter `along` the curve from its start.
    fn t_at(&self, along: f64) -> f64 {
        let pieces = self.lengths.len() - 1;
        let index = self
            .lengths
            .partition_point(|&l| l < along)
            .clamp(1, pieces);
        let (a, b) = (self.lengths[index - 1], self.lengths[index]);
        let f = if b > a {
            ((along - a) / (b - a)).clamp(0., 1.)
        } else {
            0.
        };
        (index as f64 - 1. + f) / pieces as f64
    }
}

/// Where each sample of a run sits in the dash pattern, so that the dashes
/// stay on the curve like a printed pattern as it scrolls and is drawn again
/// for each sample. A sample drawn before keeps its phase whenever the
/// segment from the previous sample can take it by stretching its dashes at
/// most `DASH_STRETCH` (the phase counts modulo the period): a segment that
/// did not change keeps its dashes exactly, one bent anew by a neighbouring
/// sample changes only within itself, and nothing beyond it moves. Other
/// samples (new ones, or past a segment that changed too much) continue the
/// pattern along the curve.
fn run_phases(times: &[f64], lengths: &[f64], known: &HashMap<u64, f64>) -> Vec<f64> {
    let n = times.len();
    let drawn = |i: usize| known.get(&times[i].to_bits()).copied();
    let mut phases = vec![0.; n];
    let Some(first) = (0..n).find(|&i| drawn(i).is_some()) else {
        for i in 1..n {
            phases[i] = phases[i - 1] + lengths[i - 1];
        }
        return phases;
    };
    phases[first] = drawn(first).unwrap_or(0.);
    for i in (0..first).rev() {
        phases[i] = phases[i + 1] - lengths[i];
    }
    for i in first..n - 1 {
        let natural = phases[i] + lengths[i];
        phases[i + 1] = drawn(i + 1)
            .map(|phase| phase + ((natural - phase) / DASH_PERIOD).round() * DASH_PERIOD)
            .filter(|phase| {
                let stretch = (phase - phases[i]) / lengths[i];
                lengths[i] > 0. && (stretch - 1.).abs() <= DASH_STRETCH
            })
            .unwrap_or(natural);
    }
    phases
}

/// The dashes of a run, cut from its cubics: segment `i` runs through the
/// pattern from `phases[i]` to `phases[i + 1]`, spread evenly along it. A dash
/// that runs on past a sample continues the same contour, so the stroke
/// joins there as the solid line does.
fn dashes(segments: &[Measured], phases: &[f64]) -> Option<tiny_skia::Path> {
    let mut path = tiny_skia::PathBuilder::new();
    let mut open = false;
    for (i, segment) in segments.iter().enumerate() {
        let (from, to) = (phases[i], phases[i + 1]);
        let length = segment.length();
        if !(to > from && length > 0.) {
            open = false;
            continue;
        }
        let scale = length / (to - from);
        let mut reached = false;
        let mut first = true;
        let mut k = (from / DASH_PERIOD).floor();
        while k * DASH_PERIOD < to {
            let (a, b) = (
                (k * DASH_PERIOD).max(from),
                (k * DASH_PERIOD + DASH).min(to),
            );
            k += 1.;
            if b - a <= 1e-9 {
                continue;
            }
            let piece = segment.cubic.part(
                segment.t_at((a - from) * scale),
                segment.t_at((b - from) * scale),
            );
            let [start, c1, c2, end] = piece.0.map(|(x, y)| (x as f32, y as f32));
            if !(first && open && a <= from) {
                path.move_to(start.0, start.1);
            }
            path.cubic_to(c1.0, c1.1, c2.0, c2.1, end.0, end.1);
            first = false;
            reached = b >= to;
        }
        open = reached;
    }
    path.finish()
}

fn curve(points: &[(f64, f64)], baseline: Option<f32>, smooth: bool) -> Option<tiny_skia::Path> {
    if points.len() < 2 {
        return None;
    }
    let mut path = tiny_skia::PathBuilder::new();
    let (x, y) = points[0];
    if let Some(base) = baseline {
        path.move_to(x as f32, base);
        path.line_to(x as f32, y as f32);
    } else {
        path.move_to(x as f32, y as f32);
    }
    for cubic in Cubic::through(points, smooth) {
        let [_, c1, c2, end] = cubic.0.map(|(x, y)| (x as f32, y as f32));
        if smooth {
            path.cubic_to(c1.0, c1.1, c2.0, c2.1, end.0, end.1);
        } else {
            path.line_to(end.0, end.1);
        }
    }
    if let Some(base) = baseline {
        path.line_to(points.last()?.0 as f32, base);
        path.close();
    }
    path.finish()
}

/// What a raster draws, built once and drawn into every band in order.
enum Shape {
    /// A filled path; an area fades through the row mask.
    Fill {
        path: tiny_skia::Path,
        color: tiny_skia::Color,
        faded: bool,
    },
    /// A line too thin to outline once (a hairline at this density).
    Stroke {
        path: tiny_skia::Path,
        color: tiny_skia::Color,
        stroke: tiny_skia::Stroke,
    },
}

/// A raster this large is drawn on several threads, one for at least this
/// many pixels: a chart shown for the first time waits for its curve, and a
/// large one (a big window, a high display density) took over 100 ms on one
/// thread. Smaller ones stay on one thread, where starting others costs more
/// than it saves.
const THREAD_PIXELS: usize = 1 << 17;
/// At most this many threads per raster, leaving cores to the rest of the app.
const MAX_THREADS: usize = 8;
/// Bands per thread, taken in turn down the texture: an area fills the lower
/// rows far more than the upper ones, and one band per thread left the lowest
/// to do most of the work.
const BANDS_PER_THREAD: usize = 2;

fn thread_count(width: u32, height: u32) -> usize {
    static CORES: std::sync::OnceLock<usize> = std::sync::OnceLock::new();
    let cores = *CORES.get_or_init(|| {
        std::thread::available_parallelism().map_or(1, |n| n.get().clamp(1, MAX_THREADS))
    });
    (width as usize * height as usize / THREAD_PIXELS).clamp(1, cores)
}

/// Where the texture starts left of the plot and its size in device pixels:
/// wide enough for every run and the line's width at both ends.
fn extent(plot: &Input) -> Option<(f32, u32, u32)> {
    let width = f32::from(plot.size.width);
    let height = f32::from(plot.size.height);
    if width <= 0. || height <= 0. {
        return None;
    }
    let span = plot.span;
    let first = plot
        .series
        .iter()
        .flat_map(|s| s.geometry.iter().filter_map(|r| r.first()))
        .map(|p| ((p.0 - plot.geometry_start) / span) as f32 * width)
        .fold(0., f32::min)
        - plot.stroke;
    // Time `t` lands on texture column ((t - start) / span * width - left) *
    // density. Moving `left` back to keep (start / span * width + left) *
    // density whole puts every time on the same fraction of a texel in every
    // raster: a redraw for a new sample then repeats the texels already on
    // screen instead of antialiasing each point and dash end afresh.
    let density = f64::from(plot.density);
    let fraction = (plot.geometry_start / span * f64::from(width) * density).rem_euclid(1.);
    let left = (((f64::from(first) * density + fraction).floor() - fraction) / density) as f32;
    let right = plot
        .series
        .iter()
        .flat_map(|s| s.geometry.iter().filter_map(|r| r.last()))
        .map(|p| ((p.0 - plot.geometry_start) / span) as f32 * width)
        .fold(width, f32::max)
        + plot.stroke;
    let pixel_width = ((right - left) * plot.density).ceil() as u32;
    let pixel_height = (height * plot.density).ceil() as u32;
    (pixel_width > 0 && pixel_height > 0).then_some((left, pixel_width, pixel_height))
}

fn rasterize(plot: &Input) -> Option<Texture> {
    let (_, pixel_width, pixel_height) = extent(plot)?;
    rasterize_in(plot, thread_count(pixel_width, pixel_height))
}

/// Draws the plot on `threads` threads, each drawing some horizontal bands of
/// one buffer. A band draws every shape moved up by its first row and clipped
/// to its rows, as tiny-skia tiles a pixmap too large for it.
fn rasterize_in(plot: &Input, threads: usize) -> Option<Texture> {
    let started = Instant::now();
    let (left, pixel_width, pixel_height) = extent(plot)?;
    let size = plot.size;
    let density = plot.density;
    let width = f32::from(size.width);
    let height = f32::from(size.height);
    let span = plot.span;
    let scale = (plot.max - plot.min).max(f64::EPSILON);
    let stroke = plot.stroke;
    let transform = tiny_skia::Transform::from_scale(density, density);
    let resolution = tiny_skia::PathStroker::compute_resolution_scale(&transform);
    let top = stroke / 2.;
    let fill = plot.fill && plot.series.iter().any(|series| series.fill > 0.);
    let mut shapes = Vec::new();
    let mut drawn_phases = plot.phases.lock().unwrap_or_else(|e| e.into_inner());
    drawn_phases.resize_with(plot.series.len(), HashMap::new);
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
            if fill
                && series.fill > 0.
                && let Some(path) = curve(&points, Some(height), plot.smooth)
            {
                shapes.push(Shape::Fill {
                    path,
                    color: tiny_skia::Color::from_rgba(
                        color.r,
                        color.g,
                        color.b,
                        color.a * series.fill.clamp(0., 1.),
                    )?,
                    faded: true,
                });
            }
            let line = if series.dashed {
                // Dashes run along the curve, so they keep their length on
                // steep spikes, and keep the phases their samples were drawn
                // at (`run_phases`), so they stay put as the curve scrolls.
                let segments = Cubic::through(&points, plot.smooth)
                    .into_iter()
                    .map(|cubic| Measured::new(cubic, f64::from(density)))
                    .collect::<Vec<_>>();
                let lengths = segments.iter().map(Measured::length).collect::<Vec<_>>();
                let times = run.iter().map(|&(at, _)| at).collect::<Vec<_>>();
                let phases = run_phases(&times, &lengths, &drawn_phases[index]);
                next_phases.extend(
                    times
                        .iter()
                        .map(|at| at.to_bits())
                        .zip(phases.iter().copied()),
                );
                dashes(&segments, &phases)
            } else {
                curve(&points, None, plot.smooth)
            };
            if let Some(path) = line {
                let line = tiny_skia::Stroke {
                    width: stroke,
                    line_join: tiny_skia::LineJoin::Round,
                    ..Default::default()
                };
                // What `stroke_path` would do in every band: outline the line
                // once, then fill the outline. A hairline at this density
                // takes tiny-skia's own path.
                if stroke * density > 1. {
                    if let Some(outline) = path.stroke(&line, resolution) {
                        shapes.push(Shape::Fill {
                            path: outline,
                            color: solid,
                            faded: false,
                        });
                    }
                } else {
                    shapes.push(Shape::Stroke {
                        path,
                        color: solid,
                        stroke: line,
                    });
                }
            }
        }
        if series.dashed {
            // Unwrapped phases grow as the curve scrolls; whole periods
            // leave the pattern as it is.
            let base = next_phases.values().copied().fold(f64::INFINITY, f64::min);
            let base = if base.is_finite() {
                (base / DASH_PERIOD).floor() * DASH_PERIOD
            } else {
                0.
            };
            drawn_phases[index] = next_phases
                .into_iter()
                .map(|(at, phase)| (at, phase - base))
                .collect();
        }
    }
    drop(drawn_phases);
    let threads = threads.max(1);
    let bands = if threads > 1 {
        threads * BANDS_PER_THREAD
    } else {
        1
    };
    let row = pixel_width as usize * 4;
    let rows = (pixel_height as usize).div_ceil(bands).max(1);
    // One buffer, turned into straight colour in place: a second one would
    // double the fresh pages every raster touches.
    let mut pixels = vec![0u8; row * pixel_height as usize];
    let draw = |first: usize, pixels: &mut [u8]| {
        let count = (pixels.len() / row) as u32;
        if let Some(mut pixmap) = tiny_skia::PixmapMut::from_bytes(pixels, pixel_width, count) {
            let transform = transform.post_translate(0., -(first as f32));
            // Areas fade from their opacity at the chart's top to nothing at its
            // bottom: a solid fill through this row mask, which costs a fraction
            // of a gradient shader's per-pixel work.
            let fade = fill
                .then(|| tiny_skia::Mask::new(pixel_width, count))
                .flatten()
                .map(|mut mask| {
                    for (index, line) in mask
                        .data_mut()
                        .chunks_exact_mut(pixel_width as usize)
                        .enumerate()
                    {
                        let y = ((first + index) as f32 + 0.5) / density;
                        let level = ((height - y) / (height - top)).clamp(0., 1.);
                        // Down to nothing at the bottom, as the interface's gradient.
                        // The renderer averages premultiplied texels (see
                        // `paint_image_subpixel`), so the noisy straight colour of
                        // the faintest rows stays invisible.
                        line.fill((level * 255. + 0.5) as u8);
                    }
                    mask
                });
            for shape in &shapes {
                match shape {
                    Shape::Fill { path, color, faded } => {
                        let mask = if *faded { fade.as_ref() } else { None };
                        if *faded && mask.is_none() {
                            continue;
                        }
                        pixmap.fill_path(
                            path,
                            &tiny_skia::Paint {
                                shader: tiny_skia::Shader::SolidColor(*color),
                                anti_alias: true,
                                ..Default::default()
                            },
                            tiny_skia::FillRule::Winding,
                            transform,
                            mask,
                        );
                    }
                    Shape::Stroke {
                        path,
                        color,
                        stroke,
                    } => pixmap.stroke_path(
                        path,
                        &tiny_skia::Paint {
                            shader: tiny_skia::Shader::SolidColor(*color),
                            anti_alias: true,
                            ..Default::default()
                        },
                        stroke,
                        transform,
                        None,
                    ),
                }
            }
        }
        straight_bgra(pixels);
    };
    let mut work = (0..threads).map(|_| Vec::new()).collect::<Vec<_>>();
    for (index, band) in pixels.chunks_mut(rows * row).enumerate() {
        work[index % threads].push((index * rows, band));
    }
    std::thread::scope(|scope| {
        let mut work = work.into_iter();
        let here = work.next();
        for bands in work {
            let draw = &draw;
            scope.spawn(move || {
                for (first, band) in bands {
                    draw(first, band);
                }
            });
        }
        for (first, band) in here.into_iter().flatten() {
            draw(first, band);
        }
    });
    let buffer = image::RgbaImage::from_raw(pixel_width, pixel_height, pixels)?;
    if gpui_kit::gpui::profiler::phase_trace_enabled() {
        println!(
            "{}",
            serde_json::json!({"event":"chart-raster","width":pixel_width,"height":pixel_height,
            "threads":threads,"us":started.elapsed().as_micros(),"atQpc":gpui_kit::gpui::profiler::trace_clock_100ns()})
        );
    }
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
        min: plot.min,
        max: plot.max,
        top,
        content: plot.content,
    })
}

/// tiny-skia's premultiplied RGBA turned, in place, into the straight BGRA a
/// RenderImage holds. Most pixels are empty or repeat their neighbour (an
/// area's row shares one colour), so the division runs only where the colour
/// changes.
fn straight_bgra(pixels: &mut [u8]) {
    let mut last = ([0u8; 4], [0u8; 4]);
    for out in pixels.as_chunks_mut::<4>().0 {
        let pixel = *out;
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
}

#[cfg(test)]
mod tests {
    use super::{Input, Texture, key, rasterize, rasterize_in, straight_bgra};
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
                let mut converted = pixel;
                straight_bgra(&mut converted);
                assert_eq!(
                    converted,
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
            readout: Default::default(),
            captions: None,
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
    fn dash_phases_stay_with_their_samples() {
        use super::{DASH_PERIOD, run_phases};
        use std::collections::HashMap;
        let lengths = [15., 22., 18., 30., 16.];
        let times = [0., 1., 2., 3., 4., 5.];
        // Nothing drawn yet: the pattern runs on along the curve.
        let first = run_phases(&times, &lengths, &HashMap::new());
        assert_eq!(first, vec![0., 15., 37., 55., 85., 101.]);
        let drawn = times
            .iter()
            .map(|t: &f64| t.to_bits())
            .zip(first.iter().copied())
            .collect::<HashMap<_, _>>();
        // The oldest sample dropped off, bending the new first segment anew,
        // and a new one arrived: every sample drawn before keeps its phase,
        // the new one continues from the last.
        let later = run_phases(
            &[1., 2., 3., 4., 5., 6.],
            &[22.6, 18., 30., 16., 20.],
            &drawn,
        );
        assert_eq!(later, vec![15., 37., 55., 85., 101., 121.]);
        // Every segment changed (a new scale): each meets its samples' phases
        // modulo the period, stretching its dashes a little.
        let rescaled = run_phases(&times, &[15., 40., 21., 27., 17.], &drawn);
        for (now, before) in rescaled.iter().zip(&first) {
            assert_eq!((now - before).rem_euclid(DASH_PERIOD), 0.);
        }
        // A segment that would have to stretch too far takes its own length;
        // the next one meets its sample's phase again.
        let short = run_phases(&times, &[15., 6., 18., 30., 16.], &drawn);
        assert_eq!(short[2], 21.);
        assert_eq!((short[3] - first[3]).rem_euclid(DASH_PERIOD), 0.);
        assert!(((short[3] - short[2]) / 18. - 1.).abs() <= super::DASH_STRETCH);
    }
    #[test]
    fn a_new_sample_redraws_the_dashes_on_screen_texel_for_texel() {
        // A dashed curve through bumpy samples a second apart, drawn, then
        // drawn again a sample later (the oldest dropped, one more arrived):
        // where both cover the same times, away from the two segments that
        // bend anew at the ends, the texels match.
        let value = |i: usize| Some(30. + ((i * 7919) % 41) as f64);
        let points = |from: usize| {
            (from..from + 12)
                .map(|i| (i as f64 * 1000., value(i)))
                .collect::<Vec<_>>()
        };
        let draw = |p: &TimePlot| rasterize(&Input::new(p, size(px(337.), px(60.)), 2.)).unwrap();
        let mut p = plot(points(0));
        p.series[0] = p.series[0].clone().dashed(true);
        p.end = 11_000.;
        let a = draw(&p);
        let phases = p.dash_phases.clone();
        let mut q = plot(points(1));
        q.series[0] = q.series[0].clone().dashed(true);
        q.dash_phases = phases;
        q.start = 1000.;
        q.end = 12_000.;
        q.geometry_start = 1000.;
        let b = draw(&q);
        // Texture column of time t: ((t - start) / span * width - left) * 2.
        let column = |t: &Texture, at: f64| {
            ((at - t.geometry_start) / t.span * 337. - f64::from(f32::from(t.offset.x))) * 2.
        };
        let shift = column(&a, 0.) - column(&b, 0.);
        assert!(
            (shift - shift.round()).abs() < 1e-3,
            "sub-texel shift {shift}"
        );
        let shift = shift.round() as usize;
        let (from, to) = (column(&a, 3000.) as usize, column(&a, 9000.) as usize);
        let (wa, wb) = (
            a.image.size(0).width.0 as usize,
            b.image.size(0).width.0 as usize,
        );
        let (pa, pb) = (a.image.as_bytes(0).unwrap(), b.image.as_bytes(0).unwrap());
        // Compared premultiplied, as the renderer averages texels. The same
        // shapes moved by whole texels can still round a coordinate to the
        // next float, moving an edge across one of a texel's 16 coverage
        // samples; a dash moved along the curve, or the curve antialiased at
        // another sub-texel position, changes far more texels by far more
        // (restarting the dashes changes more texels than are inked here, by
        // up to 255).
        let premultiplied = |c: &[u8]| {
            let alpha = u32::from(c[3]);
            [0, 1, 2]
                .map(|k| (u32::from(c[k]) * alpha + 127) / 255)
                .into_iter()
                .chain([alpha])
                .collect::<Vec<_>>()
        };
        let (mut differ, mut worst, mut inked) = (0, 0, 0);
        for y in 0..120 {
            for x in from..to {
                let (i, j) = ((y * wa + x) * 4, (y * wb + x - shift) * 4);
                let (p, q) = (premultiplied(&pa[i..i + 4]), premultiplied(&pb[j..j + 4]));
                inked += usize::from(p[3] > 0);
                let d = p.iter().zip(&q).map(|(a, b)| a.abs_diff(*b)).max().unwrap();
                if d > 0 {
                    differ += 1;
                    worst = worst.max(d);
                }
            }
        }
        eprintln!("{differ} of {inked} inked texels differ, by at most {worst}");
        assert!(inked > 500, "only {inked} texels inked");
        assert!(worst <= 32, "a texel differs by {worst}");
        assert!(
            differ * 50 < inked,
            "{differ} of {inked} inked texels differ"
        );
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
    #[test]
    fn threads_draw_what_one_thread_draws() {
        // A busy chart, 4400 by 1120 device pixels: a filled line over a
        // dashed one through 60 noisy samples.
        let noise = |i: usize, k: u64| {
            let mut x = (i as u64 + 1).wrapping_mul(0x9e37_79b9_7f4a_7c15 ^ k);
            x ^= x >> 29;
            Some((x % 1000) as f64 / 10.)
        };
        let points = |k| (0..60).map(|i| (i as f64, noise(i, k))).collect::<Vec<_>>();
        let mut p = plot(points(1));
        p.end = 59.;
        p.fill = true;
        p.series[0] = p.series[0].clone().fill(0.3);
        p.series.push(
            Series::new(
                String::new(),
                points(2),
                rgb(0x28b8e0).into(),
                MetricFormat::Percent,
            )
            .dashed(true)
            .fill(0.),
        );
        let draw = |threads| {
            let started = std::time::Instant::now();
            let texture = rasterize_in(&Input::new(&p, size(px(1100.), px(280.)), 4.), threads);
            (texture.unwrap(), started.elapsed())
        };
        let (one, one_time) = draw(1);
        let (eight, eight_time) = draw(8);
        let (a, b) = (
            one.image.as_bytes(0).unwrap(),
            eight.image.as_bytes(0).unwrap(),
        );
        assert_eq!(a.len(), b.len());
        // Compared premultiplied, as the renderer averages texels. A curve cut
        // at the edge of a band is subdivided anew there, which can move an
        // edge across one of the 4 by 4 coverage samples of a texel.
        let premultiplied = |c: &[u8; 4]| {
            let alpha = u32::from(c[3]);
            [
                (u32::from(c[0]) * alpha + 127) / 255,
                (u32::from(c[1]) * alpha + 127) / 255,
                (u32::from(c[2]) * alpha + 127) / 255,
                alpha,
            ]
        };
        let mut differ = 0;
        let mut worst = 0;
        for (p, q) in a.as_chunks::<4>().0.iter().zip(b.as_chunks::<4>().0) {
            let (p, q) = (premultiplied(p), premultiplied(q));
            let d = p.iter().zip(&q).map(|(x, y)| x.abs_diff(*y)).max().unwrap();
            if d > 0 {
                differ += 1;
                worst = worst.max(d);
            }
        }
        eprintln!(
            "{differ} of {} texels differ, by at most {worst}; one thread {one_time:?}, eight {eight_time:?}",
            a.len() / 4
        );
        assert!(differ * 1000 < a.len() / 4, "{differ} texels differ");
        assert!(worst <= 64, "a texel differs by {worst}");
    }
}
