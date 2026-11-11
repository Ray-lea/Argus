// Changelog entries (changelog/<locale>.md, written by scripts/release.mjs)
// as release.mjs reads them back to publish a release's notes. Erasable
// TypeScript, so that Node runs it directly.

/** One changelog entry: its version, `YYYY-MM-DD` date and Markdown body. */
export interface Release {
  version: string;
  date: string;
  /** In the changelog's own language, Simplified Chinese. */
  notes: string;
  /** The body in other languages, by locale (`en-US`: changelog/en-US.md). */
  translations?: Record<string, string>;
}

// ---- Changelog ---------------------------------------------------------------

/** An entry heading as release.mjs writes it: `## [1.2.0](link) - 2026-10-12`, or without the link. */
const ENTRY = /^## \[?([^\]\s(]+)\]?(?:\([^)]*\))? - (\d{4}-\d{2}-\d{2})[ \t]*$/gm;
const HEADING = new RegExp(ENTRY.source);
/** Any second-level heading. */
const LEVEL_2 = /^ {0,3}##(?:[ \t]|$)/;

/** Every entry of a changelog, in file order (newest first). */
export function changelogEntries(markdown: string): Release[] {
  const text = markdown.replace(/\r\n/g, "\n");
  const headings = [...text.matchAll(ENTRY)];
  return headings.map((m, i) => ({
    version: m[1],
    date: m[2],
    notes: text.slice((m.index ?? 0) + m[0].length, headings[i + 1]?.index ?? text.length).trim(),
  }));
}

/**
 * `entries` with the body of the same version in `translated` (another
 * language's changelog, e.g. changelog/en-US.md) as their `locale` translation.
 */
export function withTranslation(entries: Release[], locale: string, translated: Release[]): Release[] {
  return entries.map((entry) => {
    const notes = translated.find((other) => other.version === entry.version)?.notes;
    return notes ? { ...entry, translations: { ...entry.translations, [locale]: notes } } : entry;
  });
}

/**
 * Second-level headings that are not entry headings, with their 1-based line
 * numbers. changelogEntries reads them as part of the entry above, so a
 * reworded entry heading silently merges two entries; release.mjs refuses them.
 */
export function strayHeadings(markdown: string): { line: number; text: string }[] {
  return markdown
    .replace(/\r\n/g, "\n")
    .split("\n")
    .flatMap((text, i) => (LEVEL_2.test(text) && !HEADING.test(text) ? [{ line: i + 1, text }] : []));
}

// ---- Translations ------------------------------------------------------------

/**
 * The shape of an entry's body, which a translation keeps: the number of list
 * items before the first `###` section and in each section after it.
 */
export function outline(notes: string): number[] {
  const counts = [0];
  for (const line of notes.replace(/\r\n/g, "\n").split("\n")) {
    if (/^###[ \t]/.test(line)) counts.push(0);
    else if (/^[-*][ \t]/.test(line)) counts[counts.length - 1]++;
  }
  return counts;
}

/** Han characters and the full-width punctuation around them. */
const HAN = /[\u3000-\u303f\u3400-\u4dbf\u4e00-\u9fff\uf900-\ufaff\uff00-\uffef]/;
/**
 * Simplified forms that neither Traditional Chinese nor Japanese writes: what
 * is left of the Simplified Chinese source in a translation into either.
 */
const SIMPLIFIED =
  /[这们个为发进时间说对动过还关开门问题应处实现设备务网页显视图计录统软储连钮错误级标签项单击键报杂变从选择释盘络线启闭载读测试产换么样种历览优复节类别简认识资态证书长东头边运该无维护补丢压宽帧绘获]/;
/** The languages written with Han characters besides the source. */
const HAN_LOCALES = ["zh-TW", "ja-JP"];

/**
 * The lines of a translation into `locale` that still hold source text: any
 * Han character in a language written without them, Simplified forms in one
 * written with them. Link targets do not count: the anchor of a page in
 * Chinese is Chinese.
 */
export function untranslatedLines(locale: string, notes: string): string[] {
  const source = HAN_LOCALES.includes(locale) ? SIMPLIFIED : HAN;
  return notes
    .replace(/\r\n/g, "\n")
    .split("\n")
    .filter((line) => source.test(line.replace(/\]\([^)]*\)/g, "]")));
}
