// Update status pushed by the native updater; what the window offers about it
// (the title bar button, the dialog) and what the first start of a new
// version shows.

import { createEffect, createRoot, createSignal, onCleanup } from "solid-js";
import { t } from "./i18n";
import { api, inTauri } from "./ipc";
import { persisted } from "./prefs";
import { firstStart } from "./release-notes";
import type { AppInfo, InstallError, UpdateStatus } from "./types";
import { confirm, toast } from "./ui";

export const [updateStatus, setUpdateStatus] = createSignal<UpdateStatus | null>(null);

/** The version waiting for the user: ready to install, or (standalone exe) to download. */
export const updateOffer = () => {
  const s = updateStatus();
  return s?.version && (s.state === "ready" || s.state === "available") ? s.version : null;
};

/** An update waits for the user (the dots in the navigation and settings). */
export const updatePending = () => updateOffer() !== null;

/** The version the title bar button no longer offers ("don't remind me"). */
const [dismissed, setDismissed] = persisted<string | null>("update.dismissed", null);
/** The version the one-time notice announced. */
export const [announced, setAnnounced] = persisted<string | null>("update.announced", null);
/** The version an update started from, to show what changed once it is done. */
const [updatedFrom, setUpdatedFrom] = persisted<string | null>("update.from", null);

/** The version the title bar button offers: the one on offer, unless dismissed. */
export const updateButtonVersion = () => {
  const version = updateOffer();
  return version && version !== dismissed() ? version : null;
};

export const dismissUpdate = (version: string) => setDismissed(version);

/** The update on offer, or what an installed version changed (since `from`). */
export type UpdateDialogState = { mode: "offer" } | { mode: "installed"; from: string | null };
export const [updateDialog, setUpdateDialog] = createSignal<UpdateDialogState | null>(null);
export const openUpdateDialog = () => setUpdateDialog({ mode: "offer" });
export const openWhatsNew = (from: string | null = null) => setUpdateDialog({ mode: "installed", from });

let offerQueued = false;
/** The update dialog is asked for and waits (`openUpdateDialogAfter`). */
export const updateDialogQueued = () => offerQueued;
/**
 * The update dialog asked for by the tray or the ready notice: once `busy`
 * (the onboarding) is over, never on top of it, and left as it is when
 * already open.
 */
export async function openUpdateDialogAfter(busy: () => boolean) {
  if (offerQueued) return;
  offerQueued = true;
  await free(() => !busy());
  offerQueued = false;
  if (updateDialog()?.mode !== "offer") openUpdateDialog();
}

/**
 * Starts the downloaded installer; Argus exits once it runs. Resolves with
 * why it did not start, `cancelled` when the administrator prompt was declined.
 */
export async function installUpdate(current: string | undefined): Promise<InstallError | null> {
  if (current) setUpdatedFrom(current);
  try {
    await api.installUpdate();
    return null;
  } catch (e) {
    setUpdatedFrom(null);
    if (e && typeof e === "object" && "cancelled" in e) return e as InstallError;
    return { cancelled: false, message: String(e) };
  }
}

/** Follows the native status; returns a cleanup. */
export async function watchUpdates(): Promise<() => void> {
  if (!inTauri) {
    const mock = await import("./mock");
    mock.updates.subscribe(setUpdateStatus);
    setUpdateStatus(await mock.updates.status());
    return () => {};
  }
  const { listen } = await import("@tauri-apps/api/event");
  const unlisten = await listen<UpdateStatus>("update-status", (e) => setUpdateStatus(e.payload));
  setUpdateStatus(await api.updateStatus());
  return unlisten;
}

// Whether `afterStart` has something to show and is not done with it yet;
// `decided` once it has read the settings.
let welcoming = false;
let decided: () => void = () => {};
const decision = new Promise<void>((resolve) => {
  decided = resolve;
});

/**
 * Whether the first start of this version is showing, or still to show, what
 * changed (or that it was updated): until it is done, it has the window. Asked
 * as the window comes back, it settles before anything is shown for that
 * showing.
 */
export async function welcomePending(): Promise<boolean> {
  await decision;
  return welcoming;
}

/** The first start of this version (`firstStart`); `info` is null when the host did not say. */
export async function afterStart(info: AppInfo | null, busy: () => boolean) {
  try {
    if (info) await welcome(info, busy);
  } finally {
    welcoming = false;
    decided();
  }
}

/**
 * The first start of a version. After an update it shows what changed (or,
 * turned off in Settings, says it was updated), once: the version is recorded
 * natively as it shows, so closing the dialog is the end of it until the next
 * update. It waits until the window is in view and `busy` (the onboarding,
 * say) is over; a start by the task manager replacement leaves it to a later
 * start, since the system may be struggling then.
 */
async function welcome(info: AppInfo, busy: () => boolean) {
  const settings = await api.settings().catch(() => null);
  if (!settings) return;
  const start = firstStart(info.version, settings.seenVersion, settings.onboarding > 0, info.updated);
  if (start.kind === "seen") return;
  if (start.kind === "record" || info.taskManager) {
    if (start.kind === "record") void api.noteVersionSeen().catch(() => {});
    return;
  }
  welcoming = true;
  decided();
  if (inTauri) await shown();
  await free(() => !busy() && !updateDialog());
  const from = start.from ?? updatedFrom();
  setUpdatedFrom(null);
  void api.noteVersionSeen().catch(() => {});
  if (settings.whatsNew) {
    openWhatsNew(from);
    await free(() => !updateDialog());
  } else {
    toast("success", t("updates.updated", { version: info.version }), 10000, {
      actions: [{ label: t("updates.whatsNew"), run: () => openWhatsNew(from) }],
    });
  }
  await offerServiceUpdate(info.version);
}

/**
 * Resolves once the window has been on screen: at once after a normal start;
 * a start hidden in the tray (autostart, an update begun there) waits for its
 * first showing. The page cannot tell by itself: a window started hidden
 * keeps it awake, and reported visible, until it has drawn itself once.
 */
export async function shown(): Promise<void> {
  const { once } = await import("@tauri-apps/api/event");
  const { getCurrentWindow } = await import("@tauri-apps/api/window");
  await new Promise<void>((resolve) => {
    // Listening first: a showing between the two calls is not missed.
    void once("argus-restored", () => resolve()).then(() =>
      getCurrentWindow()
        .isVisible()
        .then(
          (visible) => visible && resolve(),
          () => {},
        ),
    );
  });
}

/** Resolves once `ready` holds while the page is in view (not minimized or hidden again). */
function free(ready: () => boolean): Promise<void> {
  return new Promise((resolve) =>
    createRoot((dispose) => {
      const [visible, setVisible] = createSignal(document.visibilityState === "visible");
      const update = () => setVisible(document.visibilityState === "visible");
      document.addEventListener("visibilitychange", update);
      onCleanup(() => document.removeEventListener("visibilitychange", update));
      createEffect(() => {
        if (!visible() || !ready()) return;
        queueMicrotask(dispose);
        resolve();
      });
    }),
  );
}

/**
 * The installer updates the privileged service too, except while the task
 * manager replacement is on or when that failed; then offer it here, while
 * the mismatch is fresh.
 */
async function offerServiceUpdate(version: string) {
  const service = await api.privilegedServiceStatus().catch(() => null);
  // A running service that does not report this version is older (or too old to answer).
  if (service?.state !== "running" || !service.authorized || service.version === version) return;
  const restores = (await api.taskManagerStatus().catch(() => null))?.state === "enabled";
  // No version: too old to answer in this version's protocol, so it refuses
  // what needs it; the body says what that is.
  const body = service.version
    ? t("updates.service.body", { serviceVersion: service.version, version })
    : t("updates.service.bodyOlder", { version });
  const ok = await confirm({
    title: t("updates.service.title"),
    body: restores
      ? t("updates.service.withNote", { body, note: t("updates.service.restoresTaskManager") })
      : body,
    confirmLabel: t("updates.service.confirm"),
  });
  if (!ok) return;
  try {
    await api.managePrivilegedService("update");
    toast("success", restores ? t("updates.service.doneRestored") : t("updates.service.done"));
  } catch (e) {
    toast("error", t("updates.service.failed", { error: String(e) }));
  }
}
