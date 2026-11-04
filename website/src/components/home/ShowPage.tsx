"use client";
import type { ReactNode } from "react";
import { store, type Page } from "../replica/store";

const views: Record<string, "tree" | "list"> = { tree: "tree", list: "list" };

// The comparison rows open the matching page in the window above.
export default function ShowPage({ page, children }: { page: string; children: ReactNode }) {
  return (
    <button
      type="button"
      className="demo-link"
      data-show-page={page}
      onClick={() =>
        store.set({
          request: {
            page: (views[page] ? "processes" : page) as Page,
            view: views[page],
            at: Date.now(),
          },
        })
      }
    >
      {children}
    </button>
  );
}
