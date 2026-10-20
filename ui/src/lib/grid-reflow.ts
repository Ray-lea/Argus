import { createEffect, onCleanup } from "solid-js";
import { EASE_OUT, play, uiMotion } from "./motion";

/** Animate a discrete grid rearrangement without animating layout on every frame. */
export function gridReflow(grid: HTMLElement) {
  const active = new Map<Element, Animation>();
  const cancel = () => {
    for (const [cell, animation] of active) {
      animation.cancel();
      cell.classList.remove("grid-moving");
    }
    active.clear();
    grid.classList.remove("reflowing");
  };
  createEffect(() => {
    if (!uiMotion()) cancel();
  });
  onCleanup(cancel);

  const change = (apply: () => void) => {
    if (!uiMotion()) {
      cancel();
      apply();
      return;
    }
    const cells = [...grid.children];
    // Capture before cancelling: another rearrangement may still be in flight.
    const before = cells.map((cell) => cell.getBoundingClientRect());
    cancel();
    apply();
    const after = cells.map((cell) => cell.getBoundingClientRect());
    const viewport = (grid.closest(".perf-scroll") ?? grid).getBoundingClientRect();
    const visible = (r: DOMRect) =>
      r.right > viewport.left &&
      r.left < viewport.right &&
      r.bottom > viewport.top &&
      r.top < viewport.bottom;
    const start = document.timeline.currentTime;
    cells.forEach((cell, index) => {
      const from = before[index];
      const to = after[index];
      if (!from.width || !from.height || !to.width || !to.height || (!visible(from) && !visible(to))) return;
      const x = from.left - to.left;
      const y = from.top - to.top;
      const sx = from.width / to.width;
      const sy = from.height / to.height;
      if (Math.abs(x) < 0.5 && Math.abs(y) < 0.5 && Math.abs(sx - 1) < 0.001 && Math.abs(sy - 1) < 0.001)
        return;
      cell.classList.add("grid-moving");
      const animation = play(
        cell,
        [{ transform: `translate(${x}px, ${y}px) scale(${sx}, ${sy})` }, { transform: "none" }],
        { duration: 200, easing: EASE_OUT },
      );
      if (!animation) {
        cell.classList.remove("grid-moving");
        return;
      }
      if (typeof start === "number") animation.startTime = start;
      active.set(cell, animation);
      animation.onfinish = () => {
        if (active.get(cell) !== animation) return;
        animation.cancel();
        cell.classList.remove("grid-moving");
        active.delete(cell);
        if (active.size === 0) grid.classList.remove("reflowing");
      };
    });
    if (active.size > 0) grid.classList.add("reflowing");
  };
  return { change, cancel };
}
