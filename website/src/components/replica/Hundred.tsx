"use client";
import { useEffect, useState } from "react";
import { sampleAt } from "./model";

// As on the About page, the A resolves into the last ten CPU samples.
export default function Hundred({ tick }: { tick: number }) {
  const [chart, setChart] = useState(false);
  useEffect(() => {
    const delay = matchMedia("(prefers-reduced-motion: reduce)").matches ? 0 : 860;
    const id = setTimeout(() => setChart(true), delay);
    return () => clearTimeout(id);
  }, []);
  return (
    <svg width="72" height="72" className="a-hundred" viewBox="0 0 100 100" aria-hidden="true">
      {Array.from({ length: 100 }, (_, i) => {
        const x = i % 10,
          y = Math.floor(i / 10),
          leg = 4.5 - y * 0.5;
        const big =
          (y === 0 && (x === 4 || x === 5)) ||
          (y === 6 && x >= 2 && x <= 7) ||
          Math.min(Math.abs(x - leg), Math.abs(9 - x - leg)) < 0.44;
        const load = sampleAt(tick - 9 + x).cpu,
          height = Math.max(1, Math.round(load / 10));
        const opacity = chart ? (10 - y === height ? 1 : 10 - y < height ? 0.5 : 0.12) : big ? 1 : 0.22;
        return (
          <circle
            key={i}
            cx={x * 10 + 5}
            cy={y * 10 + 5}
            r={chart ? 3.4 : big ? 4.3 : 1.7}
            fill={chart && load >= 80 ? "var(--a-io)" : "var(--a-accent)"}
            opacity={opacity}
          />
        );
      })}
    </svg>
  );
}
