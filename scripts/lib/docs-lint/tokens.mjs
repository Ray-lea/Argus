// Helpers over the micromark tokens markdownlint passes to custom rules
// (params.parsers.micromark.tokens): walking the tree, the visible text of a
// part with where each piece starts, counting characters and sentences as
// docs/development/docs-style.md does, and where a rule applies.
import { REPO_ROOT, matchesAny, repoPath } from "./paths.mjs";

export function* walk(tokens) {
  for (const token of tokens) {
    yield token;
    yield* walk(token.children);
  }
}

/** Every token of the given types below `tokens`, in document order. */
export const ofType = (tokens, ...types) => [...walk(tokens)].filter((token) => types.includes(token.type));

export function hasAncestor(token, types) {
  for (let parent = token.parent; parent; parent = parent.parent) if (types.includes(parent.type)) return true;
  return false;
}

export const LISTS = ["listOrdered", "listUnordered"];

// Subtrees that are not visible text: link destinations and titles, reference
// labels, definitions, raw HTML and images (whose alternative text is not shown).
const HIDDEN = new Set(["resource", "reference", "definition", "htmlText", "htmlFlow", "image"]);
// Leaves that carry visible text.
const TEXT = new Set([
  "data",
  "codeTextData",
  "characterEscapeValue",
  "autolinkProtocol",
  "autolinkEmail",
  "literalAutolinkHttp",
  "literalAutolinkWww",
  "literalAutolinkEmail",
]);
/** Stands for inline code when a rule reads the prose only. */
export const CODE = "￼";

/**
 * The visible text below `token` in pieces, each { text, line, column } where
 * it starts. Inline code counts with its text; with `code: false` it is one
 * CODE character, so a rule on prose neither reads it nor joins the words
 * around it. `skip` names more subtrees to leave out.
 */
export function pieces(token, { code = true, skip = [] } = {}) {
  const list = [];
  const visit = (node) => {
    if (HIDDEN.has(node.type) || skip.includes(node.type)) return;
    if (node.type === "codeText" && !code) {
      list.push({ text: CODE, line: node.startLine, column: node.startColumn });
      return;
    }
    if (node.type === "characterReference") {
      list.push({ text: "�", line: node.startLine, column: node.startColumn });
      return;
    }
    if (node.type === "lineEnding") {
      list.push({ text: " ", line: node.startLine, column: node.startColumn });
      return;
    }
    if (!node.children.length) {
      if (TEXT.has(node.type)) list.push({ text: node.text, line: node.startLine, column: node.startColumn });
      return;
    }
    for (const child of node.children) visit(child);
  };
  visit(token);
  return list;
}

export const join = (list) => list.map((piece) => piece.text).join("");

/** The visible text below `token` (see `pieces`). */
export const textOf = (token, options) => join(pieces(token, options));

/** Where the character at `index` of the joined pieces is in the file. */
export function locate(list, index) {
  let offset = 0;
  for (const piece of list) {
    if (index < offset + piece.text.length) return { line: piece.line, column: piece.column + index - offset };
    offset += piece.text.length;
  }
  const last = list.at(-1);
  return last ? { line: last.line, column: last.column + last.text.length } : { line: 1, column: 1 };
}

/**
 * An error range markdownlint accepts: [column, length] cut to the end of the
 * line (markdownlint stops on a range past it), or undefined.
 */
export function rangeOf(params, line, column, size) {
  const available = (params.lines[line - 1]?.length ?? 0) - column + 1;
  return column >= 1 && available >= 1 ? [column, Math.max(1, Math.min(size, available))] : undefined;
}

/** Characters as the style counts them: code points, white space left out. */
export const length = (text) => [...text.replace(/\s+/gu, "")].length;

/**
 * Sentences of `text`, each { text, start }: a sentence ends at 。！？ and
 * the closing quotes or brackets right after it.
 */
export function sentences(text) {
  const list = [];
  let start = 0;
  for (const match of text.matchAll(/[。！？]+[”’）」』】》]*/gu)) {
    const end = match.index + match[0].length;
    list.push({ text: text.slice(start, end), start });
    start = end;
  }
  if (start < text.length) list.push({ text: text.slice(start), start });
  return list.filter((sentence) => length(sentence.text) > 0 && sentence.text.trim() !== CODE);
}

/** The first `chars` characters of `text`, for the context of an error. */
export const excerpt = (text, chars = 24) => {
  const all = [...text.trim()];
  return all.length > chars ? `${all.slice(0, chars).join("")}…` : all.join("");
};

export const isHeading = (token) => token.type === "atxHeading" || token.type === "setextHeading";

export function headingLevel(token) {
  if (token.type === "atxHeading") return token.children.find((child) => child.type === "atxHeadingSequence")?.text.length ?? 0;
  if (token.type === "setextHeading") return token.children.find((child) => child.type === "setextHeadingLine")?.text.trim().startsWith("=") ? 1 : 2;
  return 0;
}

export const headingContent = (token) => token.children.find((child) => child.type === "atxHeadingText" || child.type === "setextHeadingText");

export const isComment = (token) => token.type === "htmlFlow" && /^\s*<!--[\s\S]*-->\s*$/.test(token.text);

/**
 * What kind of block a top-level token is: heading, paragraph, list, table,
 * code, quote (block quotes and alerts), html, break; null for blank lines,
 * HTML comments, link definitions and markdownlint's own bookkeeping.
 */
export function blockKind(token) {
  switch (token.type) {
    case "atxHeading":
    case "setextHeading":
      return "heading";
    case "content":
      return token.children.some((child) => child.type === "paragraph") ? "paragraph" : null;
    case "listOrdered":
    case "listUnordered":
      return "list";
    case "table":
      return "table";
    case "codeFenced":
    case "codeIndented":
    case "mathFlow":
      return "code";
    case "blockQuote":
      return "quote";
    case "htmlFlow":
      return isComment(token) ? null : "html";
    case "thematicBreak":
      return "break";
    default:
      return null;
  }
}

/** The top-level blocks of a document, in order (see `blockKind`). */
export const blocks = (params) => params.parsers.micromark.tokens.filter((token) => blockKind(token));

/** The five alert types of GitHub, in order of severity. */
export const ALERT_TYPES = ["NOTE", "TIP", "IMPORTANT", "WARNING", "CAUTION"];
/** The `[!TYPE]` line an alert starts with, as text. */
export const ALERT_MARK = /^\s*\[!([A-Za-z]+)\]/u;

/** A rule's parameters (markdownlint passes `true` as no object). */
export const settings = (params) => (params.config && typeof params.config === "object" ? params.config : {});

/**
 * Whether a rule applies to the file it is checking: its `include` patterns
 * (repository paths) match the file and `exempt` does not name it. `root`
 * stands in for the repository in tests.
 */
export function applies(params, include = ["docs/**"]) {
  const config = settings(params);
  const path = repoPath(params.name, config.root ?? REPO_ROOT);
  return matchesAny(path, config.include ?? include) && !(config.exempt ?? []).includes(path);
}

/** The repository's (or the test's) root for a rule. */
export const rootOf = (params) => settings(params).root ?? REPO_ROOT;
