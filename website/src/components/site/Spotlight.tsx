"use client";
import { useEffect } from "react";

// Cards marked data-spotlight take a soft light under the pointer.
export function Spotlight() {
  useEffect(() => {
    let frame = 0;
    const move = (event: PointerEvent) => {
      if (event.pointerType !== "mouse") return;
      cancelAnimationFrame(frame);
      frame = requestAnimationFrame(() => {
        for (const card of document.querySelectorAll<HTMLElement>("[data-spotlight]")) {
          const box = card.getBoundingClientRect();
          const inside =
            event.clientX >= box.left - 80 &&
            event.clientX <= box.right + 80 &&
            event.clientY >= box.top - 80 &&
            event.clientY <= box.bottom + 80;
          card.style.setProperty("--lit", inside ? "1" : "0");
          if (inside) {
            card.style.setProperty("--x", event.clientX - box.left + "px");
            card.style.setProperty("--y", event.clientY - box.top + "px");
          }
        }
      });
    };
    document.addEventListener("pointermove", move, { passive: true });
    return () => {
      cancelAnimationFrame(frame);
      document.removeEventListener("pointermove", move);
    };
  }, []);
  return null;
}
