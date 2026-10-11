// A selection mark that slides to the chosen option instead of blinking from
// one to the next: page tabs, segmented controls, the device list.
// Place it inside a `position: relative` container; it follows the child that
// matches `selector` whenever `track` changes.

import { createEffect, on, onCleanup, onMount } from "solid-js";
import { uiMotion } from "../lib/motion";

type Shape =
  /** Covers the selected option (segmented controls, lists). */
  | "fill"
  /** A short bar under the selected option (tabs). */
  | "underline";

export default function Indicator(props: {
  selector: string;
  track: () => unknown;
  class: string;
  shape?: Shape;
}) {
  let el!: HTMLSpanElement;

  onMount(() => {
    const parent = el.parentElement as HTMLElement;
    let placed = false;

    const place = (animate: boolean) => {
      const target = parent.querySelector<HTMLElement>(props.selector);
      if (!target || target.offsetWidth === 0) {
        el.style.opacity = "0";
        placed = false;
        return;
      }
      const glide = animate && placed && uiMotion();
      el.style.transition = glide
        ? "transform var(--t-slow) var(--ease-move), width var(--t-slow) var(--ease-move), height var(--t-slow) var(--ease-move)"
        : "none";
      const { offsetLeft: x, offsetTop: y, offsetWidth: w, offsetHeight: h } = target;
      if (props.shape === "underline") {
        el.style.width = "16px";
        el.style.height = "3px";
        el.style.transform = `translate(${x + w / 2 - 8}px, ${y + h - 5}px)`;
      } else {
        el.style.width = `${w}px`;
        el.style.height = `${h}px`;
        el.style.transform = `translate(${x}px, ${y}px)`;
      }
      el.style.opacity = "1";
      placed = true;
    };

    createEffect(on(props.track, () => place(true)));
    // Layout changes (fonts loading, labels hiding on narrow windows) move the
    // options; follow them without animating.
    const ro = new ResizeObserver(() => place(false));
    ro.observe(parent);
    for (const child of parent.children) if (child !== el) ro.observe(child);
    onCleanup(() => ro.disconnect());
  });

  return (
    <span
      ref={el}
      class={props.class}
      aria-hidden="true"
      style={{ position: "absolute", top: 0, left: 0, opacity: 0, "pointer-events": "none" }}
    />
  );
}
