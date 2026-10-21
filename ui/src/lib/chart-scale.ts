// Tops of the charts without a fixed one (transfer rates, power): a whole
// number in the unit the label shows, and above ten a multiple of ten, so the
// label reads "8 MB/s" or "150 Mbps" rather than "7.34 MB/s". No Solid;
// ui/tests runs it directly.

export type RateUnit = "bytes" | "bits";

/** A rate unit: its size in bytes per second and its label, as fmtRate and fmtBits use them. */
interface Unit {
  size: number;
  label: string;
}

const UNITS: Record<RateUnit, Unit[]> = {
  bytes: [
    { size: 1024, label: "KB/s" },
    { size: 1024 ** 2, label: "MB/s" },
    { size: 1024 ** 3, label: "GB/s" },
  ],
  bits: [
    { size: 1e3 / 8, label: "Kbps" },
    { size: 1e6 / 8, label: "Mbps" },
    { size: 1e9 / 8, label: "Gbps" },
  ],
};

/** Floating-point slack, so 30.000000000004 rounds to 30, not 40. */
const EPSILON = 1e-9;

/** The least whole number at or above `x`, at least 1; above ten, the least multiple of ten. */
export function roundCeiling(x: number): number {
  if (!(x > 1)) return 1;
  const whole = Math.ceil(x - EPSILON);
  return whole <= 10 ? whole : Math.ceil(x / 10 - EPSILON) * 10;
}

/** The unit a rate is shown in: the largest it reaches, as fmtRate and fmtBits pick. */
function unitOf(bytesPerSec: number, units: Unit[]): number {
  let i = 0;
  while (i + 1 < units.length && bytesPerSec >= units[i + 1].size * (1 - EPSILON)) i++;
  return i;
}

/** The top for a rate chart whose highest point is `peak` bytes per second. */
export function rateCeiling(peak: number, unit: RateUnit): number {
  const units = UNITS[unit];
  let i = unitOf(Math.max(0, peak), units);
  for (;;) {
    const top = roundCeiling(peak / units[i].size) * units[i].size;
    // Rounded up into the next unit: round there, "1 MB/s" rather than "1030 KB/s".
    if (i + 1 < units.length && top >= units[i + 1].size * (1 - EPSILON)) {
      i++;
      continue;
    }
    return top;
  }
}

/** Its label: "8 MB/s", "150 Mbps". */
export function rateCeilingLabel(top: number, unit: RateUnit): string {
  const units = UNITS[unit];
  const u = units[unitOf(top, units)];
  return `${Math.round(top / u.size)} ${u.label}`;
}
