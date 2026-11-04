"use client";
import { useEffect } from "react";

// Sections marked data-motion hold their looping animations while they are
// scrolled out of view (data-still; see global.css), so a page at rest does
// no work for what nobody sees.
export function MotionGate() {
  useEffect(() => {
    const observer = new IntersectionObserver(
      (entries) => {
        for (const entry of entries) entry.target.toggleAttribute("data-still", !entry.isIntersecting);
      },
      { rootMargin: "120px 0px" },
    );
    for (const el of document.querySelectorAll("[data-motion]")) observer.observe(el);
    return () => observer.disconnect();
  }, []);
  return null;
}
