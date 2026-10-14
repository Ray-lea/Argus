import type { CpuInfo, CpuMetric, CpuMetricKind, CpuSensors } from "./types";

export function mockCpuSensors(info: CpuInfo, load: number, amd: boolean): CpuSensors {
  const physical = [...new Map(info.threads.map((thread, processor) => [thread.core, processor])).entries()];
  const cores = physical.map(([core, processor]) => ({
    core,
    group: 0,
    processor,
    celsius: 54 + (core % 8),
    distanceToLimit: amd ? null : 100 - (54 + (core % 8)),
    clockMhz: amd ? 5350 + (core % 4) * 50 : processor < 12 ? 4200 : 3200,
    effectiveMhz: Math.round(load * 25 + core * 11),
    watts: amd ? 2.1 + (core % 5) * 0.4 : null,
    vid: amd ? null : 1.12 + (core % 7) * 0.01,
    multiplier: amd ? 53.5 + (core % 4) * 0.5 : processor < 12 ? 42 : 32,
    c0Percent: Math.min(100, load * 0.6 + (core % 5)),
    thermalThrottling: amd ? null : false,
    voltage: amd ? 1.15 : null,
    smuC0Percent: amd ? 20 : null,
    c1Percent: amd ? 10 : null,
    c6Percent: amd ? 70 : null,
    osC1Percent: 10,
    osC2Percent: 5,
    osC3Percent: 30,
    efficient: !amd && processor >= 12,
    limits: amd ? [] : [{ reason: "power" as const, active: core === 0, occurred: core < 3 }],
  }));
  const metrics: CpuMetric[] = [];
  const add = (kind: CpuMetricKind, value: number, source: CpuMetric["source"] = "msr") =>
    metrics.push({ kind, value, source });
  add("averageMhz", amd ? 5425 : 3800);
  add("effectiveMhz", Math.round(load * 25));
  add("busMhz", 100.02);
  add("thermalLimitCelsius", amd ? 95 : 100, amd ? "smu" : "msr");
  if (amd) {
    for (const [kind, value] of Object.entries({
      coreVoltage: 1.21,
      coreCurrent: 72,
      coreRailWatts: 87.12,
      coreRailVid: 1.25,
      coreVrmCelsius: 45,
      socVoltage: 1.18,
      socCurrent: 12,
      socRailWatts: 14.16,
      socVid: 1.2,
      socVrmCelsius: 42,
      miscVoltage: 1.08,
      miscCurrent: 2,
      miscRailWatts: 2.16,
      miscVid: 1.1,
      miscVrmCelsius: 39,
    }) as [CpuMetricKind, number][])
      add(kind, value, "svi3");
    for (const [kind, value] of Object.entries({
      pptWatts: 101,
      pptLimitWatts: 230,
      pptFastWatts: 105,
      pptFastLimitWatts: 230,
      pptSlowWatts: 99,
      tdcAmps: 81,
      tdcLimitAmps: 160,
      edcAmps: 110,
      edcLimitAmps: 225,
      fabricMhz: 2000,
      uncoreMhz: 3000,
      memoryMhz: 3000,
      vddpVolts: 1.05,
      vddgIodVolts: 0.95,
      vddgCcdVolts: 0.95,
      socWatts: 14,
      l3Ccd1Celsius: 52,
      l3Ccd2Celsius: 47,
    }) as [CpuMetricKind, number][])
      add(kind, value, "smu");
  } else {
    add("ringMhz", 3200);
    add("ringMinMhz", 800);
    add("ringMaxMhz", 4500);
    add("ringMultiplier", 32);
    add("pl1Watts", 45);
    add("pl2Watts", 115);
    add("pl1Seconds", 28);
    add("pl2Seconds", 0.002);
    add("tdpWatts", 45);
    add("memoryWatts", 2.5);
    add("platformWatts", 68);
    add("coresWatts", 32);
    add("graphicsWatts", 1.2);
  }
  return {
    state: "ready",
    limits: amd ? [] : [{ reason: "power", active: false, occurred: true }],
    packageCelsius: 61,
    controlCelsius: amd ? 61 : null,
    coreTemperatures: info.threads.map((thread, processor) => ({
      group: 0,
      processor,
      celsius: cores.find((core) => core.core === thread.core)?.celsius ?? 55,
    })),
    ccdTemperatures: amd
      ? [
          { index: 0, celsius: 54 },
          { index: 1, celsius: 47 },
        ]
      : [],
    coreVoltage: null,
    coreVid: 1.18,
    packageWatts: amd ? 101 : 55,
    metrics: amd
      ? metrics
          .filter(
            (m) =>
              m.source !== "svi3" &&
              ![
                "edcAmps",
                "l3Ccd1Celsius",
                "l3Ccd2Celsius",
                "pptFastWatts",
                "pptFastLimitWatts",
                "pptSlowWatts",
              ].includes(m.kind),
          )
          .concat([
            { kind: "coreSmuVoltage", value: 1.15, source: "smu" },
            { kind: "socVoltage", value: 1.2, source: "smu" },
            { kind: "pptPercent", value: 44, source: "smu" },
          ])
      : metrics,
    cores,
    smuState: amd ? "ready" : null,
    smuTableVersion: amd ? 0x620205 : null,
    thermalThrottling: amd ? null : false,
    boardSensor: amd ? "NCT6799D" : null,
    boardFans: amd
      ? [
          { channel: 0, rpm: 920 },
          { channel: 1, rpm: 1350 },
          { channel: 4, rpm: 2900 },
        ]
      : [],
  };
}
