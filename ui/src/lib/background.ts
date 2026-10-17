// How often Argus samples while the window is minimized and while it is in
// the tray (native settings, sampler.rs): the choices, the confirmation for
// short ones, and the notice on coming back that the window refreshed less
// often meanwhile. The two notices are turned off separately.

import { t } from "./i18n";
import { api, inTauri } from "./ipc";
import { persisted } from "./prefs";
import { goToSetting, intervalMs } from "./store";
import { confirm, toast } from "./ui";

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
 * stop the notice. Not after a moment away, when no tick was slower.
 */
async function noticeRestored(away: Away, awayMs: number) {
  const [isQuiet, setQuiet] = quiet[away];
  if (isQuiet()) return;
  const settings = await api.settings().catch(() => null);
  if (!settings) return;
  const shown = intervalMs();
  const hidden = away === "minimized" ? settings.minimizedInterval : settings.trayInterval;
  if (hidden <= shown || awayMs <= shown) return;
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
 * preview, `?restored=minimized` or `?restored=tray` plays one.
 */
export async function watchRestores(): Promise<() => void> {
  if (!inTauri) {
    const away = new URLSearchParams(location.search).get("restored");
    const timer =
      away === "minimized" || away === "tray" ? setTimeout(() => void noticeRestored(away, 60000), 1500) : 0;
    return () => clearTimeout(timer);
  }
  const { listen } = await import("@tauri-apps/api/event");
  return listen<{ from: Away; awayMs: number }>("argus-restored", (e) => {
    void noticeRestored(e.payload.from, e.payload.awayMs);
  });
}
