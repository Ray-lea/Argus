//! A lane's curves: the series it draws, and the plot that draws them.
use super::*;

/// A lane's curve: `series` on the left axis of `axes`, shown in the window
/// `shown` and drawn for the window from `drawn.0`, as long as `drawn.1`
/// (none: the window shown).
pub(super) fn history_curve(
    id: String,
    series: Vec<charts::Series>,
    axes: &Axes,
    (from, to): (f64, f64),
    drawn: (f64, Option<f64>),
    ticks: Option<(Arc<Vec<f64>>, Hsla)>,
) -> charts::TimePlot {
    charts::TimePlot {
        id: id.into(),
        series: Arc::new(series),
        start: from,
        end: to,
        max: axes.left.1,
        min: axes.left.0,
        interactive: false,
        grid: false,
        fill: true,
        background_separate: false,
        smooth: false,
        ticks,
        geometry_start: drawn.0,
        measured_width: Rc::new(Cell::new(1.)),
        live_clock: None,
        image_surface: Default::default(),
        dash_phases: Default::default(),
        rescale: Default::default(),
        readout: Default::default(),
        captions: None,
        redraw_alone: true,
        raster_span: drawn.1,
        corner: STEP_CORNER,
    }
}

/// Maps a right-axis value onto the left axis the plot draws.
fn onto(value: f64, from: (f64, f64), to: (f64, f64)) -> f64 {
    let span = (from.1 - from.0).max(f64::EPSILON);
    to.0 + (value - from.0) / span * (to.1 - to.0)
}

impl Desktop {
    /// The series a lane draws: the band's top (CPU's highest samples), the
    /// reading (a pair's second part as a dim line), the overlay on its axis.
    pub(super) fn history_lane_series(
        &self,
        lane: &Lane,
        // The overlay drawn (one going included) and how opaque it is.
        overlay: Option<(&str, f32)>,
        points: &Points,
        axes: &Axes,
        bucket: i64,
    ) -> Vec<charts::Series> {
        let mut series = Vec::new();
        let Some(reading) = self.history_reading(&lane.reading) else {
            return series;
        };
        let end = self.history_curve_end(points);
        let source = |part: &str| charts::hash_of(("history", lane.id, &reading.id, part));
        let format = charts::MetricFormat::Percent;
        // From the lowest to the highest reading of each bucket (CPU up to its
        // highest sample), under the line.
        let banded = bucket >= BANDED && self.history.band;
        if banded && !reading.is_pair() {
            let key = &reading.keys[0];
            let high = reading
                .peak
                .as_deref()
                .filter(|peak| points.has(peak))
                .unwrap_or(key);
            series.push(
                charts::Series::band(
                    String::new(),
                    points.band(key, high, end),
                    reading.color,
                    format,
                )
                .source(source("band"))
                .layer(true),
            );
        }
        let main = if reading.is_pair() && !points.has(&reading.keys[0]) {
            reading
                .fallback
                .clone()
                .unwrap_or_else(|| reading.keys[0].clone())
        } else {
            reading.keys[0].clone()
        };
        series.push(
            charts::Series::new(
                String::new(),
                points.line(&main, Part::Mean, end, |v| v),
                reading.color,
                format,
            )
            .fill(0.22)
            .source(source("main")),
        );
        if reading.is_pair() && points.has(&reading.keys[1]) {
            series.push(
                charts::Series::new(
                    String::new(),
                    points.line(&reading.keys[1], Part::Mean, end, |v| v),
                    reading.color.opacity(0.5),
                    format,
                )
                .fill(0.)
                .source(source("second")),
            );
        }
        if let Some((overlay, alpha)) =
            overlay.and_then(|(id, alpha)| Some((self.history_reading(id)?, alpha)))
            && let Some(right) = axes.right
        {
            let left = axes.left;
            // A pair's parts as in a lane of its own, the second dimmer.
            let mut lines = vec![(
                overlay.main_key(points).to_owned(),
                overlay.color.opacity(alpha),
                "overlay",
            )];
            if overlay.is_pair() && points.has(&overlay.keys[1]) {
                lines.push((
                    overlay.keys[1].clone(),
                    overlay.color.opacity(0.5 * alpha),
                    "overlay-second",
                ));
            }
            for (key, color, part) in lines {
                series.push(
                    charts::Series::new(
                        String::new(),
                        points.line(&key, Part::Mean, end, |v| onto(v, right, left)),
                        color,
                        format,
                    )
                    .fill(0.)
                    .source(charts::hash_of(("history", lane.id, &overlay.id, part)))
                    // Coming and going with the overlay, it leaves the lane's
                    // curve on screen while its texture is drawn.
                    .layer(true),
                );
            }
        }
        // A process laid over the lanes: its own readings of the lane's kind, dashed.
        if let Some(curve) = self.history_process_curve(reading) {
            series.push(
                charts::Series::new(String::new(), curve, self.palette.ink.opacity(0.6), format)
                    .dashed(true)
                    .fill(0.)
                    .source(charts::hash_of((
                        "history",
                        lane.id,
                        &reading.id,
                        "process",
                        &self.history.curve,
                    ))),
            );
        }
        series
    }

    /// The chosen process's readings for `reading`'s kind over the window it was
    /// asked for, its instances summed: points spread evenly across it.
    fn history_process_curve(&self, reading: &Reading) -> Option<Vec<(f64, Option<f64>)>> {
        let name = self.history.curve.as_deref()?;
        let key = reading.process?;
        let answer = if self.options.fixture.is_some() {
            self.data.get("historyTable")?
        } else {
            self.history.curves.get(key)?
        };
        let (from, to) = (answer["from"].as_f64()?, answer["to"].as_f64()?);
        let mut sum: Vec<Option<f64>> = Vec::new();
        for process in values(&answer["processes"]) {
            if !text(process, "name").eq_ignore_ascii_case(name) {
                continue;
            }
            for (i, v) in values(&process["trend"]).iter().enumerate() {
                if sum.len() <= i {
                    sum.resize(i + 1, None);
                }
                if let Some(v) = v.as_f64() {
                    sum[i] = Some(sum[i].unwrap_or(0.) + v);
                }
            }
        }
        if sum.is_empty() {
            return None;
        }
        let step = (to - from) / sum.len() as f64;
        Some(
            sum.into_iter()
                .enumerate()
                .map(|(i, v)| (from + step * (i as f64 + 0.5), v))
                .collect(),
        )
    }
}
