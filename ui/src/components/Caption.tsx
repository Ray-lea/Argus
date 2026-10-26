// The window's caption buttons: the window has no system title bar, the
// toolbar is the caption. Over the maximize button lies a native overlay
// (src-tauri/src/caption.rs) that makes Windows 11 offer snap layouts; it takes
// the pointer, so its hover and press arrive as events and are drawn here.

import { createSignal, onCleanup, onMount } from "solid-js";
import { t } from "../lib/i18n";
import { api, inTauri } from "../lib/ipc";

type Pointer = "hover" | "press" | "leave";

// Segoe Fluent Icons (Segoe MDL2 Assets on Windows 10): the system's glyphs.
const GLYPH = { minimize: "", maximize: "", restore: "", close: "" };

export default function Caption() {
  const [maximized, setMaximized] = createSignal(false);
  const [active, setActive] = createSignal(true);
  const [pointer, setPointer] = createSignal<Pointer>("leave");
  let maximize!: HTMLButtonElement;
  let disposed = false;
  const cleanups: (() => void)[] = [];
  onCleanup(() => {
    disposed = true;
    for (const stop of cleanups) stop();
  });

  const win = () => import("@tauri-apps/api/window").then((m) => m.getCurrentWindow());

  onMount(async () => {
    if (!inTauri) return;
    const [w, { listen }] = await Promise.all([win(), import("@tauri-apps/api/event")]);
    const report = async () => {
      const scale = await w.scaleFactor();
      if (disposed) return;
      const r = maximize.getBoundingClientRect();
      const zoom = devicePixelRatio / scale;
      void api.setCaptionButton({
        right: (window.innerWidth - r.right) * zoom,
        top: r.top * zoom,
        width: r.width * zoom,
        height: r.height * zoom,
      });
    };
    if (disposed) return;
    const observer = new ResizeObserver(() => void report());
    observer.observe(maximize);
    window.addEventListener("resize", report);
    cleanups.push(() => {
      observer.disconnect();
      window.removeEventListener("resize", report);
    });
    void report();
    const sync = async () => setMaximized(await w.isMaximized());
    const stops = await Promise.all([
      w.onResized(() => void sync()),
      w.onFocusChanged(({ payload }) => setActive(payload)),
      listen<Pointer>("argus-caption-maximize", (e) => setPointer(e.payload)),
    ]);
    if (disposed) for (const stop of stops) stop();
    else cleanups.push(...stops);
    void sync();
  });

  return (
    <div class="caption" classList={{ inactive: !active() }}>
      <button
        type="button"
        tabIndex={-1}
        title={t("shell.caption.minimize")}
        aria-label={t("shell.caption.minimize")}
        onClick={() => void api.minimize()}
      >
        {GLYPH.minimize}
      </button>
      <button
        ref={maximize}
        type="button"
        tabIndex={-1}
        classList={{ hover: pointer() === "hover", press: pointer() === "press" }}
        aria-label={maximized() ? t("shell.caption.restore") : t("shell.caption.maximize")}
        onClick={() => void win().then((w) => w.toggleMaximize())}
      >
        {maximized() ? GLYPH.restore : GLYPH.maximize}
      </button>
      <button
        type="button"
        class="close"
        tabIndex={-1}
        title={t("shell.caption.close")}
        aria-label={t("shell.caption.close")}
        onClick={() => void win().then((w) => w.close())}
      >
        {GLYPH.close}
      </button>
    </div>
  );
}
