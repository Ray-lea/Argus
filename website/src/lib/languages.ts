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
