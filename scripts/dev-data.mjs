// Development profiles only. A data format or protocol changes its version number at
// most once between releases; a later incompatible change in the same cycle keeps the
// number, and the files development builds wrote before it are removed here, never by
// the application. Raise the kind's revision in dev-data.json in the same commit: the
// development launcher clears that kind before it starts the next build.
//
//   node scripts/dev-data.mjs                   apply dev-data.json to this checkout's profile
//   node scripts/dev-data.mjs clear <kind>...   clear these kinds now (Argus must not be running)
//   --identifier <id>                           another development profile
import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { mkdirSync, readdirSync, readFileSync, realpathSync, rmSync, writeFileSync } from "node:fs";
import { dirname, isAbsolute, join, resolve } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";

const workspace = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const STAMP = "dev-data.json";

// The files each kind owns, all direct children of the history folder.
export const KINDS = {
  history: /^\d{4}-\d{2}-\d{2}\.(?:hist|proc|roll|idx|measure|merge-tmp)$/,
  watch: /^(?:process-watch\.jsonl|program-watch\.v\d+\.bin)$/,
};

export function releaseIdentifier(root = workspace) {
  return JSON.parse(readFileSync(join(root, "resources", "app.json"), "utf8")).identifier;
}

// Mirrors argus_app::config::development_identifier for the checkout's src-gpui.
export function developmentIdentifier(root = workspace, release = releaseIdentifier(root)) {
  const manifest = join(resolve(root), "src-gpui").replaceAll("\\", "/").toLowerCase();
  return `${release}.devbuild.${createHash("sha256").update(manifest).digest("hex").slice(0, 12)}`;
}

// The profile a launch with these arguments runs under.
export function launchIdentifier(args, root = workspace) {
  const at = args.indexOf("--identifier");
  return at >= 0 && args[at + 1] ? args[at + 1] : developmentIdentifier(root);
}

function historyDirectory(identifier, environment) {
  const fallback = join(environment.LOCALAPPDATA, identifier, "history");
  try {
    const dir = JSON.parse(readFileSync(join(environment.APPDATA, identifier, "settings.json"), "utf8"))?.historyDir;
    return typeof dir === "string" && isAbsolute(dir) ? dir : fallback;
  } catch {
    return fallback;
  }
}

function sameDirectory(a, b) {
  const real = (path) => {
    try {
      return realpathSync.native(path);
    } catch {
      return resolve(path);
    }
  };
  return real(a).toLowerCase() === real(b).toLowerCase();
}

// A development build's history folder and the record of the revisions cleared there.
export function developmentProfile(identifier, environment = process.env, release = releaseIdentifier()) {
  assert(environment.APPDATA && environment.LOCALAPPDATA, "APPDATA and LOCALAPPDATA required");
  assert(/^[A-Za-z0-9_-]+(?:\.[A-Za-z0-9_-]+)+$/.test(identifier), `invalid identifier: ${identifier}`);
  assert(identifier.toLowerCase().startsWith(`${release.toLowerCase()}.`), `not a development identifier: ${identifier}`);
  const history = historyDirectory(identifier, environment);
  // The installed application's records are never a development build's to clear.
  for (const dir of [join(environment.LOCALAPPDATA, release, "history"), historyDirectory(release, environment)])
    assert(!sameDirectory(dir, history), `history folder shared with the installed application: ${history}`);
  return { history, stamp: join(environment.LOCALAPPDATA, identifier, STAMP) };
}

export function readRevisions(path = join(workspace, "scripts", "dev-data.json")) {
  const revisions = JSON.parse(readFileSync(path, "utf8"));
  for (const [kind, revision] of Object.entries(revisions))
    assert(Object.hasOwn(KINDS, kind) && Number.isSafeInteger(revision), `invalid revision: ${kind}`);
  return revisions;
}

// Removes the files of these kinds; a file that is still open fails the call.
export function clear(profile, kinds) {
  for (const kind of kinds) assert(Object.hasOwn(KINDS, kind), `unknown kind: ${kind}`);
  let entries;
  try {
    entries = readdirSync(profile.history, { withFileTypes: true });
  } catch (error) {
    if (error.code === "ENOENT") return [];
    throw error;
  }
  const removed = [];
  for (const entry of entries) {
    if (!entry.isFile() || !kinds.some((kind) => KINDS[kind].test(entry.name))) continue;
    const path = join(profile.history, entry.name);
    rmSync(path);
    removed.push(path);
  }
  return removed;
}

// Clears every kind whose revision differs from the profile's record, then records them.
// A profile without a record predates it: everything listed is cleared once.
export function prepare(profile, revisions = readRevisions()) {
  let cleared = {};
  try {
    cleared = JSON.parse(readFileSync(profile.stamp, "utf8")) ?? {};
  } catch {}
  const stale = Object.keys(revisions).filter((kind) => cleared[kind] !== revisions[kind]);
  const removed = stale.length ? clear(profile, stale) : [];
  mkdirSync(dirname(profile.stamp), { recursive: true });
  writeFileSync(profile.stamp, `${JSON.stringify(revisions)}\n`);
  return { stale, removed };
}

if (process.argv[1] && import.meta.url === pathToFileURL(resolve(process.argv[1])).href) {
  const args = process.argv.slice(2);
  const at = args.indexOf("--identifier");
  const positional = at >= 0 ? args.filter((_, i) => i !== at && i !== at + 1) : args;
  const profile = developmentProfile(launchIdentifier(args));
  if (positional[0] === "clear") {
    const kinds = positional.slice(1);
    assert(kinds.length, `name the kinds to clear: ${Object.keys(KINDS).join(", ")}`);
    console.log(`已从 ${profile.history} 删除 ${clear(profile, kinds).length} 个文件。`);
  } else {
    assert(!positional.length, `unknown arguments: ${positional.join(" ")}`);
    const { stale, removed } = prepare(profile);
    console.log(stale.length ? `已清除 ${stale.join("、")}：从 ${profile.history} 删除 ${removed.length} 个文件。` : "开发数据与当前格式一致，无需清除。");
  }
}
