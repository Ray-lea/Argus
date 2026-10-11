// Motion: the user's preference, shared curves, and enter/exit helpers.
//
// On high refresh rate displays every animated frame costs a full trip through
// the compositor, so:
//  - Interface motion answers the user (switching pages, opening a menu) and
//    runs once.
//  - Charts scroll continuously at a frame rate the user picks (ChartMode);
//    meters move once per sample.

import { createComputed, createEffect, createRoot, createSignal, onCleanup, untrack } from "solid-js";
import { load, persisted } from "./prefs";

const reducedQuery = matchMedia("(prefers-reduced-motion: reduce)");
const [systemReduced, setSystemReduced] = createSignal(reducedQuery.matches);
reducedQuery.addEventListener("change", (e) => setSystemReduced(e.matches));

// null follows Windows (Settings > Accessibility > Visual effects > Animation effects).
const [uiPref, setUiPref] = persisted<boolean | null>("motion.ui", null);

/**
 * How charts scroll between samples:
 * - `fluid`: sub-pixel, every display refresh (on the compositor);
 * - `balanced`: the same, limited to about 60 position changes a second;
 * - `pixel`: one whole pixel at a time, a few frames a second;
 * - `off`: one jump per sample.
 */
export type ChartMode = "fluid" | "balanced" | "pixel" | "off";
// Earlier builds stored an on/off switch under "motion.charts".
const legacy = load<boolean | null>("motion.charts", null);
const [chartPref, setChartPref] = persisted<ChartMode | null>(
  "motion.chartMode",
  legacy === false ? "off" : null,
);

/** Page transitions, flyouts, sliding selection. */
export const uiMotion = () => uiPref() ?? !systemReduced();
export const chartMode = (): ChartMode => chartPref() ?? (systemReduced() ? "off" : "fluid");
/** Charts scroll at all (any mode but `off`). */
export const chartMotion = () => chartMode() !== "off";
export const setUiMotion = (on: boolean) => setUiPref(on);
export const setChartMode = (mode: ChartMode) => setChartPref(mode);

/**
 * The refresh rate of the display the window is on, measured from animation
 * frames (WebView2 does not expose it). Charts in `fluid` mode follow the
 * display on their own (the compositor animates every refresh); this value
 * labels the setting and paces `balanced` mode. Measured again whenever the
 * window comes back or changes screens, since monitors can differ.
 */
export const [refreshHz, setRefreshHz] = createSignal(60);

/** Panels run a little off their nominal rate (164.9 Hz); report the rate they are sold as. */
const NOMINAL_HZ = [
  50, 60, 72, 75, 85, 90, 100, 120, 144, 160, 165, 170, 175, 180, 200, 240, 280, 300, 360, 480, 500, 540,
];

let measuring = false;
function measureRefresh() {
  if (measuring || document.visibilityState !== "visible") return;
  measuring = true;
  const stamps: number[] = [];
  const step = (t: number) => {
    stamps.push(t);
    if (stamps.length < 61) {
      requestAnimationFrame(step);
      return;
    }
    measuring = false;
    // Frame times come in 0.1 ms steps (a 165 Hz display alternates 6.0 and
    // 6.1 ms), so average the intervals near the median; that also drops the
    // doubled ones where a frame was skipped.
    const gaps = stamps.slice(1).map((v, i) => v - stamps[i]);
    const median = [...gaps].sort((a, b) => a - b)[gaps.length >> 1];
    const steady = gaps.filter((g) => Math.abs(g - median) < median * 0.2);
    const mean = steady.reduce((a, b) => a + b, 0) / steady.length;
    if (!(mean > 1)) return;
    const hz = 1000 / mean;
    const nominal = NOMINAL_HZ.find((n) => Math.abs(n - hz) / n < 0.015);
    setRefreshHz(nominal ?? Math.round(hz));
  };
  requestAnimationFrame(step);
}
measureRefresh();
document.addEventListener("visibilitychange", measureRefresh);
window.addEventListener("focus", measureRefresh);
// A window dragged to another monitor of the same scale changes neither focus
// nor resolution; an occasional check (60 frames) catches that too.
setInterval(measureRefresh, 30_000);
// Moving to a screen with another scale factor; refresh rates usually differ too.
const watchScale = () =>
  matchMedia(`(resolution: ${window.devicePixelRatio}dppx)`).addEventListener(
    "change",
    () => {
      measureRefresh();
      watchScale();
    },
    { once: true },
  );
watchScale();

/**
 * Milliseconds between position changes in `balanced` mode: about 60 a second,
 * but always a whole number of refreshes (80 on a 160 Hz screen), so every
 * change is held for the same number of frames and the motion stays even.
 */
export const balancedFrameMs = () => {
  const hz = refreshHz();
  return (1000 / hz) * Math.max(1, Math.floor(hz / 60));
};

createRoot(() => {
  createEffect(() => document.documentElement.classList.toggle("still", !uiMotion()));
});

/** Things arriving: fast start, long settle (Fluent "decelerate max"). */
export const EASE_OUT = "cubic-bezier(0.1, 0.9, 0.2, 1)";
/** Things leaving. */
export const EASE_IN = "cubic-bezier(0.7, 0, 1, 0.5)";

/** Runs a one-off animation when interface motion is on. */
export function play(
  el: Element | null | undefined,
  keyframes: Keyframe[],
  options: KeyframeAnimationOptions,
): Animation | null {
  if (!el || !uiMotion()) return null;
  return el.animate(keyframes, options);
}

/**
 * Keeps the last truthy value of `source` for `ms` after it turns falsy, so
 * the element showing it can play an exit animation before it unmounts.
 */
export function presence<T>(source: () => T | null | undefined | false, ms: number) {
  const [value, setValue] = createSignal<T | null>(null);
  const [leaving, setLeaving] = createSignal(false);
  let timer: ReturnType<typeof setTimeout> | undefined;
  const settle = () => {
    setValue(null);
    setLeaving(false);
  };
  createComputed(() => {
    const v = source();
    clearTimeout(timer);
    if (v) {
      setLeaving(false);
      setValue(() => v);
    } else if (untrack(value) !== null) {
      if (untrack(uiMotion)) {
        setLeaving(true);
        timer = setTimeout(settle, ms);
      } else {
        settle();
      }
    }
  });
  onCleanup(() => clearTimeout(timer));
  return { value, leaving };
}
