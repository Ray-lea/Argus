/** @jsxImportSource @/components/replica/i18n */
"use client";
import { useCallback, useRef, useState, type ReactNode } from "react";
import { Icon, type UiIcon } from "./icons";
import { scaleOf, useIsoLayoutEffect } from "./widgets";

export default function SectionPage({
  title,
  entries,
  target,
  children,
}: {
  title: string;
  entries: [string, string, UiIcon][];
  target?: string;
  children: ReactNode;
}) {
  const root = useRef<HTMLDivElement>(null);
  const [current, setCurrent] = useState(0);
  const picked = useRef(false);
  const jump = useCallback((index: number, animate: boolean) => {
    const el = root.current;
    const node = el?.querySelectorAll<HTMLElement>(".a-settings-body > section")[index];
    if (!el || !node) return;
    picked.current = true;
    setCurrent(index);
    const top = el.scrollTop + (node.getBoundingClientRect().top - el.getBoundingClientRect().top) / scaleOf(el) - 4;
    el.scrollTo({
      top,
      behavior: animate && !matchMedia("(prefers-reduced-motion: reduce)").matches ? "smooth" : "instant",
    });
  }, []);
  useIsoLayoutEffect(() => {
    const i = entries.findIndex(([id]) => id === target);
    if (i >= 0) jump(i, false);
  }, [entries, target, jump]);
  const follow = () => {
    const el = root.current;
    if (!el || picked.current) return;
    const h = el.clientHeight,
      y = el.scrollTop,
      max = el.scrollHeight - h;
    const line = h * (0.1 + 0.25 * Math.min(y / (h / 2), 1) + 0.6 * Math.max(0, 1 - (max - y) / (h / 2)));
    const top = el.getBoundingClientRect().top + line * scaleOf(el);
    let selected = 0;
    el.querySelectorAll(".a-settings-body > section").forEach((node, i) => {
      if (node.getBoundingClientRect().top <= top) selected = i;
    });
    setCurrent(selected);
  };
  return (
    <div
      ref={root}
      className="a-page a-settings a-scroll"
      onScroll={follow}
      onWheel={() => (picked.current = false)}
      onPointerDown={() => (picked.current = false)}
    >
      <div className="a-settings-inner">
        <nav className="a-settings-rail" role="tablist" aria-label={title}>
          <span className="a-settings-mark" style={{ transform: "translateY(" + current * 38 + "px)" }} />
          {entries.map(([id, name, icon], i) => (
            <button
              key={id}
              type="button"
              role="tab"
              aria-selected={i === current}
              aria-current={i === current || undefined}
              onClick={() => jump(i, true)}
            >
              <Icon name={icon} />
              {name}
            </button>
          ))}
        </nav>
        <div className="a-settings-body">{children}</div>
      </div>
    </div>
  );
}
