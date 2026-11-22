"use client";
import { useLayoutEffect, useRef, useState } from "react";
import { paint, type RasterRequest } from "./historyPainter";
import type { Trace } from "./historySeries";
import type { Span } from "./historyTime";

type Result = { id: number; revision: number; image: ImageBitmap };
const deliveries = new Map<number, (result: Result) => void>();
let serial = 0,
  worker: Worker | undefined;
function painter() {
  if (typeof OffscreenCanvas === "undefined" || typeof Worker === "undefined") return;
  if (!worker) {
    worker = new Worker(new URL("./history.worker.ts", import.meta.url), { type: "module" });
    worker.onmessage = ({ data }: MessageEvent<Result>) => {
      const receive = deliveries.get(data.id);
      if (receive) receive(data);
      else data.image.close();
    };
  }
  return worker;
}
const clockShift = (anchor: number | null) =>
  anchor === null ? 0 : Math.max(0, Math.min(2, (performance.now() - anchor) / 1000));
export function glide(el: HTMLElement, from: number, to: number, duration: number, mode = "smooth") {
  for (const a of el.getAnimations()) a.cancel();
  el.style.transform = "translateX(" + to + "px)";
  if (duration > 0) {
    const steps =
      mode === "pixel"
        ? Math.ceil(Math.abs(to - from) * devicePixelRatio)
        : mode === "balanced"
          ? Math.ceil((duration * 60) / 1000)
          : 0;
    el.animate([{ transform: "translateX(" + from + "px)" }, { transform: "translateX(" + to + "px)" }], {
      duration,
      easing: steps ? "steps(" + Math.max(1, steps) + ",end)" : "linear",
      fill: "forwards",
    });
  }
}

type Surface = { el: HTMLCanvasElement; start: number; length: number; width: number };
export default function HistoryRaster({
  times,
  span,
  traces,
  anchor,
  grid,
  rows = 5,
  layerKey,
  motion = true,
  pace = "smooth",
}: {
  times: number[];
  span: Span;
  traces: Trace[];
  anchor: number | null;
  grid?: number[];
  rows?: number;
  layerKey: string;
  motion?: boolean;
  pace?: string;
}) {
  const host = useRef<HTMLDivElement>(null),
    front = useRef<Surface | null>(null);
  const id = useRef(0),
    revision = useRef(0),
    last = useRef<{ key: string; span: Span; anchor: number | null; yMin: number; yMax: number } | null>(null);
  const [size, setSize] = useState({ w: 0, h: 0, k: 1 });
  useLayoutEffect(() => {
    const el = host.current!;
    const measure = () => {
      const r = el.getBoundingClientRect(),
        w = el.offsetWidth,
        h = el.offsetHeight,
        k = (r.width / Math.max(1, w)) * devicePixelRatio;
      setSize((prev) => (prev.w === w && prev.h === h && Math.abs(prev.k - k) < 0.001 ? prev : { w, h, k }));
    };
    measure();
    const ro = new ResizeObserver(measure);
    ro.observe(el);
    window.addEventListener("resize", measure);
    id.current = ++serial;
    return () => {
      ro.disconnect();
      window.removeEventListener("resize", measure);
      deliveries.delete(id.current);
      front.current = null;
      last.current = null;
      el.replaceChildren();
    };
  }, []);
  const gridKey = grid?.join(",");
  useLayoutEffect(() => {
    const el = host.current;
    if (!el || size.w <= 0 || size.h <= 0) return;
    const rev = ++revision.current,
      style = getComputedStyle(el);
    const css = (c: string) => {
      const m = /^var\((--[\w-]+)\)$/.exec(c);
      return m ? style.getPropertyValue(m[1]).trim() || "#828a91" : c;
    };
    const start = times[0],
      length = times.at(-1)! - start,
      spanLength = span[1] - span[0],
      width = (size.w * length) / spanLength;
    const r: RasterRequest = {
      id: id.current,
      revision: rev,
      width: Math.max(1, Math.ceil(width * size.k)),
      height: Math.max(1, Math.ceil(size.h * size.k)),
      ratio: size.k,
      traces: traces.map(({ values, min, max, color, fill, opacity, low, high, dash }) => ({
        dash,
        values,
        min,
        max,
        color: css(color),
        fill,
        opacity,
        low,
        high,
      })),
      gapColor: css("var(--a-stroke-strong)"),
      grid: grid ? { ticks: grid.map((t) => (t - start) / length), rows, color: css("var(--a-stroke)") } : undefined,
    };
    const present = (image?: ImageBitmap) => {
      if (rev !== revision.current || !host.current) {
        image?.close();
        return;
      }
      const previous = last.current;
      const changing =
        !!previous &&
        (previous.key !== layerKey ||
          previous.yMin !== traces[0].min ||
          previous.yMax !== traces[0].max ||
          Math.abs(Math.log(spanLength / (previous.span[1] - previous.span[0]))) > 0.001);
      const blend = changing && motion && !matchMedia("(prefers-reduced-motion: reduce)").matches;
      const old = front.current;
      const canvas = (!blend && old?.el) || document.createElement("canvas");
      if (canvas.width !== r.width) canvas.width = r.width;
      if (canvas.height !== r.height) canvas.height = r.height;
      canvas.style.width = width + "px";
      canvas.style.height = size.h + "px";
      canvas.style.transformOrigin = "0 0";
      const ctx = canvas.getContext("2d")!;
      if (image) {
        ctx.clearRect(0, 0, canvas.width, canvas.height);
        ctx.drawImage(image, 0, 0);
        image.close();
      } else paint(ctx, r);
      if (canvas.parentNode !== el) el.append(canvas);
      const left = ((start - span[0]) * size.w) / spanLength,
        drift = size.w / spanLength;
      const elapsed = clockShift(anchor);
      const pos = left - elapsed * drift;
      glide(
        canvas,
        pos,
        anchor === null ? pos : left - 2 * drift,
        anchor === null ? 0 : Math.max(0, (2 - elapsed) * 1000),
        pace,
      );
      if (blend && old && previous) {
        const duration = Math.min(
          420,
          250 + 30 * Math.abs(Math.log2(spanLength / (previous.span[1] - previous.span[0]))),
        );
        const prevSpan = previous.span[1] - previous.span[0],
          prevStart = previous.span[0] + clockShift(previous.anchor);
        const oldAt = ((old.start - span[0]) * size.w) / spanLength;
        const previousHeight = previous.yMax - previous.yMin,
          nextHeight = traces[0].max - traces[0].min;
        const oldY = (size.h * (traces[0].max - previous.yMax)) / nextHeight,
          newY = (size.h * (previous.yMax - traces[0].max)) / previousHeight;
        const fading = old.el.animate(
          [
            { transform: getComputedStyle(old.el).transform, opacity: 1 },
            {
              transform:
                "translate(" +
                oldAt +
                "px," +
                oldY +
                "px) scale(" +
                (old.length * size.w) / (spanLength * old.width) +
                "," +
                previousHeight / nextHeight +
                ")",
              opacity: 0,
            },
          ],
          { duration, easing: "ease-in-out", fill: "forwards" },
        );
        fading.onfinish = () => old.el.remove();
        canvas.animate(
          [
            {
              transform:
                "translate(" +
                ((start - prevStart) * size.w) / prevSpan +
                "px," +
                newY +
                "px) scale(" +
                spanLength / prevSpan +
                "," +
                nextHeight / previousHeight +
                ")",
              opacity: 0,
            },
            { transform: "translateX(" + pos + "px)", opacity: 1 },
          ],
          { duration, easing: "ease-in-out" },
        );
      }
      front.current = { el: canvas, start, length, width };
      last.current = { key: layerKey, span: [...span], anchor, yMin: traces[0].min, yMax: traces[0].max };
    };
    const w = painter();
    if (!w) {
      present();
      return;
    }
    deliveries.set(id.current, ({ revision: response, image }) => {
      if (response === rev) present(image);
      else image.close();
    });
    w.postMessage(r);
  }, [size, times, traces, span[0], span[1], anchor, gridKey, rows, layerKey, motion, pace]);
  return <div ref={host} className="h-raster" aria-hidden="true" />;
}
