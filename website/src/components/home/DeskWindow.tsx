/** @jsxImportSource @/components/replica/i18n */
"use client";
import { useRef, useState, type CSSProperties, type PointerEvent as ReactPointerEvent, type ReactNode } from "react";

// The demo window on the 1920 × 1080 desktop: it opens in the middle of the
// work area above the taskbar and resizes from its edges and corners, between
// the app's minimum window (resources/app.json) and the work area.
const AREA = { w: 1920, h: 1032 };
const MIN = { w: 640, h: 480 };
const edges = ["n", "s", "e", "w", "ne", "nw", "se", "sw"] as const;
type Edge = (typeof edges)[number];
const clamp = (v: number, low: number, high: number) => Math.max(low, Math.min(high, v));

export default function DeskWindow({
  width,
  height,
  resizable = true,
  children,
}: {
  width: number;
  height: number;
  resizable?: boolean;
  children: ReactNode;
}) {
  const self = useRef<HTMLDivElement>(null);
  const [rect, setRect] = useState(() => ({ x: (AREA.w - width) / 2, y: (AREA.h - height) / 2, w: width, h: height }));
  const resize = (edge: Edge) => (event: ReactPointerEvent<HTMLElement>) => {
    if (event.button !== 0) return;
    event.preventDefault();
    const desk = self.current?.parentElement;
    if (!desk) return;
    // Pointer moves in screen pixels; the desktop is drawn scaled.
    const k = desk.getBoundingClientRect().width / desk.offsetWidth;
    const from = rect,
      x0 = event.clientX,
      y0 = event.clientY;
    const handle = event.currentTarget;
    handle.setPointerCapture(event.pointerId);
    const move = (e: PointerEvent) => {
      const dx = (e.clientX - x0) / k,
        dy = (e.clientY - y0) / k;
      let { x, y, w, h } = from;
      if (edge.includes("e")) w = clamp(from.w + dx, MIN.w, AREA.w - from.x);
      if (edge.includes("s")) h = clamp(from.h + dy, MIN.h, AREA.h - from.y);
      if (edge.includes("w")) {
        x = clamp(from.x + dx, 0, from.x + from.w - MIN.w);
        w = from.w + from.x - x;
      }
      if (edge.includes("n")) {
        y = clamp(from.y + dy, 0, from.y + from.h - MIN.h);
        h = from.h + from.y - y;
      }
      setRect({ x: Math.round(x), y: Math.round(y), w: Math.round(w), h: Math.round(h) });
    };
    const end = () => {
      handle.removeEventListener("pointermove", move);
      handle.removeEventListener("pointerup", end);
      handle.removeEventListener("pointercancel", end);
    };
    handle.addEventListener("pointermove", move);
    handle.addEventListener("pointerup", end);
    handle.addEventListener("pointercancel", end);
  };
  return (
    <div
      className="window"
      ref={self}
      style={{ "--wx": rect.x + "px", "--wy": rect.y + "px", "--ww": rect.w + "px", "--wh": rect.h + "px" } as CSSProperties}
    >
      {children}
      {resizable && edges.map((edge) => <i key={edge} className={"win-edge is-" + edge} onPointerDown={resize(edge)} aria-hidden="true" />)}
    </div>
  );
}
