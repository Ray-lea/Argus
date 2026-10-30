// Changelog entries (CHANGELOG.md, written by scripts/release.mjs) as the
// update dialog shows them. Independent of the UI runtime: scripts/tests runs it, and
// scripts/release.mjs reads CHANGELOG.md with it, so a release publishes the
// entries the app finds.

/** What a changelog heading groups. */
export type Category = "breaking" | "feat" | "fix" | "perf" | "revert" | "known" | "other";

/** A run of text; changelog entries only bold text and quote code. */
export interface Span {
  text: string;
  strong?: boolean;
  code?: boolean;
}

export type Block =
  | { kind: "text"; spans: Span[] }
  | { kind: "group"; category: Category; title: string; items: Span[][] };

/** One changelog entry: its version, `YYYY-MM-DD` date and Markdown body. */
export interface Release {
  version: string;
  date: string;
  /** In the changelog's own language, Simplified Chinese. */
  notes: string;
  /** The body in other languages, by locale (`en-US`: CHANGELOG.en.md). */
  translations?: Record<string, string>;
}

// The headings release.mjs writes (SECTIONS) and the ones written by hand.
const HEADINGS: [Category, string[]][] = [
  ["breaking", ["破坏性变更", "breaking changes"]], // i18n-ignore
  ["feat", ["新功能", "功能", "features"]], // i18n-ignore
  ["fix", ["修复", "fixes", "bug fixes"]], // i18n-ignore
  ["perf", ["性能", "performance"]], // i18n-ignore
  ["revert", ["回退", "reverts"]], // i18n-ignore
  ["known", ["已知限制", "已知问题", "known issues"]], // i18n-ignore
  ["other", ["其他", "other"]], // i18n-ignore
];

export function categoryOf(title: string): Category {
  const key = title.trim().toLowerCase();
  return HEADINGS.find(([, names]) => names.includes(key))?.[0] ?? "other";
}

// ---- Versions ----------------------------------------------------------------

interface Parsed {
  core: number[];
  pre: string[];
}

function parseVersion(text: string): Parsed | null {
  const m = /^v?(\d+)\.(\d+)\.(\d+)(?:-([0-9A-Za-z.-]+))?(?:\+[0-9A-Za-z.-]+)?$/.exec(text.trim());
  return m ? { core: [Number(m[1]), Number(m[2]), Number(m[3])], pre: m[4] ? m[4].split(".") : [] } : null;
}

/** SemVer precedence; versions that do not parse compare as equal. */
export function compareVersions(a: string, b: string): number {
  const x = parseVersion(a);
  const y = parseVersion(b);
  if (!x || !y) return 0;
  for (let i = 0; i < 3; i++) if (x.core[i] !== y.core[i]) return x.core[i] - y.core[i];
  // A release ranks above its prereleases.
  if (!x.pre.length || !y.pre.length) return y.pre.length - x.pre.length;
  for (let i = 0; i < Math.max(x.pre.length, y.pre.length); i++) {
    const p = x.pre[i];
    const q = y.pre[i];
    if (p === q) continue;
    if (p === undefined) return -1;
    if (q === undefined) return 1;
    const np = /^\d+$/.test(p);
    const nq = /^\d+$/.test(q);
    if (np && nq) return Number(p) - Number(q);
    if (np !== nq) return np ? -1 : 1;
    return p < q ? -1 : 1;
  }
  return 0;
}

export const isPrerelease = (version: string) => (parseVersion(version)?.pre.length ?? 0) > 0;

/**
 * The entries someone moving from `from` to `to` gets, newest first: those
 * after `from`, up to and including `to`. Without `from`, just `to`'s.
 */
export function between(releases: Release[], from: string | null, to: string): Release[] {
  return releases
    .filter((r) =>
      from === null
        ? compareVersions(r.version, to) === 0
        : compareVersions(r.version, from) > 0 && compareVersions(r.version, to) <= 0,
    )
    .sort((a, b) => compareVersions(b.version, a.version));
}

/** What a start makes of the version whose first start was dealt with last. */
export type FirstStart = { kind: "seen" } | { kind: "record" } | { kind: "updated"; from: string | null };

/**
 * `seen` is the version recorded last (empty when none was): `current` again
 * is `seen`; a newer one is `updated`, from it. Without a record, an update
 * shows by the onboarding done before it or the installer's relaunch; else
 * (a fresh install) and after a downgrade there is nothing to show, only the
 * version to `record`.
 */
export function firstStart(
  current: string,
  seen: string,
  onboarded: boolean,
  relaunched: boolean,
): FirstStart {
  if (seen === current) return { kind: "seen" };
  if (!seen) return onboarded || relaunched ? { kind: "updated", from: null } : { kind: "record" };
  return compareVersions(current, seen) > 0 ? { kind: "updated", from: seen } : { kind: "record" };
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

const language = (locale: string) => locale.split("-")[0].toLowerCase();

/**
 * `release` as a reader of `locale` gets it: the translation into their
 * language, else the body as written, which is `untranslated` for anyone not
 * reading Chinese.
 */
export function readable(release: Release, locale: string): Release & { untranslated: boolean } {
  const lang = language(locale);
  if (lang === "zh") return { ...release, untranslated: false };
  const translations = Object.entries(release.translations ?? {});
  const notes = (translations.find(([key]) => key.toLowerCase() === locale.toLowerCase()) ??
    translations.find(([key]) => language(key) === lang))?.[1];
  return notes ? { ...release, notes, untranslated: false } : { ...release, untranslated: true };
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

// ---- Entries -----------------------------------------------------------------

/**
 * Generated items end with their commit, `([abc1234](…))` or `(abc1234)`, and
 * start with the commit's scope, `**core:**`: neither means anything to users.
 * A hand-written lead such as `**进程**：` stays.
 */
function cleanItem(text: string): string {
  return text
    .replace(/\s*\(\[[0-9a-f]{7,40}\]\([^)]*\)\)\s*$/, "")
    .replace(/\s*\([0-9a-f]{7,40}\)\s*$/, "")
    .replace(/^\*\*[a-z0-9][a-z0-9,-]*:\*\*\s+/, "");
}

const INLINE = /\*\*([^*]+)\*\*|`([^`]+)`|\[([^\]]+)\]\([^)]*\)/g;

/** Text with bold runs and code; links keep only their text. */
export function spans(text: string): Span[] {
  const out: Span[] = [];
  const plain = (s: string) => {
    if (!s) return;
    const prev = out[out.length - 1];
    if (prev && !prev.strong && !prev.code) prev.text += s;
    else out.push({ text: s });
  };
  let last = 0;
  for (const m of text.matchAll(INLINE)) {
    const at = m.index ?? 0;
    plain(text.slice(last, at));
    if (m[1] !== undefined) out.push({ text: m[1], strong: true });
    else if (m[2] !== undefined) out.push({ text: m[2], code: true });
    else plain(m[3]);
    last = at + m[0].length;
  }
  plain(text.slice(last));
  return out;
}

/** An entry's Markdown as paragraphs and headed lists. */
export function parseNotes(markdown: string): Block[] {
  const blocks: Block[] = [];
  let group: Extract<Block, { kind: "group" }> | null = null;
  let paragraph: string[] = [];
  let item: string[] | null = null;

  const endParagraph = () => {
    if (paragraph.length) blocks.push({ kind: "text", spans: spans(paragraph.join(" ")) });
    paragraph = [];
  };
  const endItem = () => {
    if (!item) return;
    if (!group) {
      group = { kind: "group", category: "other", title: "", items: [] };
      blocks.push(group);
    }
    group.items.push(spans(cleanItem(item.join(" "))));
    item = null;
  };

  for (const raw of markdown.replace(/\r\n/g, "\n").split("\n")) {
    const line = raw.trim();
    const heading = /^#{2,6}\s+(.+)$/.exec(line);
    const bullet = /^[-*+]\s+(.+)$/.exec(line);
    if (heading) {
      endItem();
      endParagraph();
      group = { kind: "group", category: categoryOf(heading[1]), title: heading[1].trim(), items: [] };
      blocks.push(group);
    } else if (bullet) {
      endItem();
      endParagraph();
      item = [bullet[1]];
    } else if (!line) {
      endItem();
      endParagraph();
    } else if (item && /^\s/.test(raw)) {
      // An indented line continues the item above.
      item.push(line);
    } else {
      endItem();
      paragraph.push(line);
    }
  }
  endItem();
  endParagraph();
  // A heading with nothing under it says nothing.
  return blocks.filter((b) => b.kind === "text" || b.items.length > 0);
}

/**
 * Groups whose items are changes: every group but known limitations. A
 * breaking change written by hand may be new behaviour listed nowhere else;
 * release.mjs lists a `feat!` or `fix!` commit under both headings, and it
 * counts twice. `other` is commits that predate the convention, lists
 * without a heading and headings written by hand.
 */
const CHANGES: ReadonlySet<Category> = new Set(["breaking", "feat", "fix", "perf", "revert", "other"]);

/** How many changes the blocks list. */
export const changeCount = (blocks: Block[]) =>
  blocks.reduce((n, b) => n + (b.kind === "group" && CHANGES.has(b.category) ? b.items.length : 0), 0);
