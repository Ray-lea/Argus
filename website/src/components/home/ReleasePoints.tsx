"use client";
import { useRef, useState } from "react";
import { flushSync } from "react-dom";

// A release's named entries: the first few, and a "+N" chip that opens the
// rest. The list eases to its new height, the added chips rise in one after
// another, and on closing they fade before the list closes over them.
const ease = "cubic-bezier(0.2, 0.8, 0.2, 1)";
export default function ReleasePoints({
  items,
  limit = 6,
  show,
  hide,
}: {
  items: { kind: string; label: string }[];
  limit?: number;
  show: string;
  hide: string;
}) {
  const [open, setOpen] = useState(false);
  const list = useRef<HTMLUListElement>(null);
  const rest = items.length - limit;
  const toggle = () => {
    const el = list.current;
    if (!el) return;
    const still = matchMedia("(prefers-reduced-motion: reduce)").matches;
    const resize = () => {
      const from = el.offsetHeight;
      flushSync(() => setOpen(!open));
      const to = el.offsetHeight;
      if (!still && from !== to) el.animate({ height: [from + "px", to + "px"] }, { duration: 320, easing: ease });
    };
    if (still) return resize();
    if (!open) {
      resize();
      el.querySelectorAll(".is-extra").forEach((chip, i) =>
        chip.animate(
          { opacity: [0, 1], transform: ["translateY(6px) scale(0.96)", "none"] },
          { duration: 260, delay: 60 + i * 16, easing: ease, fill: "backwards" },
        ),
      );
      return;
    }
    const fading = [...el.querySelectorAll(".is-extra")].map((chip) =>
      chip.animate({ opacity: [1, 0], transform: ["none", "translateY(4px)"] }, { duration: 140, easing: "ease-in", fill: "forwards" }),
    );
    Promise.all(fading.map((a) => a.finished)).then(resize, resize);
  };
  return (
    <ul className="release-points" ref={list}>
      {(open ? items : items.slice(0, limit)).map((item, i) => (
        <li key={item.kind + item.label} className={"is-" + item.kind + (i >= limit ? " is-extra" : "")}>
          {item.label}
        </li>
      ))}
      {rest > 0 && (
        <li className="is-more">
          <button type="button" aria-expanded={open} aria-label={open ? hide : show} onClick={toggle}>
            {open ? hide : "+" + rest}
          </button>
        </li>
      )}
    </ul>
  );
}
