import assert from "node:assert/strict";
import { existsSync, readFileSync } from "node:fs";
import test from "node:test";
import { MARK_END, MARK_START, SCRIPT_SIZE, cardPath, extractPalette, readmeBlock, readmes, supporterCard, themes } from "../lib/supporter-card.mjs";
import { readFont, wordPath } from "../lib/ttf.mjs";

const supporters = JSON.parse(readFileSync("website/src/data/supporters.json", "utf8"));
// Text widths without a browser: wide enough that long names must be cut.
const measure = (text, font) => [...text].length * Number(/(\d+(?:\.\d+)?)px/.exec(font)[1]) * 0.62;

test("supporter cards survive GitHub's SVG policy: no script and nothing loaded from elsewhere", () => {
  for (const { lang } of readmes)
    for (const theme of themes) {
      const { svg } = supporterCard({
        name: "<Ann & Bob> with a nickname long enough to need an ellipsis",
        image: "data:image/jpeg;base64,AA==",
        palette: ["#3366cc", "#cc6677", "#33aacc", "#cc8866"],
        grain: "data:image/png;base64,AA==",
        logo: '<circle r="1"/>',
        lang,
        theme,
        measure,
      });
      assert.ok(svg.startsWith('<svg xmlns="http://www.w3.org/2000/svg"'), lang);
      assert.doesNotMatch(svg, /<script|<foreignObject|javascript:/i);
      // Every link or image is a data: URL or a reference inside the file.
      for (const [, value] of svg.matchAll(/\shref="([^"]*)"/g)) assert.match(value, /^(data:|#)/, `${lang} ${theme}: ${value}`);
      for (const [, value] of svg.matchAll(/url\(([^)]*)\)/g)) assert.match(value, /^#/, `${lang} ${theme}: ${value}`);
      assert.match(svg, /<title>[^<]*&lt;Ann &amp; Bob&gt;[^<]*<\/title>/);
      assert.match(svg, /…<\/text>/);
      assert.match(svg, /@media \(prefers-reduced-motion:reduce\)/);
    }
});

test("each README carries the cards of every supporter in the list, and the files exist", () => {
  for (const { lang, file } of readmes) {
    const text = readFileSync(file, "utf8");
    const start = text.indexOf(MARK_START),
      end = text.indexOf(MARK_END);
    assert.ok(start >= 0 && end > start, `${file} lacks the supporter markers`);
    assert.equal(text.slice(start, end + MARK_END.length), readmeBlock(lang, supporters), `${file} is out of date: run node scripts/supporters.mjs`);
    for (const { id, colors } of supporters) {
      for (const theme of themes) assert.ok(existsSync(cardPath(id, lang, theme)), cardPath(id, lang, theme));
      assert.equal(colors.light.length, 4);
      assert.equal(colors.dark.length, 4);
    }
  }
  for (const { id } of supporters) assert.ok(existsSync(`website/src/data/supporters/${id}.webp`), id);
});

test("the nickname is drawn with the script font's outlines, and set as text when the font lacks a character", () => {
  const font = readFont(readFileSync("scripts/lib/fonts/GreatVibes-Regular.ttf"));
  // Without a browser, each letter starts where the previous one's advance ends.
  const script = (text, size) => {
    const glyphs = [...text].map((c) => font.glyphOf(c.codePointAt(0)));
    if (glyphs.some((g) => !g)) return null;
    let x = 0;
    const offsets = glyphs.map((g) => {
      const at = x;
      x += (font.advance(g) * size) / font.unitsPerEm;
      return at;
    });
    return wordPath(font, glyphs, offsets, size, 0, 0);
  };
  const word = script("bronyaicu", SCRIPT_SIZE);
  assert.match(word.d, /^M[\d.-]+ [\d.-]+(Q|L)/);
  assert.ok(word.top > SCRIPT_SIZE * 0.4 && word.bottom > SCRIPT_SIZE * 0.2, JSON.stringify(word));
  assert.equal(script("山雀", SCRIPT_SIZE), null);
  const card = (name) =>
    supporterCard({ name, image: "data:,", palette: ["#3366cc", "#cc6677", "#33aacc", "#cc8866"], grain: "data:,", logo: "", lang: "en", theme: "dark", measure, script }).svg;
  assert.match(card("bronyaicu"), /<path transform="translate\([\d. -]+\)" d="M/);
  assert.doesNotMatch(card("bronyaicu"), />bronyaicu<\/text>/);
  assert.match(card("山雀"), />山雀<\/text>/);
  // The tail of the y stays clear of the line of thanks under it.
  const svg = card("bronyaicu");
  const baseline = Number(/<path transform="translate\([\d.-]+ ([\d.]+)\)/.exec(svg)[1]);
  const thanksY = Number(/<text x="\d+" y="([\d.]+)" class="t" font-size="15"/.exec(svg)[1]);
  assert.ok(thanksY - 11 - (baseline + word.bottom) >= 12, `${baseline} ${thanksY}`);
});

test("an avatar's colours come from its coloured pixels, not its white or grey ground", () => {
  const pixels = (spans) => spans.flatMap(([rgb, count]) => Array.from({ length: count }, () => [...rgb, 255])).flat();
  // Mostly white and grey, with some sky blue and a little pink.
  const drawing = extractPalette(pixels([[[255, 255, 255], 1400], [[200, 200, 204], 600], [[150, 195, 235], 220], [[240, 180, 190], 60]]));
  assert.equal(drawing.length, 4);
  const hue = (hex) => {
    const [r, g, b] = [1, 3, 5].map((i) => parseInt(hex.slice(i, i + 2), 16));
    return r > b ? "warm" : "cool";
  };
  assert.equal(hue(drawing[0]), "cool");
  assert.equal(hue(drawing[1]), "warm");
  // A grey avatar stays grey.
  const grey = extractPalette(pixels([[[120, 120, 120], 500], [[30, 30, 30], 200]]));
  assert.ok(grey.every((hex) => hex === grey[0] && hex.slice(1, 3) === hex.slice(3, 5)));
});
