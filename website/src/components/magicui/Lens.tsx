"use client";

// Adapted from Magic UI's Lens (MIT). The upstream license ships in /assets/magicui-license.txt.
import { useId, useState, type PointerEvent, type ReactNode } from "react";
import { AnimatePresence, motion, useReducedMotion } from "motion/react";
import { useInteractive } from "../../lib/use-interactive";

type Point = { x: number; y: number };
export function Lens({
  children,
  label,
}: {
  children: ReactNode;
  label: string;
}) {
  const [point, setPoint] = useState<Point | null>(null);
  const reduced = useReducedMotion();
  const interactive = useInteractive();
  const id = useId();
  function move(event: PointerEvent<HTMLDivElement>) {
    if (event.pointerType === "touch") return;
    const rect = event.currentTarget.getBoundingClientRect();
    setPoint({ x: event.clientX - rect.left, y: event.clientY - rect.top });
  }
  return (
    <div
      className="lens"
      onPointerMove={move}
      onPointerLeave={() => setPoint(null)}
      onFocus={(e) => {
        if (e.target === e.currentTarget)
          setPoint({
            x: e.currentTarget.clientWidth / 2,
            y: e.currentTarget.clientHeight / 2,
          });
      }}
      onBlur={() => setPoint(null)}
      role="region"
      tabIndex={interactive ? 0 : -1}
      aria-label={label}
      aria-describedby={id}
      onKeyDown={(e) => {
        if (e.key === "Escape") {
          setPoint(null);
          return;
        }
        const step: Record<string, Point> = {
          ArrowLeft: { x: -24, y: 0 },
          ArrowRight: { x: 24, y: 0 },
          ArrowUp: { x: 0, y: -24 },
          ArrowDown: { x: 0, y: 24 },
        };
        if (!step[e.key]) return;
        e.preventDefault();
        const { clientWidth: w, clientHeight: h } = e.currentTarget,
          delta = step[e.key];
        setPoint((p) => ({
          x: Math.max(0, Math.min(w, (p?.x ?? w / 2) + delta.x)),
          y: Math.max(0, Math.min(h, (p?.y ?? h / 2) + delta.y)),
        }));
      }}
    >
      {children}
      <span id={id} className="sr-only">
        {label}
      </span>
      <AnimatePresence>
        {point && (
          <motion.div
            aria-hidden="true"
            className="lens-glass"
            initial={{ opacity: 0 }}
            animate={{ opacity: 1 }}
            exit={{ opacity: 0 }}
            transition={{ duration: reduced ? 0 : 0.12 }}
            style={{ clipPath: `circle(82px at ${point.x}px ${point.y}px)` }}
          >
            <div
              style={{
                transform: "scale(1.9)",
                transformOrigin: `${point.x}px ${point.y}px`,
              }}
            >
              {children}
            </div>
          </motion.div>
        )}
      </AnimatePresence>
    </div>
  );
}
