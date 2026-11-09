// Checks the translations in locales/ against the source (zh-CN) catalogs and
// the code that uses them; run by `pnpm check` and CI. See docs/development/i18n.md.
//
//   node scripts/i18n.mjs           check, exit 1 on errors
//   node scripts/i18n.mjs --unused  also list keys nothing seems to use
//
// - every locale has the source's namespaces and keys, and no others;
// - a translation keeps the source's {placeholders} and <tags>; a locale may
//   split a message with {count} into plural forms ({"one": …, "other": …});
// - non-CJK translations contain no Han text except native language names;
// - every key the Rust code looks up (tr!("…")) exists, with the same placeholders;
// - user-facing text is not written into the code: Chinese outside comments,
//   tests and tools/window-probe/fixtures is an error. Mark a deliberate exception with
//   `i18n-ignore` in a comment on the line.

import { readdirSync, readFileSync, statSync } from "node:fs";
import { join, relative } from "node:path";
import { macroCalls } from "./lib/rust-calls.mjs";
import { maySpellOne, textErrors } from "./lib/locale-validation.mjs";
import { installerLanguages } from "./lib/installer-locales.mjs";

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

const registered = [...readFileSync(join(root, "src-gpui/src/catalog.rs"), "utf8").matchAll(/register!\(\w+,\s*"([^"]+)"\)/g)].map(m => m[1]);
const nativeLocales = [...readFileSync(join(root, "crates/argus-core/src/i18n.rs"), "utf8").matchAll(/#\[serde\(rename = "([^"]+)"\)\]/g)].map(m => m[1]);
for (const [where, tags] of [["desktop catalogs", registered], ["native locales", nativeLocales], ["installer languages", Object.keys(installerLanguages)]]) {
  if (tags.length !== locales.length || !same(new Set(locales), new Set(tags))) error(where, "registered languages differ from locales/");
}
const nsisLanguages = [...readFileSync(join(root, "packaging/windows/installer-template.nsi"), "utf8").matchAll(/^!insertmacro MUI_LANGUAGE "([^"]+)"/gm)].map(m => m[1].toUpperCase());
if (nsisLanguages.length !== locales.length || !same(new Set(nsisLanguages), new Set(Object.values(installerLanguages)))) error("installer template", "MUI languages differ from installer catalogs");
for (const [locale, namespaces] of catalogs) {
  const months = namespaces.get("format")?.get("monthNames");
  if (typeof months !== "string" || months.trim().split(/\s+/u).length !== 12) error(`locales/${locale}/format.json`, "monthNames must contain twelve entries");
  for (const tag of locales) {
    const key = `languageName.${tag.replace("-", "")}`;
    if (namespaces.get("common")?.get(key) !== source.get("common")?.get(key)) error(`locales/${locale}/common.json`, `${key} must keep its native name`);
  }
}

for (const [ns, keys] of source) {
  for (const [key, text] of keys) {
    if (typeof text !== "string") error(`locales/${SOURCE}/${ns}.json`, `${key}: the source has no plural forms`);
    else for (const message of textErrors(text, text)) error(`locales/${SOURCE}/${ns}.json`, `${key}: ${message}`);
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
        const categories = new Intl.PluralRules(locale).resolvedOptions().pluralCategories;
        for (const category of categories) if (!(category in value)) error(file, `${key}: plural forms need "${category}" for ${locale}`);
        for (const category of Object.keys(value)) if (!categories.includes(category)) error(file, `${key}: unused plural form "${category}" for ${locale}`);
      }
      for (const [form, text] of Object.entries(forms)) {
        const label = form === "text" ? key : `${key}.${form}`;
        const got = placeholders(text);
        // A plural form may spell its count out ("one process").
        if (form === "one" && maySpellOne(locale) && want.has("count")) got.add("count");
        if (!same(got, want)) error(file, `${label}: placeholders ${list(got)}, ${SOURCE} has ${list(want)}`);
        if (tags(text) !== tags(original)) error(file, `${label}: tags differ from ${SOURCE}`);
        if (!/^(zh|ja)(-|$)/.test(locale) && !(ns === "common" && key.startsWith("languageName.")) && HAN.test(text)) error(file, `${label}: untranslated Chinese text`);
        if (/QZX\d|XZQ|\[ZX\d+XZ\]/.test(text)) error(file, `${label}: translation marker left in text`);
        for (const message of textErrors(original, text)) error(file, `${label}: ${message}`);
      }
    }
  }
}

// ---- Code ----------------------------------------------------------------------------

const rustDirs = ["src-gpui/src", "crates/argus-core/src", "crates/argus-service/src", "crates/argus-app/src"];
const rustFiles = rustDirs.flatMap((d) => walk(join(root, d))).filter((f) => f.endsWith(".rs"));

// Rust lookups: tr!("key") and tr!("key", name = value, …).
const native = source.get("native") ?? new Map();
const application = new Map([...source].filter(([ns]) => ns !== "native" && ns !== "installer").flatMap(([ns, keys]) => [...keys].map(([key, value]) => [`${ns}.${key}`, value])));
for (const file of rustFiles) {
  const text = readFileSync(file, "utf8");
  // Each call on its own, with only its own arguments: one nested in another's
  // arguments is checked too, and lends the outer call none of its names.
  for (const { key, names, index } of macroCalls(text, "tr")) {
    const line = text.slice(0, index).split("\n").length;
    const where = `${rel(file)}:${line}`;
    const template = native.get(key) ?? (rel(file).startsWith("src-gpui/") ? application.get(key) : undefined);
    if (template === undefined) {
      error(where, `tr!("${key}") is not in locales/${SOURCE}/native.json`);
      continue;
    }
    const args = new Set(names);
    const want = placeholders(template);
    if (!same(args, want)) error(where, `tr!("${key}") passes ${list(args)}, the text has ${list(want)}`);
  }
}

// Native view adapters also look up labels without placeholders.
for (const file of rustFiles.filter((file) => rel(file).startsWith("src-gpui/src/frontend"))) {
  const text = readFileSync(file, "utf8");
  for (const match of text.matchAll(/\b(?:counted|quantity)\(\s*"([\w.]+)"/g)) {
    const template = application.get(match[1]) ?? native.get(match[1]);
    if (template === undefined || !placeholders(template).has("count")) error(rel(file), `counted needs a catalog key with {count}: ${match[1]}`);
  }
  for (const match of text.matchAll(/\blabel\(\s*"([\w.]+)"\s*\)/g)) {
    const key = match[1];
    if (!native.has(key) && !application.has(key)) error(rel(file), `unknown native view label: ${key}`);
    else if (placeholders(native.get(key) ?? application.get(key)).size) error(rel(file), `label needs formatted arguments: ${key}`);
  }
}

// Text written into the code instead of a catalog.
for (const file of rustFiles) {
  const name = rel(file);
  let inTest = false;
  let inBlock = false;
  readFileSync(file, "utf8")
    .split("\n")
    .forEach((line, i) => {
      if (/#\[cfg\(test\)\]/.test(line)) inTest = true;
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
      code = code.replace(/(^|[^:"'])\/\/.*$/, "$1");
      if (/^\s*\*/.test(code)) return;
      if (HAN.test(code)) error(`${name}:${i + 1}`, `text in code: ${code.trim().slice(0, 80)}`);
    });
}

// ---- Unused keys (informational) --------------------------------------------------------

if (process.argv.includes("--unused")) {
  const literals = new Set(
    rustFiles.flatMap((f) => [...readFileSync(f, "utf8").matchAll(/"((?:[^"\\\n]|\\.)*)"/g)].map((m) => m[1])),
  );
  // Keys built with format!, e.g. "shell.page.{page}", as patterns.
  const dynamic = [...literals]
    .filter((l) => l.includes("{") && /^[a-zA-Z]+\.[\w.{}:?]*$/.test(l))
    .map((l) => new RegExp(`^${l.split(/\{[^}]*\}/).map((s) => s.replaceAll(".", "\\.")).join("[\\w.]+")}$`));
  // A parent below the namespace written on its own ("settings.privilege.install")
  // is extended at run time ("{prefix}.title").
  const extended = (key) => key.split(".").some((_, i, parts) => i >= 2 && literals.has(parts.slice(0, i).join(".")));
  for (const [ns, keys] of source) {
    if (ns === "installer") continue; // Referenced by generated NSIS LangStrings.
    for (const key of keys.keys()) {
      const full = ns === "native" ? key : `${ns}.${key}`;
      if (literals.has(full) || dynamic.some((re) => re.test(full)) || extended(full)) continue;
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
