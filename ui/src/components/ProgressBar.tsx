// A progress bar (the update download): a known amount fills from the left,
// gliding to each new report on the compositor like the core bars
// (`Vitals.tsx`, not a CSS transition: see ui/AGENTS.md); an unknown one shows
// a segment sweeping across.

import { createEffect, onCleanup, Show, untrack } from "solid-js";
import { uiMotion } from "../lib/motion";

/** The glide to a new amount: as long as the host waits between reports (PROGRESS_EVERY, updater.rs). */
const GLIDE_MS = 250;

/** `value` from 0 to 1, or null when the total is not known. */
export default function ProgressBar(props: { value: number | null; class?: string }) {
  return (
    <Show
      when={props.value !== null}
      fallback={<div class={`progress ${props.class ?? ""}`} role="progressbar" />}
    >
      <Fill value={() => props.value ?? 0} class={props.class} />
    </Show>
  );
}

function Fill(props: { value: () => number; class?: string }) {
  let fill!: HTMLDivElement;
  let anim: Animation | null = null;
  // The last glide: from, to, and when it started (performance.now()); none
  // before the first amount, which shows as it is.
  let placed = false;
  let from = 0;
  let to = 0;
  let start = 0;
  const amount = () => Math.max(0, Math.min(1, props.value()));

  createEffect(() => {
    const target = amount();
    untrack(() => {
      const now = performance.now();
      const at = placed ? from + (to - from) * Math.min(1, (now - start) / GLIDE_MS) : target;
      placed = true;
      anim?.cancel();
      anim = null;
      fill.style.transform = `scaleX(${target})`;
      if (at === target || !uiMotion()) {
        from = to = target;
        return;
      }
      from = at;
      to = target;
      start = now;
      anim = fill.animate([{ transform: `scaleX(${at})` }, { transform: `scaleX(${target})` }], {
        duration: GLIDE_MS,
        easing: "linear",
      });
      anim.startTime = now;
    });
  });
  onCleanup(() => anim?.cancel());

  return (
    <div
      class={`progress determinate ${props.class ?? ""}`}
      role="progressbar"
      aria-valuemin={0}
      aria-valuemax={100}
      aria-valuenow={Math.round(amount() * 100)}
    >
      <div ref={fill} class="progress-fill" />
    </div>
  );
}
