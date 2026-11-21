// Reading the results of scripts/perf-native.mjs and comparing a base build
// with a new one over alternating pairs of runs.
import assert from "node:assert/strict";

/** The figures compared, in the order shown. `frames` marks those that count per presented frame. */
export const FIGURES = [
  { key: "drawP50Ms", name: "draw P50 ms", digits: 3 },
  { key: "drawP90Ms", name: "draw P90 ms", digits: 3 },
  { key: "mcyclesPerFrame", name: "Mcycles/frame", digits: 2, frames: true },
  { key: "mcyclesPerSecond", name: "Mcycles/s", digits: 0 },
  { key: "cpuMsPerSecond", name: "CPU ms/s", digits: 1 },
  { key: "uiThreadCpuMsPerSecond", name: "UI thread ms/s", digits: 1 },
  { key: "gpuBusiestEnginePercent", name: "GPU busiest engine %", digits: 2 },
  { key: "privateMiB", name: "private MiB", digits: 1 },
  { key: "presentedFps", name: "presented fps", digits: 1 },
  { key: "frameP95Ms", name: "frame P95 ms", digits: 2 },
];

/** Frame rates further apart than this make a pair's per-frame figures incomparable. */
export const FPS_TOLERANCE = 0.1;

/** `NAME=VALUE` arguments as an object. */
export function assignments(list) {
  return Object.fromEntries(
    list.map((text) => {
      const at = text.indexOf("=");
      assert(at > 0, `${text}: expected NAME=VALUE`);
      return [text.slice(0, at), text.slice(at + 1)];
    }),
  );
}

/**
 * Which side a pair measures first. Base and new take turns, so that what
 * drifts during the session (temperature, other load) does not favour one.
 */
export function sides(pair) {
  return pair % 2 === 0 ? ["base", "new"] : ["new", "base"];
}

const finite = (value) => (Number.isFinite(value) ? value : null);
const cycles = (mark) => mark.processes.reduce((total, process) => total + (process.cpuCycles ?? 0), 0);

const quantile = (sorted, q) => (sorted.length ? sorted[Math.floor((sorted.length - 1) * q)] : null);

/**
 * The phases of one perf-native result with the figures compared. Process
 * cycles come from the probe's marks: unlike thread times, which advance in
 * scheduler ticks, they resolve the small CPU use of an idle interface.
 * Draw times are there when the run had ARGUS_GPUI_FRAME_TRACE set: the
 * duration of each draw on the interface thread (`frame-phases`). Their
 * median hardly moves with other load on the machine, which the per-second
 * figures follow closely.
 */
export function phaseFigures(result) {
  const marks = new Map(result.events.filter((event) => event.event === "mark").map((event) => [event.tag, event]));
  const draws = result.events.filter((event) => event.event === "frame-phases" && Number.isFinite(event.atQpc) && Number.isFinite(event.totalUs));
  return result.summaries.map((summary) => {
    const before = marks.get(`${summary.tag}-begin`);
    const after = marks.get(`${summary.tag}-end`);
    const spent = before && after ? cycles(after) - cycles(before) : null;
    const frames = summary.observedFrames;
    const drawn = before && after ? draws.filter((event) => event.atQpc >= before.qpc100ns && event.atQpc <= after.qpc100ns).map((event) => event.totalUs / 1000).sort((a, b) => a - b) : [];
    return {
      phase: summary.tag,
      busy: finite(summary.systemBusyPercent),
      figures: {
        drawP50Ms: quantile(drawn, 0.5),
        drawP90Ms: quantile(drawn, 0.9),
        mcyclesPerFrame: spent !== null && frames > 0 ? spent / frames / 1e6 : null,
        mcyclesPerSecond: spent !== null && summary.durationMs > 0 ? spent / (summary.durationMs / 1000) / 1e6 : null,
        cpuMsPerSecond: finite(summary.cpuMsPerSecond),
        uiThreadCpuMsPerSecond: finite(summary.uiThreadCpuMsPerSecond),
        gpuBusiestEnginePercent: finite(summary.gpuBusiestEnginePercent),
        privateMiB: finite(summary.privateMiB),
        presentedFps: finite(summary.presentedFps),
        frameP95Ms: finite(summary.frameP95Ms),
      },
    };
  });
}

function median(values) {
  if (!values.length) return null;
  const sorted = [...values].sort((a, b) => a - b);
  const middle = sorted.length >> 1;
  return sorted.length % 2 ? sorted[middle] : (sorted[middle - 1] + sorted[middle]) / 2;
}

/**
 * Compares pairs of runs, each `{ base, new }` holding `phaseFigures` of one
 * run. Per phase and figure: the medians of both sides, and the relative
 * difference new/base − 1 of each pair as its median, range and direction
 * count. A single pair is an anecdote: what counts is whether the pairs agree.
 * Per-frame figures leave out pairs whose frame rates differ by more than
 * FPS_TOLERANCE, where the two sides did not draw the same thing.
 */
export function compare(pairs) {
  assert(pairs.length > 0, "at least one pair of runs");
  return pairs[0].base.map(({ phase }) => {
    const of = (side) => pairs.map((pair) => pair[side].find((entry) => entry.phase === phase)).filter(Boolean);
    const [base, fresh] = [of("base"), of("new")];
    assert(base.length === pairs.length && fresh.length === pairs.length, `${phase}: missing from a run`);
    const matched = base.map((entry, index) => {
      const [a, b] = [entry.figures.presentedFps, fresh[index].figures.presentedFps];
      return a > 0 && b > 0 ? Math.abs(b / a - 1) <= FPS_TOLERANCE : a === null && b === null;
    });
    const busy = [...base, ...fresh].map((entry) => entry.busy).filter((value) => value !== null);
    const rows = FIGURES.map(({ key, frames }) => {
      const used = base.map((_, index) => index).filter((index) => {
        const [a, b] = [base[index].figures[key], fresh[index].figures[key]];
        return a !== null && b !== null && (!frames || matched[index]);
      });
      const changes = used.filter((index) => base[index].figures[key] > 0).map((index) => fresh[index].figures[key] / base[index].figures[key] - 1);
      return {
        key,
        pairs: used.length,
        base: median(used.map((index) => base[index].figures[key])),
        new: median(used.map((index) => fresh[index].figures[key])),
        change: median(changes),
        least: changes.length ? Math.min(...changes) : null,
        most: changes.length ? Math.max(...changes) : null,
        lower: changes.filter((change) => change < 0).length,
        higher: changes.filter((change) => change > 0).length,
      };
    });
    return { phase, pairs: pairs.length, unmatched: matched.filter((value) => !value).length, busy: busy.length ? [Math.min(...busy), Math.max(...busy)] : null, rows };
  });
}

const percent = (value) => (value === null ? "-" : `${value >= 0 ? "+" : ""}${(value * 100).toFixed(1)}%`);

/** The comparison as text lines. */
export function format(report) {
  const lines = [];
  for (const { phase, pairs, unmatched, busy, rows } of report) {
    const notes = [`${pairs} pair${pairs === 1 ? "" : "s"}`];
    if (unmatched) notes.push(`${unmatched} with frame rates more than ${FPS_TOLERANCE * 100}% apart (left out of per-frame figures)`);
    if (busy) notes.push(`system ${busy[0].toFixed(0)}-${busy[1].toFixed(0)}% busy`);
    lines.push("", `${phase}  (${notes.join("; ")})`, `  ${"figure".padEnd(22)}${"base".padStart(10)}${"new".padStart(10)}${"change".padStart(9)}  ${"range".padEnd(17)}pairs`);
    for (const row of rows) {
      const { name, digits } = FIGURES.find((figure) => figure.key === row.key);
      if (!row.pairs) {
        lines.push(`  ${name.padEnd(22)}${"-".padStart(10)}${"-".padStart(10)}`);
        continue;
      }
      const all = row.pairs > 1 ? "all " : "";
      const agreement = row.lower === row.pairs ? `${all}lower` : row.higher === row.pairs ? `${all}higher` : row.lower + row.higher === 0 ? "equal" : `${row.lower} lower, ${row.higher} higher`;
      lines.push(
        `  ${name.padEnd(22)}${row.base.toFixed(digits).padStart(10)}${row.new.toFixed(digits).padStart(10)}${percent(row.change).padStart(9)}  ${`${percent(row.least)} … ${percent(row.most)}`.padEnd(17)}${agreement}`,
      );
    }
  }
  return lines;
}
