// Update status pushed by the native updater; what the window offers about it
// (the title bar button, the dialog) and what happens on the first start
// after an update.

import { createSignal } from "solid-js";
import { t } from "./i18n";
import { api, inTauri } from "./ipc";
import { persisted } from "./prefs";
import type { InstallError, UpdateStatus } from "./types";
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

/**
 * After an update: says so with a way to see what changed. The installer
 * updates the privileged service too, except while the task manager
 * replacement is on or when that failed; then offer it here, while the
 * mismatch is fresh.
 */
export async function afterUpdate(version: string) {
  const from = updatedFrom();
  setUpdatedFrom(null);
  toast("success", t("updates.updated", { version }), 10000, {
    actions: [{ label: t("updates.whatsNew"), run: () => openWhatsNew(from) }],
  });
  const service = await api.privilegedServiceStatus().catch(() => null);
  // A running service that does not report this version is older (or too old to answer).
  if (service?.state !== "running" || !service.authorized || service.version === version) return;
  const taskManager = await api.taskManagerStatus().catch(() => null);
  const body = service.version
    ? t("updates.service.body", { serviceVersion: service.version, version })
    : t("updates.service.bodyOlder", { version });
  const ok = await confirm({
    title: t("updates.service.title"),
    body:
      taskManager?.state === "enabled"
        ? t("updates.service.withNote", { body, note: t("updates.service.restoresTaskManager") })
        : body,
    confirmLabel: t("updates.service.confirm"),
  });
  if (!ok) return;
  try {
    await api.managePrivilegedService("update");
    toast("success", t("updates.service.done"));
  } catch (e) {
    toast("error", t("updates.service.failed", { error: String(e) }));
  }
}
