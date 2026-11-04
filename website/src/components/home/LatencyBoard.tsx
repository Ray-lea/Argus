"use client";
import { useEffect, useRef, useState } from "react";

export interface Latency {
  id: string;
  label: string;
  argus: number;
  native: number;
}
interface Props {
  rows: Latency[];
  lang: "zh" | "en";
}
const text = {
  zh: {
    argus: "Argus",
    native: "任务管理器",
    replay: "按实测耗时重放",
    saved: (ms: number) => (ms >= 1000 ? `少等 ${(ms / 1000).toFixed(2)} 秒` : `少等 ${ms} 毫秒`),
    scale: "毫秒",
    caption: (row: Latency) =>
      `${row.label}：Argus ${row.argus} 毫秒，任务管理器 ${row.native} 毫秒。`,
  },
  en: {
    argus: "Argus",
    native: "Task Manager",
    replay: "Replay at measured speed",
    saved: (ms: number) => (ms >= 1000 ? `${(ms / 1000).toFixed(2)} s sooner` : `${ms} ms sooner`),
    scale: "ms",
    caption: (row: Latency) => `${row.label}: Argus ${row.argus} ms, Task Manager ${row.native} ms.`,
  },
};
const ticks = [0, 500, 1000, 1500];

/*
 * Every row draws both medians on one time scale. When the board comes into
 * view the bars grow in real time, so the wait for each tool is felt rather
 * than read; it replays on its own until the visitor asks for a replay.
 */
export default function LatencyBoard({ rows, lang }: Props) {
  const t = text[lang];
  const board = useRef<HTMLDivElement>(null);
  const scale = Math.max(1500, ...rows.map((r) => r.native));
  const [elapsed, setElapsed] = useState(scale);
  const [visible, setVisible] = useState(false);
  const [run, setRun] = useState(0);
  const [auto, setAuto] = useState(true);

  useEffect(() => {
    const el = board.current;
    if (!el) return;
    const io = new IntersectionObserver(([entry]) => setVisible(entry.isIntersecting), {
      threshold: 0.3,
    });
    io.observe(el);
    return () => io.disconnect();
  }, []);
  // The first sight of the board starts a race; while untouched it repeats every few seconds.
  const still = () => matchMedia("(prefers-reduced-motion: reduce)").matches;
  useEffect(() => {
    if (visible && !still()) setRun((r) => r + 1);
  }, [visible]);
  useEffect(() => {
    if (!visible || !auto || still()) return;
    const id = setInterval(() => setRun((r) => r + 1), 7000);
    return () => clearInterval(id);
  }, [visible, auto]);
  useEffect(() => {
    if (!run) return;
    let frame = 0;
    const start = performance.now();
    setElapsed(0);
    const step = (now: number) => {
      const e = now - start;
      setElapsed(Math.min(e, scale));
      if (e < scale) frame = requestAnimationFrame(step);
    };
    frame = requestAnimationFrame(step);
    return () => cancelAnimationFrame(frame);
  }, [run, scale]);

  const bar = (ms: number, own: boolean) => {
    const shown = Math.min(elapsed, ms);
    return (
      <span className={"latency-bar" + (own ? " is-own" : "")}>
        <span className="latency-track">
          <span className="latency-fill" style={{ width: `${(shown / scale) * 100}%` }} />
        </span>
        <span className={"latency-value figures" + (elapsed >= ms ? " is-done" : "")}>
          {Math.round(shown)}
          <small> ms</small>
        </span>
      </span>
    );
  };
  return (
    <div className="latency" ref={board}>
      <div className="latency-key" aria-hidden="true">
        <span className="latency-legend">
          <i className="is-own" />
          {t.argus}
          <i />
          {t.native}
        </span>
        <span className="latency-ticks">
          {ticks.map((tick) => (
            <span key={tick} style={{ left: `${(tick / scale) * 100}%` }}>
              {tick}
            </span>
          ))}
          <em>{t.scale}</em>
        </span>
      </div>
      {rows.map((row) => (
        <button
          type="button"
          className="latency-row"
          key={row.id}
          aria-label={t.caption(row)}
          onClick={() => {
            setAuto(false);
            setRun((r) => r + 1);
          }}
        >
          <span className="latency-op">{row.label}</span>
          <span className="latency-bars">
            {bar(row.argus, true)}
            {bar(row.native, false)}
          </span>
          <span className={"latency-saved figures" + (elapsed >= row.native ? " is-done" : "")}>
            {t.saved(row.native - row.argus)}
          </span>
        </button>
      ))}
      <button
        type="button"
        className="latency-replay"
        onClick={() => {
          setAuto(false);
          setRun((r) => r + 1);
        }}
      >
        {t.replay}
      </button>
    </div>
  );
}
