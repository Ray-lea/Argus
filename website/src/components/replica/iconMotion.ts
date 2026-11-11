import type { UiIcon } from "./icons";

// The app's icon motions (src-gpui/src/frontend/icon_motion.rs): a control's
// icon plays one short motion when the pointer comes onto the control, and
// comes in anew when the control swaps it for another (pause for play). Each
// is a transform of the whole icon that ends on the icon at rest; none
// repeats, and none starts while the interface's motion is off.
type Motion =
  | { kind: "spin" | "swing"; degrees: number }
  | { kind: "nudge"; x: number; y: number }
  | { kind: "pop" | "blink" | "enter" };
const spin = (degrees: number): Motion => ({ kind: "spin", degrees });
const swing = (degrees: number): Motion => ({ kind: "swing", degrees });
const nudge = (x: number, y: number): Motion => ({ kind: "nudge", x, y });
// Icons with a direction follow it, those that turn in real life turn, the
// eye blinks and the rest swell.
const motions: Partial<Record<UiIcon, Motion>> = {
  // The gear has six teeth: a sixth of a turn brings it onto itself.
  settings: spin(-60),
  restart: spin(-360),
  reset: spin(360),
  history: swing(-40),
  theme: swing(180),
  pin: swing(-18),
  unpin: swing(-18),
  timer: swing(18),
  sparkle: swing(18),
  services: swing(12),
  trash: swing(-10),
  update: nudge(0, 2),
  tray: nudge(0, 2),
  sortDescending: nudge(0, 2),
  chevronDown: nudge(0, 2),
  sortAscending: nudge(0, -2),
  chevronUp: nudge(0, -2),
  transfer: nudge(0, -2),
  external: nudge(1.5, -1.5),
  goTo: nudge(2, 0),
  arrowRight: nudge(2, 0),
  chevronRight: nudge(2, 0),
  runTask: nudge(2, 0),
  keyboard: nudge(0, 1),
  eye: { kind: "blink" },
  eyeOff: { kind: "blink" },
};

const duration = (m: Motion) =>
  m.kind === "spin"
    ? Math.abs(m.degrees) >= 180
      ? 620
      : 420
    : m.kind === "swing"
      ? 460
      : m.kind === "nudge"
        ? 420
        : m.kind === "pop"
          ? 320
          : m.kind === "blink"
            ? 240
            : 260;

// A CSS cubic-bezier timing function at t.
function bezier(x1: number, y1: number, x2: number, y2: number, t: number) {
  const at = (a: number, b: number, s: number) => 3 * (1 - s) * (1 - s) * s * a + 3 * (1 - s) * s * s * b + s * s * s;
  let lo = 0,
    hi = 1,
    s = t;
  for (let i = 0; i < 24; i++) {
    s = (lo + hi) / 2;
    if (at(x1, x2, s) < t) lo = s;
    else hi = s;
  }
  return at(y1, y2, s);
}
const easeMove = (t: number) => bezier(0.33, 0, 0.1, 1, t);
const easeOut = (t: number) => bezier(0.1, 0.9, 0.2, 1, t);
// Out quickly and back at leisure: 0 at both ends, 1 a fifth of the way in.
const thereAndBack = (t: number) => Math.sin(Math.PI * bezier(0.33, 1, 0.68, 1, t));

function pose(m: Motion, t: number): Keyframe {
  let x = 0,
    y = 0,
    turn = 0,
    sx = 1,
    sy = 1,
    opacity = 1;
  if (m.kind === "spin") turn = m.degrees * (1 - easeMove(t));
  else if (m.kind === "swing") turn = m.degrees * thereAndBack(t);
  else if (m.kind === "nudge") {
    x = m.x * thereAndBack(t);
    y = m.y * thereAndBack(t);
  } else if (m.kind === "pop") sx = sy = 1 + 0.14 * thereAndBack(t);
  else if (m.kind === "blink") sy = 1 - 0.82 * Math.sin(Math.PI * t);
  else {
    // Passes its size a little before it settles.
    sx = sy = 0.6 + 0.4 * bezier(0.34, 1.56, 0.64, 1, t);
    opacity = easeOut(Math.min(1, t / 0.5));
  }
  const frame: Keyframe = {
    offset: t,
    transform: `translate(${x.toFixed(3)}px, ${y.toFixed(3)}px) rotate(${turn.toFixed(2)}deg) scale(${sx.toFixed(4)}, ${sy.toFixed(4)})`,
  };
  if (m.kind === "enter") frame.opacity = opacity;
  return frame;
}

let still = false;
/** Follows the "界面动画" setting. */
export function setIconsStill(value: boolean) {
  still = value;
}
const playing = new WeakMap<Element, Animation>();

/** Sets an icon moving: its own motion, or the entrance of a swapped icon. */
export function playIcon(svg: Element | null | undefined, entering = false) {
  if (!svg || still || typeof svg.animate !== "function" || matchMedia("(prefers-reduced-motion: reduce)").matches) return;
  const motion: Motion = entering
    ? { kind: "enter" }
    : (motions[svg.getAttribute("data-icon") as UiIcon] ?? { kind: "pop" });
  const ms = duration(motion);
  const steps = Math.ceil(ms / 16);
  const frames = Array.from({ length: steps }, (_, i) => pose(motion, i / steps));
  frames.push({ offset: 1, transform: "none", ...(entering ? { opacity: 1 } : {}) });
  // A new play starts over, as the app's does.
  playing.get(svg)?.cancel();
  playing.set(svg, svg.animate(frames, { duration: ms, easing: "linear" }));
}

// The controls whose icon moves under the pointer: buttons, the options of a
// segmented control and the settings groups not chosen, and the navigation.
const hosts = ".a-btn, .a-seg > button[aria-checked='false'], .a-settings-rail button:not([aria-current]), .a-nav button";
/** For the window's `pointerover`: the pointer came onto a control. */
export function hoverIcons(event: { target: EventTarget | null; relatedTarget: EventTarget | null }) {
  const host = event.target instanceof Element ? event.target.closest(hosts) : null;
  if (!host || (event.relatedTarget instanceof Node && host.contains(event.relatedTarget))) return;
  playIcon(host.querySelector(".ico"));
}
