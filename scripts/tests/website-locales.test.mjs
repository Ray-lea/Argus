import assert from "node:assert/strict";
import test from "node:test";
import { readFile } from "node:fs/promises";
import { languages, localized, languageOfPath, notesLanguage, preferredLanguage } from "../../website/src/lib/languages.ts";
import { translateValue } from "../../website/src/lib/translate.ts";
import { labelTables, translateHtml } from "../../website/scripts/translate-html.mjs";
import { prepareReplicaCatalogs } from "../../website/scripts/locales.mjs";
import { textErrors } from "../lib/locale-validation.mjs";

test("website routes round-trip and changelogs keep their published source language", () => {
  for (const { id, name } of languages) {
    assert.equal(languageOfPath(localized(id, "/download/")), id);
    assert.ok(name.length > 2 && !/^[A-Z]{2,3}$/.test(name));
    assert.equal(notesLanguage(id), ["zh", "zh-TW"].includes(id) ? "zh" : "en");
  }
  assert.equal(languageOfPath("/docs/dev/"), "zh");
});
test("the demo's system choice follows the first supported browser language", () => {
  assert.equal(preferredLanguage(["it-IT", "de-AT", "en-US"]), "de");
  assert.equal(preferredLanguage(["zh-Hant-CN"]), "zh-TW");
  assert.equal(preferredLanguage(["zh-Hans-TW"]), "zh");
  assert.equal(preferredLanguage(["zh-HK"]), "zh-TW");
  assert.equal(preferredLanguage(["zh-CN-x-hant"]), "zh");
  assert.equal(preferredLanguage(["ja-JP"]), "ja");
  assert.equal(preferredLanguage(["pt-BR"]), "en");
});
test("every localized HTML route keeps its CSP-hashed bytes at the edge", async () => {
  const headers = (await readFile(new URL("../../website/public/_headers", import.meta.url), "utf8")).replaceAll("\r", "");
  const rules = headers.split(/\n(?=\S)/);
  for (const { id } of languages) {
    const path = id === "zh" ? "/" : `/${id}/*`;
    const rule = rules.find(block => block.startsWith(path + "\n"));
    assert.ok(rule?.includes("no-transform"), `${path}: missing HTML cache policy`);
  }
});
test("page copy translates nested values and formatter results without changing links", () => {
  const source = { label: "Ready", rows: ["Ready", "/en/download/"], show: value => `Count: ${value}` };
  const translated = translateValue({ Ready: "完成", "Count: {number}": "數量：{number}" }, source);
  assert.equal(translated.label, "完成");
  assert.deepEqual(translated.rows, ["完成", "/en/download/"]);
  assert.equal(translated.show(21), "數量：21");
  assert.equal(source.label, "Ready");
});
test("policy translations retain URLs and literal code while escaping translated HTML", () => {
  const html = '<p>Read <a href="/docs/?mode=1">the guide</a> and <code>--hidden</code>.</p>';
  const translated = translateHtml(html, { "the guide": "指南", "Read {link0} and {code0}.": "<script> {code0} 與 {link0}", "--hidden": "must stay literal" });
  assert.ok(translated.includes('href="/docs/?mode=1"'));
  assert.ok(translated.includes("<code>--hidden</code>"));
  assert.ok(translated.includes("&lt;script&gt;"));
  assert.ok(!translated.includes("must stay literal"));
  assert.throws(() => translateHtml(html, { "the guide": "指南", "Read {link0} and {code0}.": "{link0}" }), /placeholders/);
});
test("narrow policy tables retain the translated column labels", () => {
  const result = labelTables('<table><thead><tr><th>功能</th><th>控制方式</th></tr></thead><tbody><tr><td>更新</td><td><a href="/docs/">說明</a></td></tr></tbody></table>');
  assert.ok(result.includes('data-label="功能"'));
  assert.ok(result.includes('data-label="控制方式"'));
  assert.ok(result.includes('<a href="/docs/">說明</a>'));
});
test("all site catalogs have matching keys, placeholders and protected identifiers", async () => {
  let expected;
  for (const { id } of languages.filter(item => !["zh", "en"].includes(item.id))) {
    const catalog = JSON.parse(await readFile(new URL(`../../website/src/locales/${id}.json`, import.meta.url), "utf8"));
    expected ??= Object.keys(catalog).sort();
    assert.deepEqual(Object.keys(catalog).sort(), expected, id);
    const holes = value => [...new Set([...value.matchAll(/\{(\w+)\}/g)].map(match => match[1]))].sort();
    for (const [source, value] of Object.entries(catalog)) {
      assert.deepEqual(holes(value), holes(source), `${id}: ${source}`);
      assert.deepEqual(textErrors(source, value), [], `${id}: ${source}`);
    }
  }
});
test("demo dictionaries cover each language without untranslated fallbacks", async () => {
  const result = await prepareReplicaCatalogs();
  assert.deepEqual(result.missing, []);
  const base = result.dictionaries.en;
  for (const { id } of languages.filter(item => item.id !== "zh")) {
    assert.deepEqual(Object.keys(result.dictionaries[id].exact).sort(), Object.keys(base.exact).sort());
    assert.deepEqual(result.dictionaries[id].patterns.map(row => row[0]), base.patterns.map(row => row[0]));
  }
});
