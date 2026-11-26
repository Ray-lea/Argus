// Where the window probe is: the one named, this checkout's release build, or
// else the main checkout's. A worktree builds its own program, but the probe
// only with a release build of its own; the probe seldom changes, so the main
// checkout's serves until the worktree changes the probe itself.
import { execFileSync } from "node:child_process";
import { existsSync } from "node:fs";
import { dirname, resolve } from "node:path";

export const PROBE = "target/release/argus-window-probe.exe";

/** The folder of the main checkout of the repository at `cwd`, or null. */
export function mainCheckout(cwd = process.cwd(), run = execFileSync) {
  try {
    const common = run("git", ["rev-parse", "--path-format=absolute", "--git-common-dir"], {
      cwd,
      encoding: "utf8",
      stdio: ["ignore", "pipe", "ignore"],
      windowsHide: true,
    }).trim();
    return common ? dirname(common) : null;
  } catch {
    return null;
  }
}

/**
 * The probe to run: `named` when given, else this checkout's, else the main
 * checkout's (`shared`). When none exists the path is this checkout's, for the
 * caller's "build it first" message.
 */
export function windowProbe(named, { root = process.cwd(), exists = existsSync, main = () => mainCheckout(root) } = {}) {
  if (named) return { path: resolve(root, named), shared: false };
  const own = resolve(root, PROBE);
  if (exists(own)) return { path: own, shared: false };
  const checkout = main();
  const theirs = checkout ? resolve(checkout, PROBE) : null;
  if (theirs && theirs.toLowerCase() !== own.toLowerCase() && exists(theirs)) return { path: theirs, shared: true };
  return { path: own, shared: false };
}
