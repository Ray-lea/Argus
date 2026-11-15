// Supporter cards: reads website/src/data/supporters.json and the avatars
// beside it, writes one light-field card per supporter, README language and
// theme to docs/images/supporters/, rewrites the block between the supporter
// markers in every README, and stores each avatar's colours in the list for
// the website's card. Runs the drawing in a headless Chrome or Edge, which
// measures the text with the fonts the cards name.
//   node scripts/supporters.mjs
//   node scripts/supporters.mjs --avatar <id> <image>   first stores the image
//                                                     as that supporter's avatar
import { readdir, readFile, rm, writeFile, mkdir } from "node:fs/promises";
import { join, resolve } from "node:path";
import { pathToFileURL } from "node:url";
import { crc32, deflateSync } from "node:zlib";
import { MARK_END, MARK_START, cardPath, extractPalette, readmeBlock, readmes, themes, tones } from "./lib/supporter-card.mjs";

const root = resolve(import.meta.dirname, "..");
const data = join(root, "website/src/data");
const listFile = join(data, "supporters.json");
const supporters = JSON.parse(await readFile(listFile, "utf8"));

// A tile of film grain as a grey + alpha PNG: black and white specks with a
// random, low alpha, so the tile darkens and lightens equally. Seeded, so
// every run writes the same bytes.
function grain(size = 72, strength = 30) {
  let a = 7;
  const rand = () => {
    a = (a + 0x6d2b79f5) >>> 0;
    let t = a;
    t = Math.imul(t ^ (t >>> 15), t | 1);
    t ^= t + Math.imul(t ^ (t >>> 7), t | 61);
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
  };
  const rows = [];
  for (let y = 0; y < size; y++) {
    const row = Buffer.alloc(1 + size * 2);
    for (let x = 0; x < size; x++) {
      row[1 + x * 2] = rand() < 0.5 ? 0 : 255;
      row[2 + x * 2] = Math.round(rand() * strength);
    }
    rows.push(row);
  }
  const chunk = (type, body) => {
    const head = Buffer.alloc(8);
    head.writeUInt32BE(body.length, 0);
    head.write(type, 4, "ascii");
    const crc = Buffer.alloc(4);
    crc.writeUInt32BE(crc32(Buffer.concat([Buffer.from(type, "ascii"), body])), 0);
    return Buffer.concat([head, body, crc]);
  };
  const header = Buffer.alloc(13);
  header.writeUInt32BE(size, 0);
  header.writeUInt32BE(size, 4);
  header[8] = 8;
  header[9] = 4;
  const png = Buffer.concat([Buffer.from([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a]), chunk("IHDR", header), chunk("IDAT", deflateSync(Buffer.concat(rows), { level: 9 })), chunk("IEND", Buffer.alloc(0))]);
  return "data:image/png;base64," + png.toString("base64");
}

// The mark, its ids made unique so they cannot clash with the card's own.
const logo = (await readFile(join(root, "resources/logo.svg"), "utf8"))
  .replace(/^[\s\S]*?<svg[^>]*>/, "")
  .replace(/<title[\s\S]*?<\/title>/, "")
  .replace(/<\/svg>\s*$/, "")
  .replace(/id="(\w+)"/g, 'id="lg-$1"')
  .replace(/url\(#(\w+)\)/g, "url(#lg-$1)")
  .replace(/\s*\n\s*/g, "");

const { Browser } = await import(pathToFileURL(join(root, "website/scripts/browser.mjs")).href);
const browser = await Browser.launch();
const out = join(root, "docs/images/supporters");
try {
  const page = await browser.newPage({ width: 400, height: 300, scheme: "light" });
  const module = async (file) => "data:text/javascript;base64," + (await readFile(join(root, file))).toString("base64");
  const script = (await readFile(join(root, "scripts/lib/fonts/GreatVibes-Regular.ttf"))).toString("base64");
  // The drawing code, text measurement, and the nickname font: its outlines
  // come from the font file, where each letter starts from the browser's own
  // layout of the word in that font (kerning included).
  await page.eval(`Promise.all([import(${JSON.stringify(await module("scripts/lib/supporter-card.mjs"))}), import(${JSON.stringify(await module("scripts/lib/ttf.mjs"))})]).then(async ([cards, ttf]) => {
    window.cards = cards;
    const context = document.createElement("canvas").getContext("2d");
    window.measure = (text, font) => { context.font = font; return context.measureText(text).width; };
    const bytes = Uint8Array.from(atob(${JSON.stringify(script)}), (c) => c.charCodeAt(0));
    const face = new FontFace("Supporter Script", bytes);
    await face.load();
    document.fonts.add(face);
    const font = ttf.readFont(bytes);
    window.script = (text, size) => {
      const chars = [...text];
      const glyphs = chars.map((c) => font.glyphOf(c.codePointAt(0)));
      if (glyphs.some((g, i) => !g && chars[i].trim())) return null;
      context.font = size + "px 'Supporter Script'";
      const offsets = chars.map((c, i) => context.measureText(chars.slice(0, i + 1).join("")).width - context.measureText(c).width);
      return ttf.wordPath(font, glyphs, offsets, size, 0, 0);
    };
    return true;
  })`);
  // A new avatar: any image the browser can decode, centre-cropped to a
  // square and stored as a 256 px WebP (the website shows it at up to 124 px).
  const at = process.argv.indexOf("--avatar");
  if (at >= 0) {
    const [id, file] = process.argv.slice(at + 1, at + 3);
    if (!supporters.some((s) => s.id === id)) throw Error(`${id} is not in website/src/data/supporters.json`);
    const source = "data:application/octet-stream;base64," + (await readFile(file)).toString("base64");
    const webp = await page.eval(async (src) => {
      const img = new Image();
      img.src = src;
      await img.decode();
      const side = Math.min(img.naturalWidth, img.naturalHeight);
      const canvas = document.createElement("canvas");
      canvas.width = canvas.height = 256;
      const g = canvas.getContext("2d");
      g.imageSmoothingQuality = "high";
      g.drawImage(img, (img.naturalWidth - side) / 2, (img.naturalHeight - side) / 2, side, side, 0, 0, 256, 256);
      return canvas.toDataURL("image/webp", 0.9);
    }, source);
    await writeFile(join(data, "supporters", id + ".webp"), Buffer.from(webp.split(",")[1], "base64"));
  }
  await rm(out, { recursive: true, force: true });
  await mkdir(out, { recursive: true });
  const tile = grain();
  for (const s of supporters) {
    const avatar = "data:image/webp;base64," + (await readFile(join(data, "supporters", s.id + ".webp"))).toString("base64");
    // The avatar as the cards embed it (twice its 112 px size), and its
    // pixels at 48 px for the colours.
    const { image, pixels } = await page.eval(async (src) => {
      const img = new Image();
      img.src = src;
      await img.decode();
      const big = document.createElement("canvas");
      big.width = big.height = 224;
      big.getContext("2d").drawImage(img, 0, 0, 224, 224);
      const small = document.createElement("canvas");
      small.width = small.height = 48;
      const g = small.getContext("2d");
      g.drawImage(img, 0, 0, 48, 48);
      return { image: big.toDataURL("image/jpeg", 0.88), pixels: [...g.getImageData(0, 0, 48, 48).data] };
    }, avatar);
    const palette = extractPalette(pixels);
    for (const { lang } of readmes)
      for (const theme of themes) {
        const { svg } = await page.eval(
          (o) => window.cards.supporterCard({ ...o, measure: window.measure, script: window.script }),
          { name: s.name, image, palette, grain: tile, logo, lang, theme },
        );
        await writeFile(join(root, cardPath(s.id, lang, theme)), svg);
      }
    // The website's card takes the same colours, already toned per theme, and
    // the same nickname outline, drawn at 100 px (absent when the script font
    // cannot draw the name: the site then sets it as text).
    s.colors = Object.fromEntries(themes.map((theme) => [theme, tones(palette, theme)]));
    const signature = await page.eval((name) => window.script(name, 100), s.name);
    if (signature) s.signature = signature;
    else delete s.signature;
  }
} finally {
  await browser.close();
}

await writeFile(listFile, JSON.stringify(supporters, null, 2) + "\n");
for (const { lang, file } of readmes) {
  const path = join(root, file);
  const text = await readFile(path, "utf8");
  const start = text.indexOf(MARK_START),
    end = text.indexOf(MARK_END);
  if (start < 0 || end < start) throw Error(file + " has no supporter markers");
  await writeFile(path, text.slice(0, start) + readmeBlock(lang, supporters) + text.slice(end + MARK_END.length));
}
const written = (await readdir(out)).length;
console.log(`supporters: ${supporters.length} supporter(s), ${written} cards, ${readmes.length} READMEs`);
