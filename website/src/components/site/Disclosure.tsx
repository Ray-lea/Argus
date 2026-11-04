"use client";
import { useEffect } from "react";

const EASE = "cubic-bezier(0.2, 0.8, 0.2, 1)";

// A <details data-disclose> opens and closes with its height eased, and its
// content fades in as it opens. The browser's own way (::details-content with
// interpolate-size) is missing from Firefox and Safari, where the page would
// jump. Without script, or with reduced motion, it toggles at once.
export function Disclosure() {
  useEffect(() => {
    const moving = new WeakMap<HTMLDetailsElement, Animation[]>();
    const click = (event: MouseEvent) => {
      const summary = event.target instanceof Element ? event.target.closest("summary") : null;
      const details = summary?.parentElement;
      if (!summary || !(details instanceof HTMLDetailsElement) || !details.hasAttribute("data-disclose")) return;
      if (summary !== details.querySelector(":scope > summary")) return;
      if (matchMedia("(prefers-reduced-motion: reduce)").matches) return;
      event.preventDefault();
      // Mid-way, the new move starts from the height on screen.
      const from = details.offsetHeight;
      for (const animation of moving.get(details) ?? []) animation.cancel();
      const opening = !details.open || details.classList.contains("is-closing");
      details.classList.toggle("is-closing", !opening);
      details.open = true;
      const border = details.offsetHeight - details.clientHeight;
      const to = opening ? details.offsetHeight : summary.offsetHeight + border;
      const duration = Math.round(Math.min(420, Math.max(240, Math.abs(to - from) * 1.6)));
      details.style.overflow = "clip";
      const height = details.animate({ height: [from + "px", to + "px"] }, { duration, easing: EASE });
      const animations = [height];
      const content = details.querySelector<HTMLElement>(":scope > :not(summary)");
      if (content)
        animations.push(
          opening
            ? content.animate(
                { opacity: [0, 1], transform: ["translateY(-6px)", "none"] },
                { duration, easing: EASE },
              )
            : content.animate({ opacity: [1, 0] }, { duration: duration * 0.6, easing: "ease-out", fill: "forwards" }),
        );
      moving.set(details, animations);
      height.onfinish = () => {
        // The closing fade holds until the content is hidden.
        if (!opening) for (const animation of animations) animation.cancel();
        moving.delete(details);
        details.style.overflow = "";
        if (!opening) details.open = false;
        details.classList.remove("is-closing");
      };
    };
    document.addEventListener("click", click);
    return () => document.removeEventListener("click", click);
  }, []);
  return null;
}
