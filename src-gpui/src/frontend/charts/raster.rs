//! Rasterize a curve revision once; continuous frames only translate its texture.
use super::*;
use std::hash::{Hash, Hasher};
use std::sync::atomic::{AtomicU64, Ordering};

static BUILDS: AtomicU64 = AtomicU64::new(0);
static BUILD_MICROS: AtomicU64 = AtomicU64::new(0);

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
    pending: bool,
    texture: Option<Texture>,
}
struct Texture {
    image: Arc<RenderImage>,
    offset: Point<Pixels>,
    size: Size<Pixels>,
    plot_size: Size<Pixels>,
    geometry_start: f64,
    span: f64,
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
}
impl ImageSurface {
    fn bounds_at(&self, plot: &TimePlot, bounds: Bounds<Pixels>, end: f64) -> Bounds<Pixels> {
        let span = (plot.end - plot.start).max(1.);
        let sx = f32::from(bounds.size.width) / f32::from(self.plot_size.width)
            * (self.span / span) as f32;
        let sy = f32::from(bounds.size.height) / f32::from(self.plot_size.height);
        let origin = bounds.origin
            + point(self.offset.x * sx, self.offset.y * sy)
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
    for series in &plot.series {
        series.geometry_key.hash(&mut key);
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

pub(super) fn paint(plot: &TimePlot, bounds: Bounds<Pixels>, window: &mut Window, cx: &mut App) {
    let density = window.scale_factor()
        * theme::design()["layout"]["chartRasterScale"]
            .as_f64()
            .expect("chart raster scale") as f32;
    let revision = key(plot, bounds.size, density);
    let cache = window.use_keyed_state("argus-curve-texture", cx, |window, cx| {
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
    });
    cache.update(cx, |cache, cx| {
        cache.requested = Some(revision);
        if cache.key != Some(revision) && !cache.pending {
            cache.pending = true;
            let input = Input::new(plot, bounds.size, density);
            let task = cx.background_executor().spawn(async move {
                let start = Instant::now();
                let texture = rasterize(&input);
                BUILDS.fetch_add(1, Ordering::Relaxed);
                BUILD_MICROS.fetch_add(start.elapsed().as_micros() as u64, Ordering::Relaxed);
                texture
            });
            cx.spawn_in(window, async move |cache, cx| {
                let texture = task.await;
                let _ = cache.update_in(cx, |cache, window, cx| {
                    cache.pending = false;
                    if cache.requested == Some(revision) {
                        if let Some(previous) = cache.texture.take() {
                            let _ = window.drop_image(previous.image);
                        }
                        cache.texture = texture;
                        cache.key = Some(revision);
                    }
                    cx.notify();
                });
            })
            .detach();
        }
        if let Some(texture) = &cache.texture {
            let mut surface = ImageSurface {
                owner: cx.entity().entity_id(),
                handle: None,
                _image: texture.image.clone(),
                offset: texture.offset,
                size: texture.size,
                plot_size: texture.plot_size,
                geometry_start: texture.geometry_start,
                span: texture.span,
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
        }
    });
}

fn curve(points: &[(f64, f64)], baseline: Option<f32>) -> Option<tiny_skia::Path> {
    if points.len() < 2 {
        return None;
    }
    let tangent = super::curve_tangents(points);
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
    for (index, series) in plot.series.iter().enumerate() {
        let color: Rgba = series.color.into();
        let solid = tiny_skia::Color::from_rgba(color.r, color.g, color.b, color.a)?;
        for run in series.geometry.iter() {
            let points = run
                .iter()
                .map(|&(at, value)| {
                    (
                        ((at - plot.geometry_start) / span) * width as f64 - left as f64,
                        height as f64 - (value - plot.min) / scale * (height - stroke / 2.) as f64,
                    )
                })
                .collect::<Vec<_>>();
            if plot.fill
                && index == 0
                && let Some(path) = curve(&points, Some(height))
            {
                let shader = tiny_skia::LinearGradient::new(
                    tiny_skia::Point::from_xy(
                        0.,
                        points
                            .iter()
                            .map(|point| point.1 as f32)
                            .fold(height, f32::min)
                            .clamp(0., height),
                    ),
                    tiny_skia::Point::from_xy(0., height),
                    vec![
                        tiny_skia::GradientStop::new(
                            0.,
                            tiny_skia::Color::from_rgba(color.r, color.g, color.b, color.a * 0.16)?,
                        ),
                        tiny_skia::GradientStop::new(
                            1.,
                            tiny_skia::Color::from_rgba(color.r, color.g, color.b, color.a * 0.02)?,
                        ),
                    ],
                    tiny_skia::SpreadMode::Pad,
                    tiny_skia::Transform::identity(),
                )
                .unwrap_or(tiny_skia::Shader::SolidColor(
                    tiny_skia::Color::from_rgba(color.r, color.g, color.b, color.a * 0.02)?,
                ));
                pixels.fill_path(
                    &path,
                    &tiny_skia::Paint {
                        shader,
                        anti_alias: true,
                        ..Default::default()
                    },
                    tiny_skia::FillRule::Winding,
                    transform,
                    None,
                );
            }
            if let Some(path) = curve(&points, None) {
                pixels.stroke_path(
                    &path,
                    &tiny_skia::Paint {
                        shader: tiny_skia::Shader::SolidColor(solid),
                        anti_alias: true,
                        ..Default::default()
                    },
                    &tiny_skia::Stroke {
                        width: stroke,
                        ..Default::default()
                    },
                    transform,
                    None,
                );
            }
        }
    }
    // RenderImage uses straight BGRA, while tiny-skia stores premultiplied RGBA.
    let mut bgra = Vec::with_capacity(pixels.data().len());
    for pixel in pixels.pixels() {
        let c = pixel.demultiply();
        bgra.extend([c.blue(), c.green(), c.red(), c.alpha()]);
    }
    let buffer = image::RgbaImage::from_raw(pixel_width, pixel_height, bgra)?;
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
    })
}

#[cfg(test)]
mod tests {
    use super::{Input, key, rasterize};
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
            fill: false,
            bars: false,
            selection: None,
            cursor: None,
            geometry_start: 0.,
            measured_width: Rc::new(Cell::new(100.)),
            live_clock: None,
            image_surface: Default::default(),
            bar_surfaces: Default::default(),
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
