// One preparation action shared by Settings and onboarding. The operation
// and its result survive paging or closing the guide while UAC is pending.
import { createSignal } from "solid-js";
import { api } from "./ipc";
import type { SettingsView } from "./types";

const [kernelBusy, setKernelBusy] = createSignal(false);
const [kernelError, setKernelError] = createSignal<string | null>(null);
const [kernelRevision, setKernelRevision] = createSignal(0);

export { kernelBusy, kernelError, kernelRevision };

export async function changeKernelSensors(enabled: boolean): Promise<SettingsView | null> {
  if (kernelBusy()) return null;
  setKernelBusy(true);
  setKernelError(null);
  try {
    return await api.setKernelSensors(enabled);
  } catch (error) {
    setKernelError(String(error));
    return null;
  } finally {
    setKernelBusy(false);
    setKernelRevision((value) => value + 1);
  }
}
