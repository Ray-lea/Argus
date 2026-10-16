// Changelog entries (CHANGELOG.md, written by scripts/release.mjs) as the
// update dialog shows them. No Solid and no i18n, so ui/tests can run it.

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
  notes: string;
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

// ---- Changelog ---------------------------------------------------------------

/** Same headings as readEntries in scripts/release.mjs: `## [1.2.0](link) - 2026-10-12`. */
const ENTRY = /^## \[?([^\]\s(]+)\]?(?:\([^)]*\))? - (\d{4}-\d{2}-\d{2})[ \t]*$/gm;

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

/** How many changes the blocks list. */
export const itemCount = (blocks: Block[]) =>
  blocks.reduce((n, b) => n + (b.kind === "group" ? b.items.length : 0), 0);
