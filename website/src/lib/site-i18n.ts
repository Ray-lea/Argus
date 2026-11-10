// Server-side catalogs. Only the selected catalog is passed to client islands.
import tw from "../locales/zh-TW.json";
import es from "../locales/es.json";
import de from "../locales/de.json";
import ja from "../locales/ja.json";
import fr from "../locales/fr.json";
import ru from "../locales/ru.json";
import { languages, type Language } from "./languages";
import { translateValue, type Messages } from "./translate";
import { chineseDocsLabels } from "./document-labels";

const catalogs: Partial<Record<Language, Messages>> = { "zh-TW": tw, es, de, ja, fr, ru };
const empty: Messages = {};
export const messagesFor = (lang: Language): Messages => catalogs[lang] ?? empty;
// Marketing and policy text render on the server; client islands receive only
// the words their taskbar, comparison widgets and document search can display.
const clientKeys = new Set([
  ...Object.keys(chineseDocsLabels).map(key => key.split("(")[0]),
  "Sunny", "Start", "Search", "Task view", "File Explorer", "Browser", "Terminal", "Show hidden icons", "Memory",
  "Task Manager", "Replay at measured speed", "{count} ms sooner", "{count} s sooner",
  "{label}: Argus {argus} ms, Task Manager {native} ms.", "Flows continuously", "Jumps once per update",
  "This display runs at about {count} Hz: the Argus curve moves {count} times a second, Task Manager's once.",
  "Measuring this display's refresh rate…", "Search documentation in Simplified Chinese", "Stable", "Development", "Select", "Open",
]);
export const clientMessagesFor = (lang: Language): Messages => Object.fromEntries(Object.entries(messagesFor(lang)).filter(([key]) => clientKeys.has(key)));
export const localize = <T>(lang: Language, value: T): T => translateValue(messagesFor(lang), value);
export const copyFor = <T extends { zh: unknown; en: unknown }>(lang: Language, base: T): T["en"] =>
  (lang === "zh" ? base.zh : localize(lang, base.en)) as T["en"];
export const withLanguages = <T extends { zh: unknown; en: unknown }>(base: T): Record<Language, T["en"]> =>
  Object.fromEntries(languages.map(({ id }) => [id, copyFor(id, base)])) as Record<Language, T["en"]>;
