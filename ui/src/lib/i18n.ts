// UI text, looked up by key in locales/<locale>/<namespace>.json. Simplified
// Chinese is the source every other catalog is translated from; its files
// define the keys (`MessageKey`). See docs/development/i18n.md.

import { createSignal, type JSX } from "solid-js";
import actions from "../../../locales/zh-CN/actions.json";
import common from "../../../locales/zh-CN/common.json";
import connections from "../../../locales/zh-CN/connections.json";
import details from "../../../locales/zh-CN/details.json";
import fileLock from "../../../locales/zh-CN/fileLock.json";
import format from "../../../locales/zh-CN/format.json";
import history from "../../../locales/zh-CN/history.json";
import onboarding from "../../../locales/zh-CN/onboarding.json";
import performance from "../../../locales/zh-CN/performance.json";
import processes from "../../../locales/zh-CN/processes.json";
import services from "../../../locales/zh-CN/services.json";
import settings from "../../../locales/zh-CN/settings.json";
import shell from "../../../locales/zh-CN/shell.json";
import startup from "../../../locales/zh-CN/startup.json";
import updates from "../../../locales/zh-CN/updates.json";

const source = {
  actions,
  common,
  connections,
  details,
  fileLock,
  format,
  history,
  onboarding,
  performance,
  processes,
  services,
  settings,
  shell,
  startup,
  updates,
};

type Leaves<T, P extends string = ""> = {
  [K in keyof T & string]: T[K] extends string ? `${P}${K}` : Leaves<T[K], `${P}${K}.`>;
}[keyof T & string];

/** Every key of the source catalogs, e.g. `"settings.update.title"`. */
export type MessageKey = Leaves<typeof source>;

export type Locale = "zh-CN" | "en-US";
/** What the language setting stores: a locale, or the Windows display language. */
export type Language = "system" | Locale;

/** Each language named in itself, as language pickers do. */
export const LOCALES: { locale: Locale; name: string }[] = [
  { locale: "zh-CN", name: "简体中文" }, // i18n-ignore
  { locale: "en-US", name: "English" },
];

type Catalog = { [key: string]: Catalog | string };

// Namespaces of the other locales, by locale. The native catalog belongs to the Rust side.
const catalogs: Record<string, Catalog> = { "zh-CN": source as Catalog };
const files = import.meta.glob<Catalog>(["../../../locales/*/*.json", "!../../../locales/*/native.json"], {
  eager: true,
  import: "default",
});
for (const [path, catalog] of Object.entries(files)) {
  const [, locale, namespace] = path.match(/locales\/([^/]+)\/([^/]+)\.json$/) ?? [];
  if (!locale || locale === "zh-CN") continue;
  catalogs[locale] ??= {};
  catalogs[locale][namespace] = catalog;
}

const isLocale = (value: string): value is Locale => LOCALES.some((l) => l.locale === value);

/** The locale for a BCP 47 tag: Chinese in any script reads Simplified Chinese, the rest English. */
export function localeFor(tag: string): Locale {
  if (isLocale(tag)) return tag;
  return tag.toLowerCase().startsWith("zh") ? "zh-CN" : "en-US";
}

const [current, setCurrent] = createSignal<Locale>("zh-CN");

/** The UI's locale; reading it makes a computation follow language changes. */
export const locale = current;

export function setLocale(next: Locale) {
  setCurrent(next);
  document.documentElement.lang = next;
}

/** Caches a per-locale object (formatters, collators) and follows locale changes. */
export function localized<T>(make: (locale: Locale) => T): () => T {
  let cached: { locale: Locale; value: T } | undefined;
  return () => {
    const l = current();
    if (cached?.locale !== l) cached = { locale: l, value: make(l) };
    return cached.value;
  };
}

const pluralRules = localized((l) => new Intl.PluralRules(l));
const counts = localized((l) => new Intl.NumberFormat(l));

function lookup(l: string, key: string): Catalog | string | undefined {
  let node: Catalog | string | undefined = catalogs[l];
  for (const part of key.split(".")) {
    if (node === undefined || typeof node === "string") return undefined;
    node = node[part];
  }
  return node;
}

/**
 * The template for `key`. A locale can split a message by plural category
 * (`{"one": "…", "other": "…"}`), chosen by the `count` parameter.
 */
function template(key: string, count: unknown): string {
  const l = current();
  const value = lookup(l, key) ?? lookup("zh-CN", key);
  if (typeof value === "string") return value;
  if (value && typeof count === "number") {
    const plural = value[pluralRules().select(count)] ?? value.other;
    if (typeof plural === "string") return plural;
  }
  return key;
}

export type Params = Record<string, string | number>;

const show = (name: string, value: string | number) =>
  // Counts read as quantities (1,234); other numbers (PIDs, ports) as they are.
  name === "count" && typeof value === "number" ? counts().format(value) : String(value);

/**
 * The text for `key` in the current locale, with each `{name}` replaced from
 * `params`. Reactive: call it where the text is rendered (in JSX or a getter),
 * not once at module load.
 */
export function t(key: MessageKey, params?: Params): string {
  const text = template(key, params?.count);
  if (!params) return text;
  return text.replace(/\{(\w+)\}/g, (match, name: string) =>
    name in params ? show(name, params[name]) : match,
  );
}

export type RichParam = string | number | JSX.Element | ((children: string) => JSX.Element);

/**
 * Like `t`, for text with markup: `<b>…</b>` in the message is rendered by
 * `params.b(children)`, and `{name}` placeholders may be elements. Tags do not nest.
 */
export function rich(key: MessageKey, params: Record<string, RichParam>): JSX.Element {
  const text = template(key, params.count);
  const nodes: JSX.Element[] = [];
  let last = 0;
  for (const m of text.matchAll(/<(\w+)>(.*?)<\/\1>|\{(\w+)\}/g)) {
    if (m.index > last) nodes.push(text.slice(last, m.index));
    last = m.index + m[0].length;
    const [, tag, inner, name] = m;
    const value = params[tag ?? name];
    if (tag) nodes.push(typeof value === "function" ? value(inner) : inner);
    else if (value === undefined) nodes.push(m[0]);
    else nodes.push(typeof value === "number" ? show(name, value) : (value as JSX.Element));
  }
  if (last < text.length) nodes.push(text.slice(last));
  return nodes;
}

/** A list joined the locale's way ("A、B 和 C", "A, B, and C"). */
const lists = localized((l) => new Intl.ListFormat(l, { type: "conjunction" }));
export const fmtList = (items: string[]) => lists().format(items);
