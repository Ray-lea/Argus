// Adds or removes catalog keys in every language at once, at the same place
// in each file, then runs the catalog check. If the check fails, the files are
// put back as they were.
//
//   node scripts/locale-keys.mjs add <texts.json>
//   node scripts/locale-keys.mjs remove <file>.<key> [<file>.<key>…]
//
// <texts.json> maps each new key to its text in every language and, when it
// should not go to the end of its group, the key beside it:
//
//   {
//     "tasks.explorer.note": {
//       "after": "confirm",
//       "zh-CN": "…", "zh-TW": "…", "en-US": "…", "es-ES": "…",
//       "de-DE": "…", "ja-JP": "…", "ko-KR": "…", "fr-FR": "…", "ru-RU": "…"
//     }
//   }
//
// The first part of a key is the catalog file (`tasks` is tasks.json, `native`
// is native.json). A text is a string, or plural forms such as
// { "one": "…", "other": "…" } where the language needs them. "before" works
// like "after". Write the file with an editor, not on a command line: quotes
// and non-ASCII text do not survive every shell.
import { spawnSync } from "node:child_process";
import { readFileSync } from "node:fs";
import { relative, resolve } from "node:path";
import assert from "node:assert/strict";
import { plan, write } from "./lib/locale-keys.mjs";

const root = resolve(import.meta.dirname, "..");
const [command, ...rest] = process.argv.slice(2);
let edits;
if (command === "add") {
  assert(rest.length === 1, "usage: node scripts/locale-keys.mjs add <texts.json>");
  const entries = JSON.parse(readFileSync(resolve(rest[0]), "utf8"));
  edits = Object.entries(entries).map(([key, { after, before, ...texts }]) => ({ add: key, texts, after, before }));
} else if (command === "remove") {
  assert(rest.length > 0, "usage: node scripts/locale-keys.mjs remove <file>.<key> [<file>.<key>…]");
  edits = rest.map((key) => ({ remove: key }));
} else {
  console.error("usage: node scripts/locale-keys.mjs add <texts.json> | remove <file>.<key>…");
  process.exit(2);
}
assert(edits.length > 0, "nothing to do");

let changes;
try {
  changes = plan(resolve(root, "locales"), edits);
} catch (error) {
  console.error(error.message);
  process.exit(1);
}
write(changes.next);
const check = spawnSync(process.execPath, [resolve(root, "scripts/i18n.mjs")], { cwd: root, encoding: "utf8" });
if (check.status !== 0) {
  write(changes.previous);
  console.error(`${check.stdout}${check.stderr}`.trim());
  console.error("\nThe catalog check failed; the catalogs are as they were.");
  process.exit(1);
}
console.log(`${command === "add" ? "added" : "removed"} ${edits.map((edit) => edit.add ?? edit.remove).join(", ")} in ${Object.keys(changes.next).length} files:`);
for (const path of Object.keys(changes.next)) console.log(`  ${relative(root, path).replaceAll("\\", "/")}`);
