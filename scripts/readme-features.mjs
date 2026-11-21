// Feature demos for the READMEs: writes every demo in every README language
// and theme to docs/images/features/ and removes files no demo uses any more.
// The drawing runs in a headless Chrome or Edge, which measures the text with
// the fonts the demos name; the interface text comes from locales/.
//   node scripts/readme-features.mjs
import { readFileSync } from "node:fs";
import { mkdir, readdir, readFile, rm, writeFile } from "node:fs/promises";
import { join, resolve } from "node:path";
import { pathToFileURL } from "node:url";
import { demoPath, demos, languages, pickStrings, themes } from "./lib/feature-demos.mjs";

const root = resolve(import.meta.dirname, "..");
const palette = JSON.parse(await readFile(join(root, "resources/theme.json"), "utf8"));

const out = join(root, "docs/images/features");
await mkdir(out, { recursive: true });
const { Browser } = await import(pathToFileURL(join(root, "website/scripts/browser.mjs")).href);
const browser = await Browser.launch();
const written = new Set();
let bytes = 0;
try {
  const page = await browser.newPage({ width: 400, height: 300, scheme: "light" });
  const module = "data:text/javascript;base64," + (await readFile(join(root, "scripts/lib/feature-demos.mjs"))).toString("base64");
  await page.eval(`import(${JSON.stringify(module)}).then((demos) => {
    window.demos = demos;
    // The demos draw figures tabular, which a canvas cannot measure: text
    // with figures is measured as SVG text in the demos' style.
    const context = document.createElement("canvas").getContext("2d");
    const svg = document.createElementNS("http://www.w3.org/2000/svg", "svg");
    const probe = document.createElementNS("http://www.w3.org/2000/svg", "text");
    svg.append(probe);
    document.body.append(svg);
    window.measure = (text, font) => {
      if (!/[0-9]/.test(text)) {
        context.font = font;
        return context.measureText(text).width;
      }
      probe.style.font = font;
      probe.style.fontVariantNumeric = "tabular-nums";
      probe.textContent = text;
      return probe.getComputedTextLength();
    };
    return true;
  })`);
  for (const { lang, locale } of languages) {
    const strings = pickStrings((file) => JSON.parse(readFileSync(join(root, "locales", locale, `${file}.json`), "utf8")));
    for (const id of demos)
      for (const theme of themes) {
        const svg = await page.eval((o) => window.demos.featureDemo({ ...o, measure: window.measure }), { id, lang, theme, palette, strings });
        const file = demoPath(id, lang, theme);
        await writeFile(join(root, file), svg);
        written.add(file.split("/").pop());
        bytes += Buffer.byteLength(svg);
      }
  }
} finally {
  await browser.close();
}
for (const file of await readdir(out)) if (!written.has(file)) await rm(join(out, file));
console.log(`readme-features: ${written.size} demos, ${(bytes / 1024).toFixed(0)} KB`);
