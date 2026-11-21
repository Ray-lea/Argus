// Adding and removing catalog keys in every language at once, at the same
// place in each file (scripts/locale-keys.mjs).
import assert from "node:assert/strict";
import { existsSync, readdirSync, readFileSync, statSync, writeFileSync } from "node:fs";
import { join } from "node:path";

export const SOURCE = "zh-CN";
const PLURAL = new Set(["zero", "one", "two", "few", "many", "other"]);

const isText = (value) => typeof value === "string" && value.length > 0;
const isPlural = (value) => value && typeof value === "object" && !Array.isArray(value) && Object.keys(value).length > 0 && Object.entries(value).every(([form, text]) => PLURAL.has(form) && isText(text));
const isGroup = (value) => value && typeof value === "object" && !Array.isArray(value) && !isPlural(value);

/** `tasks.explorer.note` → the catalog file's name and the path inside it. */
export function splitKey(key) {
  const [namespace, ...path] = key.split(".");
  assert(namespace && path.length > 0 && path.every(Boolean), `${key}: expected <file>.<key>, as in tasks.explorer.note`);
  // Objects list whole-number keys first, whatever the order they were added in.
  assert(path.every((part) => !/^\d+$/.test(part)), `${key}: a key made of digits alone would not keep its place`);
  return { namespace, path };
}

/**
 * The catalog with `value` added at `path`. The new key goes after the
 * sibling `after`, before the sibling `before`, or at the end of its group;
 * groups on the way that do not exist yet are created at the end of theirs.
 */
export function inserted(catalog, path, value, { after, before } = {}) {
  assert(isText(value) || isPlural(value), `${path.join(".")}: the text must be a non-empty string or plural forms`);
  assert(!(after && before), `${path.join(".")}: give "after" or "before", not both`);
  const [head, ...rest] = path;
  if (rest.length) {
    const group = catalog[head];
    assert(group === undefined || isGroup(group), `${head} is a text, not a group`);
    return { ...catalog, [head]: inserted(group ?? {}, rest, value, { after, before }) };
  }
  assert(!(head in catalog), `${head} already exists`);
  const anchor = after ?? before;
  if (anchor === undefined) return { ...catalog, [head]: value };
  assert(anchor in catalog, `there is no "${anchor}" beside ${head}`);
  const result = {};
  for (const [key, existing] of Object.entries(catalog)) {
    if (before === key) result[head] = value;
    result[key] = existing;
    if (after === key) result[head] = value;
  }
  return result;
}

/** The catalog without the key at `path`; a group left empty goes with it. */
export function removed(catalog, path) {
  const [head, ...rest] = path;
  assert(head in catalog, `${path.join(".")} does not exist`);
  const result = { ...catalog };
  if (rest.length) {
    assert(isGroup(catalog[head]), `${head} is a text, not a group`);
    const group = removed(catalog[head], rest);
    if (Object.keys(group).length) result[head] = group;
    else delete result[head];
  } else delete result[head];
  return result;
}

const placeholders = (value) => new Set((typeof value === "string" ? [value] : Object.values(value)).flatMap((text) => [...text.matchAll(/\{(\w+)\}/g)].map((match) => match[1])));

/** The languages of a locales folder. */
export function languages(root) {
  return readdirSync(root).filter((name) => statSync(join(root, name)).isDirectory()).sort();
}

/**
 * Carries out `edits` on the catalogs under `root` and returns what to write
 * as `{ path: text }`, with the texts there before as `previous`. Nothing is
 * written here, and one bad entry fails the whole set: a key is in every
 * language or in none.
 *
 * An edit is `{ add: key, texts: { locale: text }, after?, before? }` or `{ remove: key }`.
 */
export function plan(root, edits) {
  const locales = languages(root);
  assert(locales.includes(SOURCE), `${root} has no ${SOURCE}`);
  const files = new Map();
  const open = (locale, namespace) => {
    const path = join(root, locale, `${namespace}.json`);
    if (!files.has(path)) {
      assert(existsSync(path), `locales/${locale}/${namespace}.json does not exist`);
      const text = readFileSync(path, "utf8");
      files.set(path, { previous: text, catalog: JSON.parse(text) });
    }
    return files.get(path);
  };
  for (const edit of edits) {
    const key = edit.add ?? edit.remove;
    const { namespace, path } = splitKey(key);
    if (edit.add) {
      const given = Object.keys(edit.texts ?? {}).sort();
      const missing = locales.filter((locale) => !given.includes(locale));
      const unknown = given.filter((locale) => !locales.includes(locale));
      assert(!missing.length, `${key}: no text for ${missing.join(", ")}`);
      assert(!unknown.length, `${key}: ${unknown.join(", ")} ${unknown.length > 1 ? "are" : "is"} not a language of ${root}`);
      assert(typeof edit.texts[SOURCE] === "string", `${key}: the ${SOURCE} source has no plural forms`);
      const want = [...placeholders(edit.texts[SOURCE])].sort().join(",");
      for (const locale of locales) {
        // A plural form may spell its count out ("one process"); the checker knows which languages may.
        const got = [...placeholders(edit.texts[locale])].sort().join(",");
        assert(typeof edit.texts[locale] !== "string" || got === want, `${key}: ${locale} has placeholders {${got}}, ${SOURCE} has {${want}}`);
      }
    }
    for (const locale of locales) {
      const file = open(locale, namespace);
      try {
        file.catalog = edit.add ? inserted(file.catalog, path, edit.texts[locale], edit) : removed(file.catalog, path);
      } catch (error) {
        throw new Error(`locales/${locale}/${namespace}.json: ${error.message}`);
      }
    }
  }
  const next = {};
  const previous = {};
  for (const [path, file] of files) {
    next[path] = `${JSON.stringify(file.catalog, null, 2)}\n`;
    previous[path] = file.previous;
  }
  return { next, previous };
}

/** Writes texts by path, as `plan` returns them. */
export function write(texts) {
  for (const [path, text] of Object.entries(texts)) writeFileSync(path, text);
}
