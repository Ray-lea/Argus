import assert from "node:assert/strict";
import { existsSync, readFileSync } from "node:fs";
import test from "node:test";
import { demoPath, demos, featureDemo, languages, pickStrings, themes } from "../lib/feature-demos.mjs";

const palette = JSON.parse(readFileSync("resources/theme.json", "utf8"));
const strings = (locale) => pickStrings((file) => JSON.parse(readFileSync(`locales/${locale}/${file}.json`, "utf8")));
// Text widths without a browser: wide enough that long labels must be cut.
const measure = (text, font) => [...text].length * Number(/(\d+(?:\.\d+)?)px/.exec(font)[1]) * 0.62;

test("every demo draws in every README language from the locale strings, within GitHub's SVG policy", () => {
  for (const { lang, locale } of languages) {
    const s = strings(locale);
    for (const id of demos)
      for (const theme of themes) {
        const svg = featureDemo({ id, lang, theme, palette, strings: s, measure });
        const where = `${id} ${lang} ${theme}`;
        assert.ok(svg.startsWith('<svg xmlns="http://www.w3.org/2000/svg"'), where);
        assert.doesNotMatch(svg, /<script|<foreignObject|javascript:|<image|filter=/i, where);
        for (const [, value] of svg.matchAll(/url\(([^)]*)\)/g)) assert.match(value, /^#/, where);
        // No unfilled placeholder, missing value or broken number in the text.
        assert.doesNotMatch(svg, />[^<]*\{\w+\}[^<]*</, where);
        assert.doesNotMatch(svg, /undefined|NaN/, where);
        assert.match(svg, /@media \(prefers-reduced-motion:reduce\)/, where);
        // Figures follow the region's decimal mark.
        if (["de", "es", "fr", "ru"].includes(lang) && id === "performance") assert.match(svg, />41%  3,94 GHz</, where);
      }
  }
});

test("each README shows the demos of its own language, and the files exist", () => {
  const files = { "zh-CN": "README.md", "zh-TW": "README.zh-TW.md", en: "README.en.md", es: "README.es.md", de: "README.de.md", ja: "README.ja.md", fr: "README.fr.md", ru: "README.ru.md" };
  for (const { lang } of languages) {
    const text = readFileSync(files[lang], "utf8");
    for (const id of demos)
      for (const theme of themes) {
        assert.ok(text.includes(demoPath(id, lang, theme)), `${files[lang]} lacks ${demoPath(id, lang, theme)}`);
        assert.ok(existsSync(demoPath(id, lang, theme)), `${demoPath(id, lang, theme)} is missing: run node scripts/readme-features.mjs`);
      }
  }
});
