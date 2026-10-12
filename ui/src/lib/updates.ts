// Update status pushed by the native updater, and what happens on the first
// start after an update.

import { createSignal } from "solid-js";
import { t } from "./i18n";
import { api, inTauri } from "./ipc";
import type { UpdateStatus } from "./types";
import { confirm, toast } from "./ui";

export const [updateStatus, setUpdateStatus] = createSignal<UpdateStatus | null>(null);

/** An update waits for the user: ready to install, or (standalone exe) to download. */
export const updatePending = () => {
  const state = updateStatus()?.state;
  return state === "ready" || state === "available";
};

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
 * The installer replaced Argus but not the privileged service, which needs
 * administrator rights; offer to update it while the mismatch is fresh.
 */
export async function afterUpdate(version: string) {
  toast("success", t("updates.updated", { version }));
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
