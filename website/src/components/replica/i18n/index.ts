// The demo is written in Chinese, as the app's source text is. On the English
// home page the same components render in English: every text child and text
// attribute of an HTML element passes through translate() as the element is
// created (./jsx-runtime.ts), and Chinese text is looked up in ./en.ts — exact
// strings first, then patterns for messages that carry names and numbers.
// Only the English page loads ./en.ts (EnglishReplica.tsx hands it over).
export type Language = "zh" | "en";
export interface Dictionary {
  exact: Record<string, string>;
  patterns: [string, string][];
}

// One language per document: the server sets it as it renders a page's demo,
// the browser reads it from the page.
let rendering: Language = "zh";
export function setLanguage(lang: Language) {
  rendering = lang;
}
export const language = (): Language =>
  typeof document === "undefined" ? rendering : document.documentElement.lang.startsWith("en") ? "en" : "zh";

const chinese = /[　-〿一-鿿＀-￯“”‘’]/;
let exact: Record<string, string> = {};
let compiled: { re: RegExp; names: string[]; to: string }[] = [];
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
    return { re: new RegExp("^" + source + "$"), names, to };
  });
  memo.clear();
}
const missing = new Set<string>();

// A word that reads differently in one place (进程 is the page "Processes" in
// the navigation, a "Process" column elsewhere) is looked up as context:text
// first; an element names its context with data-i18n.
export function translate(text: string, context = ""): string {
  if (language() !== "en" || !chinese.test(text)) return text;
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
        out = lead + p.to.replace(/\{(\w+)\}/g, (_, name: string) => values[name] ?? "") + tail;
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
  if (memo.size < 5000) memo.set(key, out);
  return out;
}

// Text attributes of HTML elements that people read; data-text carries a
// label for CSS to reserve its bold width.
const attributes = ["title", "aria-label", "placeholder", "alt", "aria-valuetext", "aria-description", "aria-roledescription", "data-text"];
export function translateProps<P>(type: unknown, props: P): P {
  if (typeof type !== "string" || language() !== "en" || !props || typeof props !== "object") return props;
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
