import { createEffect, onCleanup } from "solid-js";
import { EASE_IN, EASE_OUT, play, uiMotion } from "./motion";

/** How long a card takes to reach its new place, and to appear or leave. */
const MOVE_MS = 260;
const ENTER_MS = 240;
const ENTER_DELAY_MS = 60;
const LEAVE_MS = 160;

/**
 * Animate a discrete grid rearrangement without animating layout on every
 * frame: cards that stay glide from where they were (translate and scale on
 * the compositor), new ones grow in, and ones taken out shrink away where
 * they were. A card that leaves goes on showing until it has faded: the
 * grid holds it, absolutely placed under the others (the grid needs a
 * position and its own stacking context).
 */
export function gridReflow(grid: HTMLElement) {
  const active = new Map<Element, Animation>();
  const leaving = new Set<HTMLElement>();
  const cancel = () => {
    for (const [cell, animation] of active) {
      animation.cancel();
      cell.classList.remove("grid-moving");
    }
    active.clear();
    grid.classList.remove("reflowing");
  };
  const dropLeaving = () => {
    for (const ghost of leaving) ghost.remove();
    leaving.clear();
    grid.classList.remove("reflowing");
  };
  createEffect(() => {
    if (!uiMotion()) {
      cancel();
      dropLeaving();
    }
  });
  onCleanup(() => {
    cancel();
    dropLeaving();
  });

  const idle = () => {
    if (active.size === 0 && leaving.size === 0) grid.classList.remove("reflowing");
  };
  const settled = (cell: Element, animation: Animation) => {
    if (active.get(cell) !== animation) return;
    animation.cancel();
    cell.classList.remove("grid-moving");
    active.delete(cell);
    idle();
  };

  const change = (apply: () => void) => {
    if (!uiMotion() || !grid.isConnected) {
      cancel();
      apply();
      return;
    }
    const cells = [...grid.children].filter((cell) => !leaving.has(cell as HTMLElement));
    // Capture before cancelling: another rearrangement may still be in flight.
    const before = new Map(cells.map((cell) => [cell, cell.getBoundingClientRect()]));
    cancel();
    apply();
    const viewport = (grid.closest(".perf-scroll") ?? grid).getBoundingClientRect();
    const visible = (r: DOMRect) =>
      r.right > viewport.left &&
      r.left < viewport.right &&
      r.bottom > viewport.top &&
      r.top < viewport.bottom;
    const start = document.timeline.currentTime;
    const track = (cell: Element, animation: Animation | null) => {
      if (!animation) {
        cell.classList.remove("grid-moving");
        return;
      }
      if (typeof start === "number") animation.startTime = start;
      active.set(cell, animation);
      animation.onfinish = () => settled(cell, animation);
    };

    for (const cell of grid.children) {
      if (leaving.has(cell as HTMLElement)) continue;
      const to = cell.getBoundingClientRect();
      const from = before.get(cell);
      if (!to.width || !to.height || (!visible(to) && !(from && visible(from)))) continue;
      if (!from) {
        // New: grows in once the others have started moving.
        cell.classList.add("grid-moving");
        track(
          cell,
          play(
            cell,
            [
              { opacity: 0, transform: "scale(0.94)", transformOrigin: "50% 50%" },
              { opacity: 1, transform: "none", transformOrigin: "50% 50%" },
            ],
            { duration: ENTER_MS, delay: ENTER_DELAY_MS, easing: EASE_OUT, fill: "backwards" },
          ),
        );
        continue;
      }
      if (!from.width || !from.height) continue;
      const x = from.left - to.left;
      const y = from.top - to.top;
      const sx = from.width / to.width;
      const sy = from.height / to.height;
      if (Math.abs(x) < 0.5 && Math.abs(y) < 0.5 && Math.abs(sx - 1) < 0.001 && Math.abs(sy - 1) < 0.001)
        continue;
      cell.classList.add("grid-moving");
      track(
        cell,
        play(cell, [{ transform: `translate(${x}px, ${y}px) scale(${sx}, ${sy})` }, { transform: "none" }], {
          duration: MOVE_MS,
          easing: EASE_OUT,
        }),
      );
    }

    // Taken out: shown again where it was, over the grid, while it fades.
    const box = grid.getBoundingClientRect();
    for (const [cell, from] of before) {
      if (cell.isConnected || !(cell instanceof HTMLElement) || !visible(from)) continue;
      cell.classList.remove("grid-moving");
      cell.classList.add("grid-leaving");
      Object.assign(cell.style, {
        // Off its grid area, which would otherwise be its containing block.
        gridArea: "auto",
        left: `${from.left - box.left - grid.clientLeft}px`,
        top: `${from.top - box.top - grid.clientTop}px`,
        width: `${from.width}px`,
        height: `${from.height}px`,
      });
      grid.append(cell);
      leaving.add(cell);
      const animation = play(
        cell,
        [
          { opacity: 1, transform: "none" },
          { opacity: 0, transform: "scale(0.94)" },
        ],
        { duration: LEAVE_MS, easing: EASE_IN },
      );
      const done = () => {
        cell.remove();
        leaving.delete(cell);
        idle();
      };
      if (animation) animation.onfinish = done;
      else done();
    }
    if (active.size > 0 || leaving.size > 0) grid.classList.add("reflowing");
  };
  return { change, cancel };
}
