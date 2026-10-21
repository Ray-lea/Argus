// The handle on a pane's edge that sets its width: drag it, use the arrow keys,
// or double-click to go back to the default. The width is only saved
// (onCommit) when a drag ends. Placed by its caller (class); styles.css draws
// the grip.

import { createSignal } from "solid-js";

export default function Splitter(props: {
  width: number;
  min: number;
  max: number;
  /** What a double-click goes back to. */
  initial: number;
  /** 1 for a pane left of the handle (dragging right widens it), -1 for one on its right. */
  grows: 1 | -1;
  label: string;
  class: string;
  onResize: (width: number) => void;
  onCommit: () => void;
}) {
  const [dragging, setDragging] = createSignal(false);
  const begin = (e: PointerEvent) => {
    if (e.button !== 0) return;
    const handle = e.currentTarget as HTMLElement;
    handle.setPointerCapture(e.pointerId);
    setDragging(true);
    const startX = e.clientX;
    const start = props.width;
    let next = start;
    let frame = 0;
    const move = (m: PointerEvent) => {
      next = start + (m.clientX - startX) * props.grows;
      // One layout per frame, however fast the pointer reports.
      frame ||= requestAnimationFrame(() => {
        frame = 0;
        props.onResize(next);
      });
    };
    const end = () => {
      cancelAnimationFrame(frame);
      handle.removeEventListener("pointermove", move);
      handle.removeEventListener("lostpointercapture", end);
      props.onResize(next);
      props.onCommit();
      setDragging(false);
    };
    handle.addEventListener("pointermove", move);
    handle.addEventListener("lostpointercapture", end);
  };
  const key = (e: KeyboardEvent) => {
    const step = (e.key === "ArrowLeft" ? -16 : e.key === "ArrowRight" ? 16 : 0) * props.grows;
    if (step === 0 && e.key !== "Home" && e.key !== "End") return;
    e.preventDefault();
    props.onResize(e.key === "Home" ? props.min : e.key === "End" ? props.max : props.width + step);
    props.onCommit();
  };
  return (
    <div
      class={`splitter ${props.class}`}
      classList={{ dragging: dragging() }}
      role="separator"
      tabIndex={0}
      aria-orientation="vertical"
      aria-label={props.label}
      aria-valuemin={props.min}
      aria-valuemax={props.max}
      aria-valuenow={Math.round(props.width)}
      onPointerDown={begin}
      onKeyDown={key}
      onDblClick={() => {
        props.onResize(props.initial);
        props.onCommit();
      }}
    />
  );
}
