import type { CpuMetricKind } from "./types";

export const CPU_METRICS = {
  ringMhz: { unit: "MHz", digits: 0, group: "clock" },
  ringMinMhz: { unit: "MHz", digits: 0, group: "clock" },
  ringMaxMhz: { unit: "MHz", digits: 0, group: "clock" },
  ringMultiplier: { unit: "×", digits: 2, group: "clock" },
  pptPercent: { unit: "%", digits: 1, group: "power" },
  tdcPercent: { unit: "%", digits: 1, group: "power" },
  edcPercent: { unit: "%", digits: 1, group: "power" },
  coreSmuVoltage: { unit: "V", digits: 3, group: "voltage" },
  vddioWatts: { unit: "W", digits: 1, group: "power" },
  vdd18Watts: { unit: "W", digits: 1, group: "power" },
  memoryIoVolts: { unit: "V", digits: 3, group: "voltage" },
  graphicsMhz: { unit: "MHz", digits: 0, group: "clock" },
  vidLimitVolts: { unit: "V", digits: 3, group: "voltage" },
  coreVoltage: {
    unit: "V",
    digits: 3,
    group: "voltage",
  },
  coreCurrent: {
    unit: "A",
    digits: 1,
    group: "voltage",
  },
  coreRailWatts: {
    unit: "W",
    digits: 1,
    group: "power",
  },
  coreRailVid: {
    unit: "V",
    digits: 3,
    group: "voltage",
  },
  coreVrmCelsius: {
    unit: "°C",
    digits: 0,
    group: "temperature",
  },
  secondCoreVoltage: {
    unit: "V",
    digits: 3,
    group: "voltage",
  },
  secondCoreCurrent: {
    unit: "A",
    digits: 1,
    group: "voltage",
  },
  secondCoreRailWatts: {
    unit: "W",
    digits: 1,
    group: "power",
  },
  secondCoreRailVid: {
    unit: "V",
    digits: 3,
    group: "voltage",
  },
  secondCoreVrmCelsius: {
    unit: "°C",
    digits: 0,
    group: "temperature",
  },
  socVoltage: {
    unit: "V",
    digits: 3,
    group: "voltage",
  },
  socCurrent: {
    unit: "A",
    digits: 1,
    group: "voltage",
  },
  socRailWatts: {
    unit: "W",
    digits: 1,
    group: "power",
  },
  socVid: {
    unit: "V",
    digits: 3,
    group: "voltage",
  },
  socVrmCelsius: {
    unit: "°C",
    digits: 0,
    group: "temperature",
  },
  miscVoltage: {
    unit: "V",
    digits: 3,
    group: "voltage",
  },
  miscCurrent: {
    unit: "A",
    digits: 1,
    group: "voltage",
  },
  miscRailWatts: {
    unit: "W",
    digits: 1,
    group: "power",
  },
  miscVid: {
    unit: "V",
    digits: 3,
    group: "voltage",
  },
  miscVrmCelsius: {
    unit: "°C",
    digits: 0,
    group: "temperature",
  },
  coresWatts: {
    unit: "W",
    digits: 1,
    group: "power",
  },
  socWatts: {
    unit: "W",
    digits: 1,
    group: "power",
  },
  miscWatts: {
    unit: "W",
    digits: 1,
    group: "power",
  },
  graphicsWatts: {
    unit: "W",
    digits: 1,
    group: "power",
  },
  memoryWatts: {
    unit: "W",
    digits: 1,
    group: "power",
  },
  platformWatts: {
    unit: "W",
    digits: 1,
    group: "power",
  },
  pptWatts: {
    unit: "W",
    digits: 1,
    group: "power",
  },
  pptLimitWatts: {
    unit: "W",
    digits: 1,
    group: "power",
  },
  pptFastWatts: {
    unit: "W",
    digits: 1,
    group: "power",
  },
  pptFastLimitWatts: {
    unit: "W",
    digits: 1,
    group: "power",
  },
  pptSlowWatts: {
    unit: "W",
    digits: 1,
    group: "power",
  },
  tdcAmps: {
    unit: "A",
    digits: 1,
    group: "power",
  },
  tdcLimitAmps: {
    unit: "A",
    digits: 1,
    group: "power",
  },
  edcAmps: {
    unit: "A",
    digits: 1,
    group: "power",
  },
  edcLimitAmps: {
    unit: "A",
    digits: 1,
    group: "power",
  },
  pl1Watts: {
    unit: "W",
    digits: 1,
    group: "power",
  },
  pl2Watts: {
    unit: "W",
    digits: 1,
    group: "power",
  },
  pl1Seconds: {
    unit: "s",
    digits: 3,
    group: "power",
  },
  pl2Seconds: {
    unit: "s",
    digits: 3,
    group: "power",
  },
  tdpWatts: {
    unit: "W",
    digits: 1,
    group: "power",
  },
  thermalLimitCelsius: {
    unit: "°C",
    digits: 0,
    group: "temperature",
  },
  averageMhz: {
    unit: "MHz",
    digits: 0,
    group: "clock",
  },
  effectiveMhz: {
    unit: "MHz",
    digits: 0,
    group: "clock",
  },
  busMhz: {
    unit: "MHz",
    digits: 2,
    group: "clock",
  },
  fabricMhz: {
    unit: "MHz",
    digits: 0,
    group: "clock",
  },
  uncoreMhz: {
    unit: "MHz",
    digits: 0,
    group: "clock",
  },
  memoryMhz: {
    unit: "MHz",
    digits: 0,
    group: "clock",
  },
  socCelsius: {
    unit: "°C",
    digits: 0,
    group: "temperature",
  },
  ioDieCelsius: {
    unit: "°C",
    digits: 0,
    group: "temperature",
  },
  l3Ccd1Celsius: {
    unit: "°C",
    digits: 0,
    group: "temperature",
  },
  l3Ccd2Celsius: {
    unit: "°C",
    digits: 0,
    group: "temperature",
  },
  vddpVolts: {
    unit: "V",
    digits: 3,
    group: "voltage",
  },
  vddgIodVolts: {
    unit: "V",
    digits: 3,
    group: "voltage",
  },
  vddgCcdVolts: {
    unit: "V",
    digits: 3,
    group: "voltage",
  },
  northbridgeVidVolts: {
    unit: "V",
    digits: 3,
    group: "voltage",
  },
} as const satisfies Record<CpuMetricKind, { unit: string; digits: number; group: string }>;
