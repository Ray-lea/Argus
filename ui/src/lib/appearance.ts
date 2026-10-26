import { createEffect } from "solid-js";
import { persisted } from "./prefs";

export const [uiScale, setUiScale] = persisted(
  "uiScale",
  100,
  (v): v is number => typeof v === "number" && Number.isInteger(v) && v >= 80 && v <= 150,
);
export const [fontSize, setFontSize] = persisted(
  "fontSize",
  13,
  (v): v is number => typeof v === "number" && Number.isInteger(v) && v >= 11 && v <= 20,
);

export function followAppearance() {
  createEffect(() => {
    document.documentElement.style.fontSize = `${fontSize()}px`;
    // The host uses WebView zoom, keeping viewport coordinates, canvas pixels
    // and native hit testing consistent. CSS zoom is only for the web preview.
    if (!("__TAURI_INTERNALS__" in window)) document.documentElement.style.zoom = String(uiScale() / 100);
  });
}
