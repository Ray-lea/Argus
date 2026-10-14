import type { Hardware } from "./types";

/** A die reading takes priority; ACPI remains an explicitly labelled fallback. */
export function cpuTemperature(hw: Hardware): number | null {
  if (hw.cpuSensors?.packageCelsius != null) return hw.cpuSensors.packageCelsius;
  if (hw.cpuSensors?.coreTemperatures.length)
    return Math.max(...hw.cpuSensors.coreTemperatures.map((core) => core.celsius));
  return hw.thermal.length ? Math.max(...hw.thermal.map((zone) => zone.celsius)) : null;
}

export function hasCpuDieTemperature(hw: Hardware): boolean {
  return hw.cpuSensors?.packageCelsius != null || Boolean(hw.cpuSensors?.coreTemperatures.length);
}
