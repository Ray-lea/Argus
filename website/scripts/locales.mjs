// Reuse application translations by key; the site's own messages cover demo-only text.
import { readFile, readdir } from "node:fs/promises";
import { join } from "node:path";
import { languages } from "../src/lib/languages.ts";
import { exact, patterns } from "../src/components/replica/i18n/en.ts";
import { repoRoot, websiteRoot, contentDir, putIfChanged } from "./paths.mjs";

const forms = new Set(["one", "few", "many", "other"]);
const isPlural = value => value && typeof value === "object" && Object.keys(value).every(key => forms.has(key));
function flatten(value, prefix = "", out = {}) {
  for (const [key, child] of Object.entries(value)) {
    const path = prefix + key;
    if (typeof child === "string" || isPlural(child)) out[path] = child;
    else flatten(child, path + ".", out);
  }
  return out;
}
async function appCatalog(tag) {
  const result = {};
  for (const name of await readdir(join(repoRoot, "locales", tag))) {
    if (!name.endsWith(".json")) continue;
    for (const [key, value] of Object.entries(flatten(JSON.parse(await readFile(join(repoRoot, "locales", tag, name), "utf8"))))) result[name + ":" + key] = value;
  }
  return result;
}
function pattern(template) {
  const names = [];
  const body = template.split(/(\{\w+\})/).map(part => {
    const name = /^\{(\w+)\}$/.exec(part);
    if (name) { names.push(name[1]); return name[1] === "count" ? "(-?\\d+(?:\\.\\d+)?)" : "(.+?)"; }
    return part.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
  }).join("");
  return { names, regex: new RegExp("^" + body + "$", "s") };
}
const fill = (template, args) => template.replace(/\{(\w+)\}/g, (hole, key) => args[key] ?? hole);
export async function prepareReplicaCatalogs({ strict = true } = {}) {
  const mapping = JSON.parse(await readFile(join(websiteRoot, "src/components/replica/i18n/catalog-map.json"), "utf8"));
  const source = await appCatalog("zh-CN");
  const templates = Object.entries(source).filter(([, value]) => typeof value === "string" && /\{\w+\}/.test(value) && value.replace(/\{\w+\}/g, "").trim().length >= 2)
    .sort((a,b) => b[1].replace(/\{\w+\}/g, "").length - a[1].replace(/\{\w+\}/g, "").length)
    .map(([key, value]) => ({ key, ...pattern(value) }));
  const all = { zh: { exact: {}, patterns: [] }, en: { exact, patterns } }, missing = new Set();
  for (const { id, tag } of languages.filter(item => !["zh", "en"].includes(item.id))) {
    const app = await appCatalog(tag), messages = JSON.parse(await readFile(join(websiteRoot, "src/locales", id + ".json"), "utf8"));
    const dictionary = { exact: {}, patterns: [], plurals: {} };
    const pick = (from, english, patterned) => {
      if (languages.some(item => item.name === from)) return from;
      const mapped = mapping[from];
      const base = mapped && (Object.hasOwn(source, mapped) ? mapped : mapped.replace(/\.(one|few|many|other)$/, ""));
      let value = base && app[base];
      if (value && patterned && isPlural(value)) { dictionary.plurals[from] = value; return value.other; }
      if (typeof value === "string") return value;
      const plain = from.replace(/^[a-zA-Z][\w-]*:/, "");
      for (const candidate of templates) {
        const found = candidate.regex.exec(plain);
        if (!found) continue;
        const args = Object.fromEntries(candidate.names.map((name,index)=>[name,found[index+1]]));
        if (Object.values(args).some(value => /[\u3400-\u9fff]/.test(value))) continue;
        value = app[candidate.key];
        if (isPlural(value)) value = value[new Intl.PluralRules(tag).select(Number(args.count))] ?? value.other;
        if (typeof value === "string") return fill(value,args);
      }
      if (Object.hasOwn(messages, english)) return messages[english];
      if (!/[\u3400-\u9fff]/.test(from)) return english;
      missing.add(english);
      return english;
    };
    for (const [from, english] of Object.entries(exact)) dictionary.exact[from] = pick(from, english, false);
    for (const [from, english] of patterns) dictionary.patterns.push([from, pick(from, english, true)]);
    all[id] = dictionary;
  }
  if (strict && missing.size) throw Error("Missing website demo translations: " + [...missing].slice(0,10).join(" | "));
  await putIfChanged(contentDir, "replica.json", JSON.stringify(all));
  return { missing: [...missing], dictionaries: all };
}
