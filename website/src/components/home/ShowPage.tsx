"use client";
import type { ReactNode } from "react";
import { store, type Page, type Request } from "../replica/store";

// Where a row's link leads when it is not simply a page: the view, device
// and page within it that show what the row describes.
const places: Record<string, Omit<Request, "at">> = {
  tree: { page: "processes", view: "tree" },
  list: { page: "processes", view: "list" },
  history: { page: "history", range: "7d" },
  "cpu-details": { page: "performance", device: "cpu", details: true },
  "cpu-cores": { page: "performance", device: "cpu", cores: true },
  "disk-details": { page: "performance", device: "disk", details: true },
  gpu: { page: "performance", device: "gpu" },
  battery: { page: "performance", device: "battery" },
};

// The comparison rows open the matching place in the window above.
export default function ShowPage({ page, children }: { page: string; children: ReactNode }) {
  return (
    <button
      type="button"
      className="demo-link"
      data-show-page={page}
      onClick={() =>
        store.set({ request: { ...(places[page] ?? { page: page as Page }), at: Date.now() } })
      }
    >
      {children}
    </button>
  );
}
