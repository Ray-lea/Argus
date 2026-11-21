// What a new worktree takes over from the main checkout's Cargo output.
import { readdirSync } from "node:fs";

// Where Cargo keeps compiled crates and their freshness records: `build`
// alone in the layout of current Cargo (one folder per package and hash, with
// its fingerprint beside its output), `deps` and `.fingerprint` as well in the
// older one. The rest is left behind: `incremental` belongs to the
// workspace's own crates, which are compiled again under the worktree's path
// anyway, and the executables are what the worktree is there to build.
const SEEDED = new Set([".fingerprint", "build", "deps"]);

/** The entries of a profile folder (`target/<profile>`) worth copying into a new worktree. */
export function seedEntries(profileDirectory, list = readdirSync) {
  return list(profileDirectory).filter((entry) => SEEDED.has(entry)).sort();
}
