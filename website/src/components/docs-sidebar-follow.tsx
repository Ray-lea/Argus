"use client";

import { usePathname } from "next/navigation";
import { useEffect } from "react";

// Fumadocs opens a sidebar folder when the page being read moves into it, but
// not when the reader closed it earlier and then opens another page inside it.
// After every change of page this opens the folders that lead to the page,
// outermost first; the sidebar then scrolls the page's entry into view itself.

/** The names of the folders that lead to each page, by URL without the trailing slash. */
export type SidebarTrails = Record<string, string[]>;

const nextFrame = () => new Promise<void>((resolve) => requestAnimationFrame(() => resolve()));

// The folder named `name` in the sidebar: its root carries data-open or
// data-closed, its first child is the trigger (a button, or the link of a
// folder with a page of its own).
function findFolder(name: string) {
  for (const root of document.querySelectorAll("#nd-sidebar [data-open], #nd-sidebar [data-closed]")) {
    const trigger = root.firstElementChild;
    if (trigger instanceof HTMLElement && trigger.innerText.trim() === name) return { root, trigger };
  }
}

export function SidebarFollow({ trails }: { trails: SidebarTrails }) {
  const pathname = usePathname();
  useEffect(() => {
    const names = trails[pathname.replace(/\/$/, "")] ?? [];
    let cancelled = false;
    (async () => {
      for (const name of names) {
        // An outer folder that just opened renders its children on a later frame.
        let folder = findFolder(name);
        for (let frame = 0; !folder && frame < 30 && !cancelled; frame++) {
          await nextFrame();
          folder = findFolder(name);
        }
        if (cancelled || !folder) return;
        if (!folder.root.hasAttribute("data-closed")) continue;
        // A folder link toggles from its chevron without leaving the page.
        const toggle = folder.trigger.tagName === "A" ? folder.trigger.querySelector("[data-icon]") : folder.trigger;
        toggle?.dispatchEvent(new MouseEvent("click", { bubbles: true, cancelable: true }));
        await nextFrame();
      }
    })();
    return () => {
      cancelled = true;
    };
  }, [pathname, trails]);
  return null;
}
