// Checks the translations in locales/ against the source (zh-CN) catalogs and
// the code that uses them; run by `pnpm check` and CI. See docs/development/i18n.md.
//
//   node scripts/i18n.mjs           check, exit 1 on errors
//   node scripts/i18n.mjs --unused  also list keys nothing seems to use
//
// - every locale has the source's namespaces and keys, and no others;
// - a translation keeps the source's {placeholders} and <tags>; a locale may
//   split a message with {count} into plural forms ({"one": …, "other": …});
// - translations other than Chinese contain no Chinese text;
// - every key the Rust code looks up (tr!("…")) exists, with the same placeholders;
// - user-facing text is not written into the code: Chinese outside comments,
//   tests and ui/src/lib/mock.ts is an error. Mark a deliberate exception with
//   `i18n-ignore` in a comment on the line.

import { readdirSync, readFileSync, statSync } from "node:fs";
import { join, relative } from "node:path";

const root = join(import.meta.dirname, "..");
const SOURCE = "zh-CN";
const PLURAL = new Set(["zero", "one", "two", "few", "many", "other"]);
const HAN = /[㐀-鿿豈-﫿]/;

const errors = [];
const error = (where, message) => errors.push(`${where}: ${message}`);

const walk = (dir) =>
  readdirSync(dir).flatMap((name) => {
    const path = join(dir, name);
    return statSync(path).isDirectory() ? walk(path) : [path];
  });
const rel = (path) => relative(root, path).replaceAll("\\", "/");

// ---- Catalogs ----------------------------------------------------------------------

const localesDir = join(root, "locales");
const locales = readdirSync(localesDir).filter((name) => statSync(join(localesDir, name)).isDirectory());

/** Flattens a catalog to dotted keys; plural objects stay whole. */
function flatten(node, prefix, out, file) {
  for (const [key, value] of Object.entries(node)) {
    const path = prefix ? `${prefix}.${key}` : key;
    if (typeof value === "string") out.set(path, value);
    else if (value && typeof value === "object" && isPlural(value)) out.set(path, value);
    else if (value && typeof value === "object") flatten(value, path, out, file);
    else error(file, `${path} is neither text nor a group`);
  }
  return out;
}
const isPlural = (value) =>
  Object.keys(value).length > 0 &&
  Object.keys(value).every((k) => PLURAL.has(k)) &&
  Object.values(value).every((v) => typeof v === "string");

function load(locale) {
  const dir = join(localesDir, locale);
  const namespaces = new Map();
  for (const name of readdirSync(dir).filter((n) => n.endsWith(".json"))) {
    const file = `locales/${locale}/${name}`;
    try {
      namespaces.set(name.slice(0, -5), flatten(JSON.parse(readFileSync(join(dir, name), "utf8")), "", new Map(), file));
    } catch (e) {
      error(file, `invalid JSON: ${e.message}`);
    }
  }
  return namespaces;
}

const placeholders = (text) => new Set([...text.matchAll(/\{(\w+)\}/g)].map((m) => m[1]));
const tags = (text) => [...text.matchAll(/<(\w+)>/g)].map((m) => m[1]).sort().join(",");
const same = (a, b) => a.size === b.size && [...a].every((x) => b.has(x));
const list = (set) => [...set].map((x) => `{${x}}`).join(" ") || "none";

const catalogs = new Map(locales.map((l) => [l, load(l)]));
const source = catalogs.get(SOURCE);
if (!source) throw new Error(`locales/${SOURCE} is missing`);

for (const [ns, keys] of source) {
  for (const [key, text] of keys) {
    if (typeof text !== "string") error(`locales/${SOURCE}/${ns}.json`, `${key}: the source has no plural forms`);
  }
}

for (const [locale, namespaces] of catalogs) {
  if (locale === SOURCE) continue;
  for (const ns of source.keys()) if (!namespaces.has(ns)) error(`locales/${locale}`, `${ns}.json is missing`);
  for (const [ns, keys] of namespaces) {
    const file = `locales/${locale}/${ns}.json`;
    const base = source.get(ns);
    if (!base) {
      error(file, `no such namespace in ${SOURCE}`);
      continue;
    }
    for (const key of base.keys()) if (!keys.has(key)) error(file, `${key} is missing`);
    for (const [key, value] of keys) {
      const original = base.get(key);
      if (original === undefined) {
        error(file, `${key} is not in ${SOURCE}`);
        continue;
      }
      if (typeof original !== "string") continue;
      const want = placeholders(original);
      const forms = typeof value === "string" ? { text: value } : value;
      if (typeof value !== "string") {
        if (!want.has("count")) error(file, `${key}: plural forms need {count} in the source text`);
        if (!("other" in value)) error(file, `${key}: plural forms need "other"`);
      }
      for (const [form, text] of Object.entries(forms)) {
        const label = form === "text" ? key : `${key}.${form}`;
        const got = placeholders(text);
        // A plural form may spell its count out ("one process").
        if (form !== "text" && form !== "other" && want.has("count")) got.add("count");
        if (!same(got, want)) error(file, `${label}: placeholders ${list(got)}, ${SOURCE} has ${list(want)}`);
        if (tags(text) !== tags(original)) error(file, `${label}: tags differ from ${SOURCE}`);
        if (!locale.startsWith("zh") && HAN.test(text)) error(file, `${label}: untranslated Chinese text`);
      }
    }
  }
}

// ---- Code ----------------------------------------------------------------------------

const rustDirs = ["src-tauri/src", "crates/argus-core/src", "crates/argus-service/src"];
const rustFiles = rustDirs.flatMap((d) => walk(join(root, d))).filter((f) => f.endsWith(".rs"));
const uiFiles = walk(join(root, "ui/src")).filter((f) => /\.tsx?$/.test(f));

// Rust lookups: tr!("key") and tr!("key", name = value, …).
const native = source.get("native") ?? new Map();
for (const file of rustFiles) {
  const text = readFileSync(file, "utf8");
  for (const m of text.matchAll(/\btr!\(\s*"([^"]+)"((?:[^()]|\((?:[^()]|\([^()]*\))*\))*)\)/g)) {
    const [, key, rest] = m;
    const line = text.slice(0, m.index).split("\n").length;
    const where = `${rel(file)}:${line}`;
    const template = native.get(key);
    if (template === undefined) {
      error(where, `tr!("${key}") is not in locales/${SOURCE}/native.json`);
      continue;
    }
    const args = new Set([...rest.matchAll(/(?:^|,)\s*(\w+)\s*=(?!=)/g)].map((a) => a[1]));
    const want = placeholders(template);
    if (!same(args, want)) error(where, `tr!("${key}") passes ${list(args)}, the text has ${list(want)}`);
  }
}

// Text written into the code instead of a catalog.
const exempt = new Set(["ui/src/lib/mock.ts"]);
for (const file of [...rustFiles, ...uiFiles]) {
  const name = rel(file);
  if (exempt.has(name)) continue;
  const rust = file.endsWith(".rs");
  let inTest = false;
  let inBlock = false;
  readFileSync(file, "utf8")
    .split("\n")
    .forEach((line, i) => {
      if (rust && /#\[cfg\(test\)\]/.test(line)) inTest = true;
      if (inTest || line.includes("i18n-ignore")) return;
      let code = line;
      if (inBlock) {
        const end = code.indexOf("*/");
        if (end < 0) return;
        code = code.slice(end + 2);
        inBlock = false;
      }
      code = code.replace(/\/\*.*?\*\//g, "");
      if (code.includes("/*")) {
        code = code.slice(0, code.indexOf("/*"));
        inBlock = true;
      }
      // Line comments (but not "//" inside a string such as a URL).
      code = code.replace(/(^|[^:"'`])\/\/.*$/, "$1");
      if (/^\s*\*/.test(code)) return;
      if (HAN.test(code)) error(`${name}:${i + 1}`, `text in code: ${code.trim().slice(0, 80)}`);
    });
}

// ---- Unused keys (informational) --------------------------------------------------------

if (process.argv.includes("--unused")) {
  const code = [...rustFiles, ...uiFiles].map((f) => readFileSync(f, "utf8")).join("\n");
  // Keys built from template literals, e.g. `shell.page.${p}`, as patterns.
  const dynamic = [...code.matchAll(/`([\w.]+\.)\$\{[^}]+\}([\w.]*)`/g)].map(
    (m) => new RegExp(`^${m[1].replaceAll(".", "\\.")}[\\w]+${m[2].replaceAll(".", "\\.")}$`),
  );
  for (const [ns, keys] of source) {
    for (const key of keys.keys()) {
      const full = ns === "native" ? key : `${ns}.${key}`;
      if (code.includes(`"${full}"`) || dynamic.some((re) => re.test(full))) continue;
      console.log(`unused? ${ns}: ${key}`);
    }
  }
}

if (errors.length) {
  console.error(errors.join("\n"));
  console.error(`\n${errors.length} problem${errors.length === 1 ? "" : "s"}; see docs/development/i18n.md`);
  process.exit(1);
}
console.log(`i18n: ${locales.length} locales, ${[...source.values()].reduce((n, k) => n + k.size, 0)} keys each, OK`);
