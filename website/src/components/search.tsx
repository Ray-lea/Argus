"use client";
import type { SharedProps } from "fumadocs-ui/components/dialog/search";
import { lazy, useState } from "react";

// The dialog, its search engine and the Chinese word segmenter load when search
// is first opened (or hovered, see preloadSearch), not with every page.
const load = () => import("./search-dialog");
const Dialog = lazy(load);
export const preloadSearch = () => void load();

export default function SearchDialog(props: SharedProps) {
  const [used, setUsed] = useState(false);
  if (props.open && !used) setUsed(true);
  return used ? <Dialog {...props} /> : null;
}
