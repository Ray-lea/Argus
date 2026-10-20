import { createSignal } from "solid-js";
import { openNewTask } from "../components/NewTaskDialog";
import { type MessageKey, t } from "./i18n";
import { api } from "./ipc";
import type { ExplorerError, ProcessIdentity } from "./types";
import { confirm, dismissToast, toast } from "./ui";

export const [restartingExplorer, setRestartingExplorer] = createSignal(false);
const ERRORS: Record<ExplorerError["kind"], MessageKey> = {
  stale: "tasks.explorer.error.stale",
  unsupported: "tasks.explorer.error.unsupported",
  permission: "tasks.explorer.error.permission",
  prepare: "tasks.explorer.error.prepare",
  shutdown: "tasks.explorer.error.shutdown",
  restore: "tasks.explorer.error.restore",
  busy: "tasks.explorer.error.busy",
};

export async function restartExplorer(shell: ProcessIdentity) {
  if (restartingExplorer()) return;
  if (
    !(await confirm({
      title: t("tasks.explorer.title"),
      body: t("tasks.explorer.confirm"),
      confirmLabel: t("tasks.explorer.restart"),
    }))
  )
    return;
  setRestartingExplorer(true);
  const pending = toast("info", t("tasks.explorer.restarting"), 60000);
  try {
    await api.restartExplorer(shell.pid, shell.createTime);
    toast("success", t("tasks.explorer.done"));
  } catch (value) {
    const error = value as Partial<ExplorerError> | null;
    const key = error?.kind && Object.hasOwn(ERRORS, error.kind) ? ERRORS[error.kind] : ERRORS.prepare;
    const code =
      typeof error?.code === "number"
        ? ` ${t("tasks.code", { code: `0x${error.code.toString(16).toUpperCase().padStart(8, "0")}` })}`
        : "";
    toast(
      "error",
      t(key) + code,
      15000,
      error?.shellMissing
        ? {
            actions: [
              { label: t("tasks.explorer.recover"), run: () => openNewTask("%WINDIR%\\explorer.exe") },
            ],
          }
        : {},
    );
  } finally {
    dismissToast(pending);
    setRestartingExplorer(false);
  }
}
