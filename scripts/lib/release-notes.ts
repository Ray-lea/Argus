// Changelog entries (CHANGELOG.md, written by scripts/release.mjs) as
// release.mjs reads them back to publish a release's notes. Erasable
// TypeScript, so that Node runs it directly.

/** One changelog entry: its version, `YYYY-MM-DD` date and Markdown body. */
export interface Release {
  version: string;
  date: string;
  /** In the changelog's own language, Simplified Chinese. */
  notes: string;
  /** The body in other languages, by locale (`en-US`: CHANGELOG.en.md). */
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
 * language's changelog, e.g. CHANGELOG.en.md) as their `locale` translation.
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
