// Routes and native names are shared by metadata, navigation and build checks.
export const languages = [
  { id: "zh", tag: "zh-CN", name: "简体中文", readme: "README.md", requirements: "系统要求" },
  { id: "zh-TW", tag: "zh-TW", name: "繁體中文", readme: "README.zh-TW.md", requirements: "系統需求" },
  { id: "en", tag: "en-US", name: "English", readme: "README.en.md", requirements: "System requirements" },
  { id: "es", tag: "es-ES", name: "Español", readme: "README.es.md", requirements: "Requisitos del sistema" },
  { id: "de", tag: "de-DE", name: "Deutsch", readme: "README.de.md", requirements: "Systemanforderungen" },
  { id: "ja", tag: "ja-JP", name: "日本語", readme: "README.ja.md", requirements: "動作環境" },
  { id: "fr", tag: "fr-FR", name: "Français", readme: "README.fr.md", requirements: "Configuration requise" },
  { id: "ru", tag: "ru-RU", name: "Русский", readme: "README.ru.md", requirements: "Системные требования" },
] as const;
export type Language = typeof languages[number]["id"];
export type SourceLanguage = "zh" | "en";
export const languageInfo = (lang: Language) => languages.find(item => item.id === lang)!;
export const isLanguage = (value: string): value is Language => languages.some(item => item.id === value);
export const localized = (lang: Language, path = "/") => (lang === "zh" ? "" : "/" + lang) + path;
export const languageOfPath = (path: string): Language => {
  const segment = path.split("/")[1];
  return isLanguage(segment) ? segment : "zh";
};
// The language shown in place of a page's own release notes and preview image when it has none.
export const notesLanguage = (lang: Language): SourceLanguage => lang === "zh" || lang === "zh-TW" ? "zh" : "en";
export const dateLocale = (lang: Language) => languageInfo(lang).tag;

// The browser provides the display-language preference for the web demo.
export function preferredLanguage(tags: readonly string[]): Language {
  for (const tag of tags) {
    const [primary, ...subtags] = tag.trim().toLowerCase().split(/[-_]/);
    const extension = subtags.findIndex(part => part.length === 1);
    const parts = extension < 0 ? subtags : subtags.slice(0, extension);
    if (primary === "zh") {
      if (parts.includes("hant") || parts.includes("cht")) return "zh-TW";
      if (parts.includes("hans") || parts.includes("chs")) return "zh";
      return parts.some(part => ["tw", "hk", "mo"].includes(part)) ? "zh-TW" : "zh";
    }
    if (isLanguage(primary)) return primary;
  }
  return "en";
}

// The offer of a page in another language is written in that language: the
// visitor it is for may not read the page's own.
export const offers: Record<Language, { text: string; action: string; close: string }> = {
  zh: { text: "本页面也提供简体中文版本。", action: "查看简体中文版", close: "关闭" },
  "zh-TW": { text: "本頁面也提供繁體中文版本。", action: "查看繁體中文版", close: "關閉" },
  en: { text: "This page is also available in English.", action: "View in English", close: "Close" },
  es: { text: "Esta página también está disponible en español.", action: "Ver en español", close: "Cerrar" },
  de: { text: "Diese Seite gibt es auch auf Deutsch.", action: "Auf Deutsch anzeigen", close: "Schließen" },
  ja: { text: "このページは日本語でもご覧いただけます。", action: "日本語で表示", close: "閉じる" },
  fr: { text: "Cette page est également disponible en français.", action: "Afficher en français", close: "Fermer" },
  ru: { text: "Эта страница доступна и на русском языке.", action: "Открыть на русском", close: "Закрыть" },
};

/*
 * A visitor's language is the one they picked last: in the header's menu,
 * from an offer or in the demo's settings. The home page's address opens it
 * (layout.tsx), and the other pages offer it. Until then the browser's
 * languages suggest one; an offer closed is not made again.
 */
export const choiceKeys = { chosen: "argus-language", dismissed: "argus-language-dismissed" } as const;
export function offeredLanguage(page: Language, chosen: string | null, dismissed: string | null, browser: readonly string[]): Language | null {
  if (!(chosen && isLanguage(chosen)) && !browser.length) return null;
  const wanted = chosen && isLanguage(chosen) ? chosen : preferredLanguage(browser);
  return wanted === page || wanted === dismissed ? null : wanted;
}
export function rememberLanguage(lang: Language) {
  try {
    localStorage.setItem(choiceKeys.chosen, lang);
    localStorage.removeItem(choiceKeys.dismissed);
  } catch {
    // Without storage the choice lasts as long as its page.
  }
}
