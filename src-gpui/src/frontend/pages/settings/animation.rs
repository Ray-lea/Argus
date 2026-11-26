//! Motion: the interface motion switch and the chart animation choice, which
//! the onboarding shows as well.
use super::*;

impl Desktop {
    /// The interface motion toggle (also used by the onboarding).
    pub(in crate::frontend) fn motion_toggle(
        &self,
        id: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        self.toggle(
            id.to_owned(),
            label("settings.motion.ui.title"),
            !cx.reduce_motion(),
            false,
            |this, on, window, cx| {
                this.preference("uiMotion", json!(if on { "on" } else { "off" }), window, cx);
            },
            window,
            cx,
        )
    }

    /// The chart animation choice, labelled with frame rates (also used by the onboarding).
    pub(in crate::frontend) fn chart_select(
        &self,
        id: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let fps = |mode| {
            (1. / motion::frame_period(mode, window.refresh_interval(cx)).as_secs_f64()).round()
                as u32
        };
        let modes = ["fluid", "balanced", "pixel", "off"];
        let current = self
            .prefs
            .get::<Option<String>>("motion.chartMode", None)
            .unwrap_or_else(|| {
                if crate::platform::system_motion() {
                    "fluid"
                } else {
                    "off"
                }
                .into()
            });
        self.select(
            id.to_owned(),
            label("settings.motion.chart.title"),
            vec![
                tr!(
                    "settings.motion.chart.fluid",
                    fps = fps(motion::Mode::Fluid)
                ),
                tr!(
                    "settings.motion.chart.balanced",
                    fps = fps(motion::Mode::Balanced)
                ),
                label("settings.motion.chart.pixel"),
                label("settings.motion.chart.off"),
            ],
            modes.iter().position(|m| *m == current),
            false,
            move |this, index, window, cx| {
                this.preference("motion.chartMode", json!(modes[index]), window, cx);
            },
            cx,
        )
    }
}
