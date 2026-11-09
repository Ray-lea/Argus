// NSIS reads generated LangStrings; the source text stays with the app catalogs.
import { readFileSync } from "node:fs";
import { join } from "node:path";

export const installerLanguages = {
  "en-US": "ENGLISH", "zh-CN": "SIMPCHINESE", "es-ES": "SPANISH",
  "de-DE": "GERMAN", "ja-JP": "JAPANESE", "fr-FR": "FRENCH", "ru-RU": "RUSSIAN",
};
const variables = { version: "${VERSION}", oldVersion: "$ArgusOldVersion", installDir: "$INSTDIR", product: "${PRODUCTNAME}", code: "$0" };
export function nsisText(text) {
  return text.replaceAll("$", () => "$$").replaceAll('"', '$\\"').replaceAll("\r", "").replaceAll("\n", "$\\r$\\n")
    .replace(/\{(\w+)\}/g, (_, key) => {
      if (!(key in variables)) throw Error(`Unknown installer placeholder: ${key}`);
      return variables[key];
    });
}
export function installerStrings(root) {
  const lines = ["; Generated from locales/*/installer.json by scripts/bundle.mjs."];
  for (const [locale, language] of Object.entries(installerLanguages)) {
    const catalog = JSON.parse(readFileSync(join(root, "locales", locale, "installer.json"), "utf8"));
    for (const [key, value] of Object.entries(catalog)) {
      if (!/^\w+$/.test(key) || typeof value !== "string") throw Error(`Invalid installer string: ${locale}/${key}`);
      lines.push(`LangString ${key} \${LANG_${language}} "${nsisText(value)}"`);
    }
  }
  return lines.join("\n") + "\n";
}
