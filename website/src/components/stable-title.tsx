"use client";
import { useLayoutEffect } from "react";

// On navigation React removes the old page's <title> one commit before it adds
// the next page's, and for that moment the browser tab shows the address. The
// last title therefore stays until its successor is in place. React inserts the
// new title before it and never reuses it: it still carries React's marker.
export function StableTitle() {
  useLayoutEffect(() => {
    const head = document.head;
    const titles = head.getElementsByTagName("title");
    const remove = Node.prototype.removeChild;
    let kept: Node | null = null;
    head.removeChild = function <T extends Node>(this: HTMLHeadElement, child: T): T {
      if (child instanceof HTMLTitleElement && child.parentNode === this && titles.length === 1) {
        kept = child;
        return child;
      }
      return remove.call(this, child) as T;
    };
    const observer = new MutationObserver(() => {
      if (kept && titles.length > 1) {
        remove.call(head, kept);
        kept = null;
      }
    });
    observer.observe(head, { childList: true });
    return () => {
      observer.disconnect();
      Reflect.deleteProperty(head, "removeChild");
      if (kept) remove.call(head, kept);
    };
  }, []);
  return null;
}
