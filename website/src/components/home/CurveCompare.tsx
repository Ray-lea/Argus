"use client";
import { useEffect, useRef, useState } from "react";
import Curve from "../replica/Curve";
import { sampleAt, span } from "../replica/model";

const text = {
  zh: {
    argus: "Argus",
    argusNote: "连续流动",
    native: "任务管理器",
    nativeNote: "每次刷新跳一格",
    rate: (hz: number) =>
      `此屏幕约 ${hz} Hz：Argus 的曲线每秒移动 ${hz} 次，任务管理器每秒移动 1 次。`,
    measuring: "正在测量此屏幕的刷新率…",
  },
  en: {
    argus: "Argus",
    argusNote: "Flows continuously",
    native: "Task Manager",
    nativeNote: "Jumps once per update",
    rate: (hz: number) =>
      `This display runs at about ${hz} Hz: the Argus curve moves ${hz} times a second, Task Manager's once.`,
    measuring: "Measuring this display's refresh rate…",
  },
};
const common = [48, 50, 60, 75, 90, 100, 120, 144, 165, 180, 200, 240, 280, 360];

// The same recorded CPU load drawn both ways, side by side.
export default function CurveCompare({ lang }: { lang: "zh" | "en" }) {
  const t = text[lang];
  const root = useRef<HTMLDivElement>(null);
  const [tick, setTick] = useState(0);
  const [visible, setVisible] = useState(false);
  const [hz, setHz] = useState<number | null>(null);
  useEffect(() => {
    const io = new IntersectionObserver(([e]) => setVisible(e.isIntersecting));
    if (root.current) io.observe(root.current);
    return () => io.disconnect();
  }, []);
  useEffect(() => {
    if (!visible) return;
    const id = setInterval(() => setTick((v) => v + 1), 1000);
    return () => clearInterval(id);
  }, [visible]);
  // Count animation frames for a second, then snap to the nearest common refresh rate.
  useEffect(() => {
    if (!visible || hz) return;
    let frames = 0,
      first = 0,
      handle = 0;
    const count = (now: number) => {
      if (!first) first = now;
      frames++;
      if (now - first < 1000) handle = requestAnimationFrame(count);
      else {
        const measured = ((frames - 1) * 1000) / (now - first);
        setHz(common.reduce((a, b) => (Math.abs(b - measured) < Math.abs(a - measured) ? b : a)));
      }
    };
    handle = requestAnimationFrame(count);
    return () => cancelAnimationFrame(handle);
  }, [visible, hz]);
  const values = Array.from({ length: span + 2 }, (_, i) => sampleAt(tick - span - 1 + i).cpu);
  const lane = (name: string, note: string, smooth: boolean) => (
    <div className={"compare-lane" + (smooth ? " is-own" : "")}>
      <p>
        <b>{name}</b>
        <span>{note}</span>
      </p>
      <div className="compare-chart">
        <Curve
          series={[{ values, color: "var(--compare-line)" }]}
          max={100}
          tick={tick}
          interval={1000}
          motion={smooth ? "smooth" : "step"}
          running={visible}
        />
      </div>
    </div>
  );
  return (
    <div className="compare" ref={root}>
      <div className="compare-lanes" data-ui>
        {lane(t.argus, t.argusNote, true)}
        {lane(t.native, t.nativeNote, false)}
      </div>
      <p className="compare-rate" aria-live="polite">
        {hz ? t.rate(hz) : t.measuring}
      </p>
    </div>
  );
}
