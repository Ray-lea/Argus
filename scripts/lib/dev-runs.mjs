// The development script (scripts/desktop.mjs) starts each build from a copy
// in target/native-dev/runs/run-<ms>, so that cargo can replace the build
// while it runs. Nothing else reads the older copies; at about 65 MB each
// they had grown to gigabytes.
import { readdirSync, rmSync } from "node:fs";
import { join } from "node:path";

/** Copies kept besides the running one: the last few, for a look at a recent build. */
export const KEEP = 3;

/** The run folders to remove: all but the newest `keep` and `current`. */
export function staleRuns(names, current, keep = KEEP) {
  const runs = names
    .map((name) => ({ name, at: Number(/^run-(\d+)$/.exec(name)?.[1]) }))
    .filter((run) => Number.isFinite(run.at))
    .sort((a, b) => b.at - a.at);
  return runs
    .slice(keep)
    .map((run) => run.name)
    .filter((name) => name !== current);
}

/** Removes stale run folders; one still in use (an instance that did not exit) stays. Returns those removed. */
export function pruneRuns(directory, current, keep = KEEP) {
  let names;
  try {
    names = readdirSync(directory);
  } catch {
    return [];
  }
  const removed = [];
  for (const name of staleRuns(names, current, keep)) {
    try {
      rmSync(join(directory, name), { recursive: true, force: true });
      removed.push(name);
    } catch {
      // A program still mapped from it; tried again after the next build.
    }
  }
  return removed;
}
