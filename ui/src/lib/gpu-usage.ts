// Independent engines run concurrently. Sum contributions to each physical
// engine first, then take the busiest; never add already-reduced percentages.
import type { GpuEngineUsage } from "./types.ts";

interface GpuUsage {
  gpu: number;
  gpuEngine?: string | null;
  gpuEngines?: readonly GpuEngineUsage[];
}

export function aggregateGpu(rows: readonly GpuUsage[]) {
  const byEngine = new Map<string, GpuEngineUsage>();
  let unknown = 0;
  for (const row of rows) {
    const engines = row.gpuEngines?.length
      ? row.gpuEngines
      : row.gpuEngine
        ? [{ id: `label:${row.gpuEngine}`, name: row.gpuEngine, util: row.gpu }]
        : [];
    if (!engines.length && Number.isFinite(row.gpu)) unknown = Math.max(unknown, row.gpu);
    for (const engine of engines) {
      if (!Number.isFinite(engine.util) || engine.util <= 0) continue;
      const previous = byEngine.get(engine.id);
      byEngine.set(engine.id, {
        ...engine,
        util: Math.min(100, (previous?.util ?? 0) + engine.util),
      });
    }
  }
  const gpuEngines = [...byEngine.values()];
  let gpu = Math.min(100, Math.max(0, unknown));
  let gpuEngine: string | null = null;
  for (const engine of gpuEngines) {
    if (engine.util > gpu) {
      gpu = engine.util;
      gpuEngine = engine.name;
    }
  }
  return { gpu, gpuEngine, gpuEngines };
}
