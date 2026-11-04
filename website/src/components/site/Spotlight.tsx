"use client";
import { useEffect } from "react";

// Cards marked data-spotlight take a soft light under the pointer. Only cards
// on screen are measured, and a card's style changes only when its light
// moves or switches, so a pointer moving elsewhere restyles nothing.
export function Spotlight() {
  useEffect(() => {
    const visible = new Set<HTMLElement>();
    const lit = new Set<HTMLElement>();
    const off = (card: HTMLElement) => {
      if (lit.delete(card)) card.style.setProperty("--lit", "0");
    };
    const observer = new IntersectionObserver((entries) => {
      for (const entry of entries) {
        const card = entry.target as HTMLElement;
        if (entry.isIntersecting) visible.add(card);
        else {
          visible.delete(card);
          off(card);
        }
      }
    });
    for (const card of document.querySelectorAll<HTMLElement>("[data-spotlight]")) observer.observe(card);
    let frame = 0,
      x = 0,
      y = 0;
    const update = () => {
      frame = 0;
      for (const card of visible) {
        const box = card.getBoundingClientRect();
        if (x < box.left - 80 || x > box.right + 80 || y < box.top - 80 || y > box.bottom + 80) {
          off(card);
          continue;
        }
        card.style.setProperty("--x", x - box.left + "px");
        card.style.setProperty("--y", y - box.top + "px");
        if (!lit.has(card)) {
          lit.add(card);
          card.style.setProperty("--lit", "1");
        }
      }
    };
    const move = (event: PointerEvent) => {
      if (event.pointerType !== "mouse") return;
      x = event.clientX;
      y = event.clientY;
      if (!frame) frame = requestAnimationFrame(update);
    };
    document.addEventListener("pointermove", move, { passive: true });
    return () => {
      cancelAnimationFrame(frame);
      observer.disconnect();
      document.removeEventListener("pointermove", move);
    };
  }, []);
  return null;
}
