// How often Argus samples while the window is minimized and while it is in
// the tray (native settings, sampler.rs): the choices, the confirmation for
// short ones, and the notice on coming back that the window refreshed less
// often meanwhile. The two notices are turned off separately. Also tells the
// host when the page has drawn itself, so a window started in the tray can
// let its WebView sleep.

import { createEffect, createRoot } from "solid-js";
import { t } from "./i18n";
import { api, inTauri } from "./ipc";
import { persisted } from "./prefs";
import { goToSetting, intervalMs, snapshot } from "./store";
import { confirm, confirmRequest, toast } from "./ui";
import { updateDialog, updateDialogQueued, welcomePending } from "./updates";

/** The cadences on offer, ms: BACKGROUND_INTERVALS in settings.rs. */
export const BACKGROUND_INTERVALS = [500, 1000, 2000, 5000, 10000, 30000];
/** Shorter cadences ask first: they cost noticeably while nobody is looking. */
const WARN_BELOW = 5000;

/** How the window was out of sight: minimized, or in the tray. */
export type Away = "minimized" | "tray";

/** The setting each notice points to, `setting-<id>` on the settings page. */
export const BACKGROUND_SETTING: Record<Away, string> = {
  minimized: "minimizedInterval",
  tray: "trayInterval",
};

const quiet = {
  minimized: persisted<boolean>("notice.minimizedRate", false),
  tray: persisted<boolean>("notice.trayRate", false),
};

const seconds = (ms: number) => ms / 1000;

/** Whether to save `ms`: anything under 5 s is confirmed first. */
export async function confirmBackgroundInterval(away: Away, ms: number): Promise<boolean> {
  if (ms >= WARN_BELOW) return true;
  return confirm({
    title: t(`shell.background.confirm.${away}.title`),
    body: t(`shell.background.confirm.${away}.body`, { seconds: seconds(ms) }),
    confirmLabel: t("shell.background.confirm.accept", { seconds: seconds(ms) }),
  });
}

/**
 * Coming back: when the window sampled less often while it was away than it
 * does in front, says so once, with the way to that setting and a way to
 * stop the notice. Not after a moment away, when no tick was slower, and not
 * when `yields` (the window comes back to something with a stronger claim)
 * or a dialog is up by the time it would show (`dialogUp`).
 */
async function noticeRestored(away: Away, awayMs: number, yields: Promise<boolean>, dialogUp: () => boolean) {
  const [isQuiet, setQuiet] = quiet[away];
  if (isQuiet() || (await yields)) return;
  const settings = await api.settings().catch(() => null);
  if (!settings) return;
  const shown = intervalMs();
  const hidden = away === "minimized" ? settings.minimizedInterval : settings.trayInterval;
  if (hidden <= shown || awayMs <= shown || dialogUp()) return;
  toast("info", t(`shell.background.notice.${away}`, { from: seconds(shown), to: seconds(hidden) }), 10000, {
    actions: [
      {
        label: t("shell.background.notice.adjust"),
        run: () => goToSetting("data", BACKGROUND_SETTING[away]),
      },
      { label: t("shell.background.notice.quiet"), run: () => setQuiet(true) },
    ],
  });
}

/**
 * Follows the host's `argus-restored`; returns a cleanup. In the browser
 * preview, `?restored=minimized` or `?restored=tray` plays one. The notice
 * gives way, that time, to a dialog the window comes back to (`busy`: the
 * onboarding; the update dialog, a confirmation) and to what a new version
 * shows at the first showing after an update or an autostart: one thing at a
 * time, and the notice comes again on a later return.
 */
export async function watchRestores(busy: () => boolean): Promise<() => void> {
  const dialogUp = () =>
    busy() || updateDialog() !== null || updateDialogQueued() || confirmRequest() !== null;
  const restored = (away: Away, awayMs: number) => {
    // Decided as the window comes back, before anything opens for it; the
    // tray's update entry asks for its dialog just after, so look again.
    const yields = dialogUp() ? Promise.resolve(true) : welcomePending();
    void noticeRestored(away, awayMs, yields, dialogUp);
  };
  if (!inTauri) {
    const away = new URLSearchParams(location.search).get("restored");
    const timer = away === "minimized" || away === "tray" ? setTimeout(() => restored(away, 60000), 1500) : 0;
    return () => clearTimeout(timer);
  }
  const { listen } = await import("@tauri-apps/api/event");
  return listen<{ from: Away; awayMs: number }>("argus-restored", (e) =>
    restored(e.payload.from, e.payload.awayMs),
  );
}

/**
 * Tells the host once the page has drawn its first data. A window started in
 * the tray keeps its WebView awake until then: put to sleep before its first
 * frame, it would have nothing to show for a moment when the window appears
 * (shell.rs, `follow_visibility`).
 */
export function reportFirstFrame() {
  if (!inTauri) return;
  createRoot((dispose) =>
    createEffect(() => {
      if (!snapshot()) return;
      queueMicrotask(dispose);
      void drawn().then(() => api.pagePainted().catch(() => {}));
    }),
  );
}

const frame = () => new Promise<number>((resolve) => requestAnimationFrame(resolve));

/** Resolves once the page as it is now, images and fonts included, has been drawn. */
async function drawn() {
  await frame();
  const decoded = [...document.images].map((img) => img.decode().catch(() => {}));
  const timeout = new Promise((resolve) => setTimeout(resolve, 2000));
  await Promise.race([Promise.all([...decoded, document.fonts.ready]), timeout]);
  await frame();
  await frame();
}
