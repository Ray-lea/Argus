// Vendored GPUI against the packages it was taken from (scripts/vendor-patches.mjs).
// The local branch `vendor-upstream` holds each upstream version as published
// on crates.io, laid out as vendor/ is and with the files the snapshots leave
// out left out, so that `git diff vendor-upstream -- vendor/` is the patch set.
import { gunzipSync } from "node:zlib";

export const UPSTREAM_BRANCH = "vendor-upstream";

// Packaging files Cargo adds to every published package; the workspace lockfile
// and the normalized manifest take their place.
const PACKAGING = ["Cargo.lock", "Cargo.toml.orig", ".cargo_vcs_info.json"];

/**
 * The vendored packages (folders under vendor/, named as on crates.io) and what
 * their snapshots leave out of the published package: paths relative to the
 * package root, a trailing slash for a folder. `keep` overrides `omit`.
 */
export const VENDORED = [
  // The standalone examples, their media and the benchmark are omitted; the
  // EXIF fixture stays, as the package's own source tests read it.
  { name: "gpui-pre", omit: [...PACKAGING, "benches/", "examples/"], keep: ["examples/image/exif-orientation-rotate-180.jpg"] },
  { name: "gpui-pre-windows", omit: PACKAGING },
];

/** Whether the vendored snapshot of `spec` keeps `path` (relative to the package root). */
export function keeps(spec, path) {
  if (spec.keep?.includes(path)) return true;
  return !spec.omit.some((rule) => (rule.endsWith("/") ? path.startsWith(rule) : path === rule));
}

const text = (bytes) => {
  const end = bytes.indexOf(0);
  return bytes.subarray(0, end < 0 ? bytes.length : end).toString("utf8");
};

const octal = (bytes) => {
  // Sizes past 8 GiB are base-256 with the top bit set; no source file is that large.
  if (bytes[0] & 0x80) throw new Error("tar: base-256 sizes are not supported");
  const digits = text(bytes).trim();
  return digits ? Number.parseInt(digits, 8) : 0;
};

/**
 * The regular files of a tar archive, in order: [{ path, mode, data }]. Reads
 * ustar names with their prefix, GNU long names (`L`) and pax `path` records,
 * which is what Cargo writes into a `.crate`.
 */
export function readTar(buffer) {
  const files = [];
  let longName = null;
  let offset = 0;
  while (offset + 512 <= buffer.length) {
    const header = buffer.subarray(offset, offset + 512);
    if (header.every((byte) => byte === 0)) break;
    const size = octal(header.subarray(124, 136));
    const type = String.fromCharCode(header[156] || 0x30);
    const data = buffer.subarray(offset + 512, offset + 512 + size);
    if (data.length < size) throw new Error("tar: truncated archive");
    offset += 512 + Math.ceil(size / 512) * 512;
    if (type === "L") {
      longName = text(data);
      continue;
    }
    if (type === "x") {
      // Records are "<length> <key>=<value>\n".
      const records = data.toString("utf8");
      for (let at = 0; at < records.length; ) {
        const space = records.indexOf(" ", at);
        const length = Number(records.slice(at, space));
        if (!(length > 0)) throw new Error("tar: malformed pax header");
        const record = records.slice(space + 1, at + length - 1);
        if (record.startsWith("path=")) longName = record.slice(5);
        at += length;
      }
      continue;
    }
    if (type === "g") continue;
    const ustar = text(header.subarray(257, 263)).startsWith("ustar");
    const prefix = ustar ? text(header.subarray(345, 500)) : "";
    const name = longName ?? (prefix ? `${prefix}/${text(header.subarray(0, 100))}` : text(header.subarray(0, 100)));
    longName = null;
    if (type === "0" || type === "\0" || type === "7") files.push({ path: name, mode: octal(header.subarray(100, 108)), data: Buffer.from(data) });
    else if (type !== "5") throw new Error(`tar: ${name}: unsupported entry type ${JSON.stringify(type)}`);
  }
  return files;
}

/**
 * The files a vendored snapshot of `spec` takes from its published package (a
 * `.crate`, a gzipped tar of `<name>-<version>/…`), sorted by path relative to
 * the package root: [{ path, data }].
 */
export function packageFiles(crate, spec, version) {
  const root = `${spec.name}-${version}/`;
  const files = [];
  for (const file of readTar(gunzipSync(crate))) {
    if (!file.path.startsWith(root)) throw new Error(`${spec.name} ${version}: ${file.path} lies outside ${root}`);
    const path = file.path.slice(root.length);
    if (path.split("/").some((part) => part === ".." || part === "")) throw new Error(`${spec.name} ${version}: unsafe path ${file.path}`);
    if (keeps(spec, path)) files.push({ path, data: file.data });
  }
  return files.sort((a, b) => (a.path < b.path ? -1 : a.path > b.path ? 1 : 0));
}

/** The `version` of a manifest's `[package]` table, or null. */
export function manifestVersion(manifest) {
  let inPackage = false;
  for (const line of manifest.split(/\r?\n/)) {
    const table = /^\s*\[([^\]]+)\]\s*$/.exec(line);
    if (table) inPackage = table[1].trim() === "package";
    else if (inPackage) {
      const match = /^\s*version\s*=\s*"([^"]+)"/.exec(line);
      if (match) return match[1];
    }
  }
  return null;
}

/**
 * `git diff --numstat -z --no-renames` and `git diff --name-status -z
 * --no-renames` of the same comparison, as rows { path, status, added, removed }
 * (`added` and `removed` are null for a binary file).
 */
export function parseDiffStat(numstat, nameStatus) {
  const statuses = new Map();
  const fields = nameStatus.split("\0");
  for (let i = 0; i + 1 < fields.length; i += 2) if (fields[i]) statuses.set(fields[i + 1], fields[i][0]);
  const rows = [];
  for (const entry of numstat.split("\0")) {
    if (!entry) continue;
    const [added, removed, ...path] = entry.split("\t");
    const name = path.join("\t");
    const count = (value) => (value === "-" ? null : Number(value));
    rows.push({ path: name, status: statuses.get(name) ?? "M", added: count(added), removed: count(removed) });
  }
  return rows;
}

const changed = (row) => (row.added ?? 0) + (row.removed ?? 0);

/** Rows by changed lines, most first, then by path. */
export function sortRows(rows) {
  return [...rows].sort((a, b) => changed(b) - changed(a) || (a.path < b.path ? -1 : a.path > b.path ? 1 : 0));
}

/**
 * Files, added and removed lines per vendored package (the folder under
 * vendor/), by name, then overall under the empty key.
 */
export function totals(rows) {
  const sum = (selected) => ({
    files: selected.length,
    added: selected.reduce((lines, row) => lines + (row.added ?? 0), 0),
    removed: selected.reduce((lines, row) => lines + (row.removed ?? 0), 0),
  });
  const packageOf = (row) => /^vendor\/([^/]+)\//.exec(row.path)?.[1] ?? row.path;
  const names = [...new Set(rows.map(packageOf))].sort();
  return new Map([...names.map((name) => [name, sum(rows.filter((row) => packageOf(row) === name))]), ["", sum(rows)]]);
}

/** The rows as a table: added, removed, status (A for a local file, M patched, D removed), path. */
export function formatRows(rows) {
  const width = Math.max(1, ...rows.flatMap((row) => [String(row.added ?? "-").length, String(row.removed ?? "-").length]));
  return rows.map((row) => `${String(row.added ?? "-").padStart(width)} ${String(row.removed ?? "-").padStart(width)}  ${row.status}  ${row.path}`);
}

/**
 * How to bring the patches over to a new upstream version, path by path. Each
 * argument maps a path to its blob id: `base` the upstream version the patches
 * were made on, `ours` the vendored files, `theirs` the new upstream version.
 * Actions: `take` (write or delete as upstream has it: the file carries no
 * patch), `merge` (three-way merge), `conflict` (no automatic answer; `reason`
 * says why). Paths that stay as they are vendored are not listed.
 */
export function mergePlan(base, ours, theirs) {
  const paths = [...new Set([...base.keys(), ...ours.keys(), ...theirs.keys()])].sort();
  const plan = [];
  for (const path of paths) {
    const [b, o, t] = [base.get(path), ours.get(path), theirs.get(path)];
    if (o === t || b === t) continue;
    if (b === o) plan.push({ path, action: "take", deleted: t === undefined });
    else if (b !== undefined && o !== undefined && t !== undefined) plan.push({ path, action: "merge" });
    else if (t === undefined) plan.push({ path, action: "conflict", reason: "removed upstream, patched locally" });
    else if (o === undefined) plan.push({ path, action: "conflict", reason: "removed locally, changed upstream" });
    else plan.push({ path, action: "conflict", reason: "added both upstream and locally" });
  }
  return plan;
}
