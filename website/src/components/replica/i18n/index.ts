// Source text is Chinese. The page supplies only its selected dictionary,
// assembled from the application catalogs and the site's own translations.
// JSX text and readable attributes use exact entries, then message patterns.
import { languages, type Language } from "@/lib/languages";
export type { Language } from "@/lib/languages";
export interface Dictionary {
  exact: Record<string, string>;
  patterns: [string, string][];
  plurals?: Record<string, Record<string, string>>;
}

// One language per document: the server sets it as it renders a page's demo,
// the browser reads it from the page.
let rendering: Language = "zh";
export function setLanguage(lang: Language) {
  rendering = lang;
}
export const language = (): Language =>
  typeof document === "undefined" ? rendering : languages.find(item => item.tag === document.documentElement.lang || item.id === document.documentElement.lang)?.id ?? "zh";

const chinese = /[　-〿一-鿿＀-￯“”‘’]/;
const nativeNames = new Set(languages.map(item => item.name as string));
let exact: Record<string, string> = {};
let compiled: { re: RegExp; names: string[]; to: string; plural?: Record<string, string> }[] = [];
const translated = new Set<string>();
const memo = new Map<string, string>();
let current: Dictionary | null = null;
export function registerDictionary(dictionary: Dictionary) {
  if (current === dictionary) return;
  current = dictionary;
  exact = dictionary.exact;
  compiled = dictionary.patterns.map(([from, to]) => {
    const names: string[] = [];
    const source = from
      .split(/(\{\w+\})/)
      .map((part) => {
        const hole = /^\{(\w+)\}$/.exec(part);
        if (!hole) return part.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
        names.push(hole[1]);
        return "(.+?)";
      })
      .join("");
    return { re: new RegExp("^" + source + "$"), names, to, plural: dictionary.plurals?.[from] };
  });
  memo.clear();
  translated.clear();
}
const missing = new Set<string>();

// A word that reads differently in one place (进程 is the page "Processes" in
// the navigation, a "Process" column elsewhere) is looked up as context:text
// first; an element names its context with data-i18n.
export function translate(text: string, context = ""): string {
  if (language() === "zh" || !chinese.test(text) || nativeNames.has(text) || translated.has(text)) return text;
  const key = context + "\u0000" + text;
  const known = memo.get(key);
  if (known !== undefined) return known;
  // Tooltips put the name and the path on lines of their own.
  if (text.includes("\n")) return text.split("\n").map((line) => translate(line, context)).join("\n");
  const lookup = (s: string) => (context && exact[context + ":" + s]) || exact[s];
  let out = lookup(text);
  if (out === undefined) {
    // Surrounding spaces belong to the layout, not the phrase.
    const core = text.trim();
    const lead = text.slice(0, text.indexOf(core)),
      tail = text.slice(text.indexOf(core) + core.length);
    const hit = lookup(core);
    if (hit !== undefined) out = lead + hit + tail;
    else
      for (const p of compiled) {
        const m = p.re.exec(core);
        if (!m) continue;
        const values = Object.fromEntries(p.names.map((name, i) => [name, translate(m[i + 1], context)]));
        const rawCount = values.count;
        const category = rawCount && /^-?\d+(?:\.\d+)?$/.test(rawCount) ? new Intl.PluralRules(languages.find(item => item.id === language())!.tag).select(Number(rawCount)) : "other";
        const template = p.plural?.[category] ?? p.plural?.other ?? p.to;
        out = lead + template.replace(/\{(\w+)\}/g, (_, name: string) => values[name] ?? "") + tail;
        break;
      }
  }
  if (out === undefined) {
    out = text;
    if (!missing.has(text)) {
      missing.add(text);
      (globalThis as { __argusMissing?: Set<string> }).__argusMissing = missing;
    }
  }
  if (memo.size < 5000) { memo.set(key, out); if (out !== text) translated.add(out); }
  return out;
}

// Text attributes of HTML elements that people read; data-text carries a
// label for CSS to reserve its bold width.
const attributes = ["title", "aria-label", "placeholder", "alt", "aria-valuetext", "aria-description", "aria-roledescription", "data-text"];
export function translateProps<P>(type: unknown, props: P): P {
  if (typeof type !== "string" || language() === "zh" || !props || typeof props !== "object") return props;
  const source = props as Record<string, unknown>;
  const context = typeof source["data-i18n"] === "string" ? source["data-i18n"] : "";
  let copy: Record<string, unknown> | null = null;
  const put = (key: string, value: unknown) => {
    copy ??= { ...source };
    copy[key] = value;
  };
  for (const key of attributes) {
    const value = source[key];
    if (typeof value === "string") {
      const next = translate(value, context);
      if (next !== value) put(key, next);
    }
  }
  const children = source.children;
  if (typeof children === "string") {
    const next = translate(children, context);
    if (next !== children) put("children", next);
  } else if (Array.isArray(children) && children.some((c) => typeof c === "string")) {
    put(
      "children",
      children.map((c) => (typeof c === "string" ? translate(c, context) : c)),
    );
  }
  return (copy ?? source) as P;
}
