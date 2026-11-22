"use client";
import { useEffect, useState } from "react";
import { sampleAt } from "./model";

// The hundred dots (百, frontend/hundred.rs): a 10 × 10 lattice whose bigger
// dots draw the A. They grow in along the diagonal, the A lights up, then the
// lattice turns into a dot-matrix chart of the last ten CPU samples: a column
// per sample lit up to its load, amber from 80 %, unlit dots faint ink.
const FORMED = 560,
  LIT: [number, number] = [440, 720],
  TURN: [number, number] = [860, 1560];
const BIG = 0.43,
  SMALL = 0.17,
  EVEN = 0.34,
  FAINT = 0.12;
// theme::ease_out, cubic-bezier(0.1, 0.9, 0.2, 1).
function ease(t: number) {
  t = Math.min(1, Math.max(0, t));
  const at = (u: number, a: number, b: number) => 3 * (1 - u) * (1 - u) * u * a + 3 * (1 - u) * u * u * b + u * u * u;
  let lo = 0,
    hi = 1,
    u = t;
  for (let i = 0; i < 20; i++) {
    u = (lo + hi) / 2;
    if (at(u, 0.1, 0.2) < t) lo = u;
    else hi = u;
  }
  return at(u, 0.9, 1);
}
const span = (elapsed: number, [from, to]: [number, number]) => ease((elapsed - from) / (to - from));
// Two legs leaning in, a pair at the apex and the bar.
const inA = (x: number, y: number) => {
  const centre = 4.5 - 0.5 * y;
  const leg = (at: number) => Math.min(1, Math.max(0, 1 - Math.abs(at - centre) * 1.15));
  return (y === 0 && (x === 4 || x === 5)) || (y === 6 && x >= 2 && x <= 7) || Math.max(leg(x), leg(9 - x)) > 0.5;
};
// How a chart dot shows its column's load: opacity and how much it takes the tint.
function light(load: number, row: number): [number, number] {
  const lit = Math.min(10, Math.max(1, Math.round(load * 10)));
  const height = 10 - row;
  return height === lit ? [1, 1] : height < lit ? [0.5, 1] : [FAINT, 0];
}

export default function Hundred({ tick, size = 72 }: { tick: number; size?: number }) {
  const [elapsed, setElapsed] = useState(() =>
    typeof matchMedia !== "undefined" && matchMedia("(prefers-reduced-motion: reduce)").matches ? Infinity : 0,
  );
  // The intro plays each time the dots are shown, then they follow the samples.
  useEffect(() => {
    if (elapsed === Infinity) return;
    const start = performance.now();
    let frame = requestAnimationFrame(function step(now) {
      const t = now - start;
      setElapsed(t >= TURN[1] ? Infinity : t);
      if (t < TURN[1]) frame = requestAnimationFrame(step);
    });
    return () => cancelAnimationFrame(frame);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);
  const lit = span(elapsed, LIT),
    turn = span(elapsed, TURN);
  // The dark theme's resting A is a little stronger than the light one's (0.36).
  const resting = 0.42;
  const dots = [];
  for (let row = 0; row < 10; row++)
    for (let column = 0; column < 10; column++) {
      const load = Math.min(1, Math.max(0, sampleAt(tick - 9 + column).cpu / 100));
      const [cAlpha, cTint] = light(load, row);
      const cWarm = load >= 0.8 ? 1 : 0;
      let radius: number, alpha: number, tint: number, warm: number;
      if (turn >= 1) [radius, alpha, tint, warm] = [EVEN, cAlpha, cTint, cWarm];
      else {
        const start = ((row + column) / 18) * (FORMED - 180);
        const grown = ease((elapsed - start) / 180);
        if (grown <= 0) continue;
        const a = inA(column, row);
        const [r0, a0, t0] = a ? [BIG, resting + (1 - resting) * lit, 1] : [SMALL, FAINT, 0];
        radius = (r0 + (EVEN - r0) * turn) * (0.4 + 0.6 * grown);
        alpha = (a0 + (cAlpha - a0) * turn) * grown;
        tint = t0 + (cTint - t0) * turn;
        warm = cWarm * turn;
      }
      const hue = warm >= 1 ? "var(--a-io)" : warm > 0 ? `color-mix(in srgb, var(--a-io) ${warm * 100}%, var(--a-accent))` : "var(--a-accent)";
      const fill = tint >= 1 ? hue : tint <= 0 ? "var(--a-ink)" : `color-mix(in srgb, ${hue} ${tint * 100}%, var(--a-ink))`;
      dots.push(<circle key={row * 10 + column} cx={column * 10 + 5} cy={row * 10 + 5} r={radius * 10} style={{ fill }} opacity={alpha} />);
    }
  return (
    <svg width={size} height={size} className="a-hundred" viewBox="0 0 100 100" aria-hidden="true">
      {dots}
    </svg>
  );
}
