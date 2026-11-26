// Compares the captures of one run with the same captures of earlier runs,
// pixel by pixel: the check after a change that must not alter the picture.
//
//   node scripts/compare-shots.mjs <new> <old> [<old>…] [--tolerance N] [--only NAME,…]
//
// <new> and <old> are folders of PNGs, or labels under target/native-validation
// (what ui-shots and ui-smoke write). A capture passes when any old run holds
// an equal one. Give several old runs when captures differ from run to run of
// the same build: curves move with real time even over a fixture, and the
// software rasterizer of CI runners varies by a level here and there.
// For each capture without a match it prints the closest old run, the count of
// changed pixels, the largest channel difference, the box around them and the
// rows they fall in. Exit code 1 when a capture differs or is missing.
import { existsSync, readdirSync, statSync } from "node:fs";
import { basename, join, resolve } from "node:path";
import assert from "node:assert/strict";
import { differences, pixels } from "./lib/capture-pixels.mjs";

const root = resolve(import.meta.dirname, "..");
const argv = process.argv.slice(2);
const option = (name) => {
  const at = argv.indexOf(name);
  return at >= 0 ? argv.splice(at, 2)[1] : undefined;
};
const tolerance = Number(option("--tolerance") ?? 0);
const only = option("--only")?.split(",");
assert(Number.isInteger(tolerance) && tolerance >= 0 && tolerance < 255, "--tolerance is a whole channel difference");
const folder = (name) => {
  for (const path of [resolve(name), resolve(root, "target/native-validation", name)])
    if (existsSync(path) && statSync(path).isDirectory()) return path;
  throw Error(`${name}: no such folder, and no run of that label in target/native-validation`);
};
assert(argv.length >= 2, "usage: node scripts/compare-shots.mjs <new> <old> [<old>…] [--tolerance N] [--only NAME,…]");
const [fresh, ...olds] = argv.map(folder);
const captures = (dir) => readdirSync(dir).filter((name) => name.endsWith(".png")).sort();
const names = captures(fresh).filter((name) => !only || only.includes(name.slice(0, -4)));
assert(names.length, `${fresh}: no captures`);

let same = 0;
const differ = [];
const missing = [];
for (const name of names) {
  const image = pixels(join(fresh, name));
  let closest;
  let found = false;
  for (const old of olds) {
    const path = join(old, name);
    if (!existsSync(path)) continue;
    found = true;
    let result;
    try {
      result = differences(image, pixels(path), tolerance);
    } catch (cause) {
      result = { error: cause.message, changedPixels: Infinity };
    }
    if (!closest || result.changedPixels < closest.changedPixels) closest = { against: basename(old), ...result };
    if (result.changedPixels === 0) break;
  }
  if (!found) missing.push(name);
  else if (closest.changedPixels === 0) same++;
  else differ.push({ name, ...closest, rows: closest.rows?.slice(0, 12) });
}
const stale = olds.flatMap((old) => captures(old).filter((name) => !existsSync(join(fresh, name)) && (!only || only.includes(name.slice(0, -4))))).filter((name, i, all) => all.indexOf(name) === i);

console.log(`${names.length} captures: ${same} ${tolerance ? `within ${tolerance}` : "identical"}, ${differ.length} differ, ${missing.length} in no old run`);
for (const entry of differ) console.log(JSON.stringify(entry));
if (missing.length) console.log(`in no old run: ${missing.join(", ")}`);
if (stale.length) console.log(`only in an old run: ${stale.join(", ")}`);
process.exit(differ.length || missing.length ? 1 : 0);
