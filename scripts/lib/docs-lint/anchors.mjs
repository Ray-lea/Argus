// The anchors of a Markdown page as GitHub makes them, and as the website
// does with the same github-slugger: headings by their visible text, made
// unique with -1, -2 in page order, and the names of <a id> and <a name>.
// Headings are found line by line, outside fenced code.
import { readFileSync, statSync } from "node:fs";
import GithubSlugger from "github-slugger";

const ENTITIES = { amp: "&", lt: "<", gt: ">", quot: '"', apos: "'", nbsp: " " };

function decodeEntities(text) {
  return text.replace(/&(#x[0-9a-f]+|#\d+|[a-z]+);/gi, (whole, name) => {
    if (name[0] === "#") {
      const code = name[1] === "x" || name[1] === "X" ? Number.parseInt(name.slice(2), 16) : Number.parseInt(name.slice(1), 10);
      return Number.isFinite(code) && code <= 0x10ffff ? String.fromCodePoint(code) : whole;
    }
    return ENTITIES[name.toLowerCase()] ?? whole;
  });
}

/** The text GitHub shows for the Markdown of a heading. */
export function headingText(markdown) {
  const code = [];
  let text = markdown.replace(/(`+)([^`]|[^`][\s\S]*?[^`])\1(?!`)/g, (whole, ticks, inner) => {
    code.push(/^ [\s\S]* $/.test(inner) && inner.trim() ? inner.slice(1, -1) : inner);
    return `\u0000${code.length - 1}\u0000`;
  });
  text = text
    .replace(/!\[[^\]]*\]\([^)]*\)/g, "")
    .replace(/!\[[^\]]*\]\[[^\]]*\]/g, "")
    .replace(/\[([^\]]*)\]\([^)]*\)/g, "$1")
    .replace(/\[([^\]]*)\]\[[^\]]*\]/g, "$1")
    .replace(/<[^>]*>/g, "")
    .replace(/\\([!-/:-@[-`{-~])/g, "$1");
  text = decodeEntities(text).replace(/\u0000(\d+)\u0000/g, (whole, index) => code[Number(index)]);
  return text.trim();
}

const ATX = /^ {0,3}(#{1,6})(?:[ \t]+(.*?))?(?:[ \t]+#+)?[ \t]*$/;
const FENCE = /^ {0,3}(`{3,}|~{3,})/;
const SETEXT = /^ {0,3}(=+|-+)[ \t]*$/;
const NAMED = /<a\s[^>]*?\b(?:id|name)\s*=\s*(?:"([^"]*)"|'([^']*)'|([^\s"'>]+))/gi;

/** The headings of a page: { level, text, anchor, line } in order. */
export function headings(markdown) {
  const lines = markdown.split(/\r?\n/);
  const slugger = new GithubSlugger();
  const list = [];
  let fence = null;
  for (let i = 0; i < lines.length; i++) {
    const line = lines[i];
    const opening = FENCE.exec(line);
    if (fence) {
      if (opening && opening[1][0] === fence[0] && opening[1].length >= fence.length && !/\S/.test(line.slice(opening[0].length))) fence = null;
      continue;
    }
    if (opening) {
      fence = opening[1];
      continue;
    }
    const atx = ATX.exec(line);
    let level = 0;
    let text = null;
    if (atx) {
      level = atx[1].length;
      text = headingText(atx[2] ?? "");
    } else if (
      i + 1 < lines.length &&
      SETEXT.test(lines[i + 1]) &&
      line.trim() &&
      !/^ {0,3}([-*+]\s|\d+[.)]\s|>|\||<)/.test(line) &&
      (i === 0 || !lines[i - 1].trim() || ATX.test(lines[i - 1]))
    ) {
      level = lines[i + 1].trim()[0] === "=" ? 1 : 2;
      text = headingText(line);
      i++;
    }
    if (text !== null) list.push({ level, text, anchor: slugger.slug(text), line: i + 1 });
  }
  return list;
}

/** Every anchor a link may point to on a page. */
export function anchorsOf(markdown) {
  const anchors = new Set(headings(markdown).map((heading) => heading.anchor));
  let fence = null;
  for (const line of markdown.split(/\r?\n/)) {
    const opening = FENCE.exec(line);
    if (fence) {
      if (opening && opening[1][0] === fence[0] && opening[1].length >= fence.length) fence = null;
      continue;
    }
    if (opening) {
      fence = opening[1];
      continue;
    }
    for (const match of line.matchAll(NAMED)) anchors.add(match[1] ?? match[2] ?? match[3]);
  }
  return anchors;
}

const cache = new Map();

/** The anchors of a page on disk, read again when the file changes. */
export function anchorsOfFile(file) {
  const { mtimeMs, size } = statSync(file);
  const cached = cache.get(file);
  if (cached && cached.mtimeMs === mtimeMs && cached.size === size) return cached.anchors;
  const anchors = anchorsOf(readFileSync(file, "utf8"));
  cache.set(file, { mtimeMs, size, anchors });
  return anchors;
}

/** A fragment as written in a link, decoded the way browsers and GitHub match it. */
export function decodeFragment(fragment) {
  let decoded = fragment;
  try {
    decoded = decodeURIComponent(fragment);
  } catch {
    // Keep a fragment with a stray % as written.
  }
  return decoded.replace(/^user-content-/, "");
}

/** Fragments that point to lines of a file on GitHub (#L10, #L10-L20), not to anchors. */
export const isLineFragment = (fragment) => /^L\d+(?:C\d+)?(?:-L\d+(?:C\d+)?)?$/.test(fragment);
