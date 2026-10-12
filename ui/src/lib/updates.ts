// Update status pushed by the native updater, and what happens on the first
// start after an update.

import { createSignal } from "solid-js";
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
  toast("success", `Argus 已更新到 ${version}。`);
  const service = await api.privilegedServiceStatus().catch(() => null);
  // A running service that does not report this version is older (or too old to answer).
  if (service?.state !== "running" || !service.authorized || service.version === version) return;
  const taskManager = await api.taskManagerStatus().catch(() => null);
  const ok = await confirm({
    title: "更新特权服务？",
    body: `特权服务仍是${service.version ? ` ${service.version}` : "旧版本"}，与 Argus ${version} 不一致，部分功能可能不可用。更新需要管理员授权。${
      taskManager?.state === "enabled" ? "更新会先恢复 Windows 任务管理器，之后可在设置中重新启用替换。" : ""
    }`,
    confirmLabel: "更新服务",
  });
  if (!ok) return;
  try {
    await api.managePrivilegedService("update");
    toast("success", "特权服务已更新。");
  } catch (e) {
    toast("error", `无法更新特权服务：${String(e)}`);
  }
}
