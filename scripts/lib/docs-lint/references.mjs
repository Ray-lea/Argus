// References to docs/ from the files of the repository, which `pnpm docs:lint`
// always checks in full: paths such as docs/guide/settings.md#更新 in any text
// (AGENTS.md, the changelog, comments in scripts and workflows), the files'
// addresses on GitHub (https://github.com/Ray-lea/Argus/blob/main/docs/…), and
// relative links of Markdown files that lead into docs/ (../docs/x.md#y).
// The file must exist with that case and the anchor must be on the page.
import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { REPO_ROOT, repoPath } from "./paths.mjs";
import { isReferenceSource } from "./scope.mjs";
import { checkTarget, resolveLink } from "./targets.mjs";

const FRAGMENT = String.raw`[\p{L}\p{M}\p{N}_%-]+`;
const GITHUB = new RegExp(String.raw`https?://github\.com/Ray-lea/Argus/blob/main/(docs/[A-Za-z0-9_./-]*?\.md)(?:\?[^\s#"'<>()[\]]*)?(?:#(${FRAGMENT}))?`, "gu");
// A path that starts with docs/ itself: not ../docs/ or website/content/docs/.
const MENTION = new RegExp(String.raw`(?<![\p{L}\p{N}_./\\-])(docs/(?:[A-Za-z0-9_.-]+/)*[A-Za-z0-9_.-]+\.md)(?:#(${FRAGMENT}))?`, "gu");
// Destinations of Markdown links and images, link definitions, and HTML attributes.
const INLINE = /\]\(\s*(<[^>\n]*>|[^\s()<>]*(?:\([^\s()<>]*\)[^\s()<>]*)*)(?:\s+(?:"[^"\n]*"|'[^'\n]*'|\([^)\n]*\)))?\s*\)/g;
const DEFINITION = /^ {0,3}\[[^\]\n]+\]:[ \t]*(<[^>\n]*>|\S+)/gm;
const ATTRIBUTE = /\b(?:href|src)\s*=\s*(?:"([^"\n]*)"|'([^'\n]*)')/gi;

const blank = (text) => text.replace(/[^\n]/g, " ");

/**
 * Markdown with fenced code, inline code and HTML comments blanked out
 * (lengths and line breaks kept), so that examples of links in code are not
 * taken for links.
 */
export function maskCode(markdown) {
  const lines = markdown.split("\n");
  let fence = null;
  for (let i = 0; i < lines.length; i++) {
    const opening = /^ {0,3}(`{3,}|~{3,})/.exec(lines[i]);
    if (fence) {
      if (opening && opening[1][0] === fence[0] && opening[1].length >= fence.length) fence = null;
      lines[i] = blank(lines[i]);
    } else if (opening) {
      fence = opening[1];
      lines[i] = blank(lines[i]);
    }
  }
  return lines
    .join("\n")
    .replace(/<!--[\s\S]*?-->/g, blank)
    .replace(/(`+)(?!`)[^\n]*?[^`\n]\1(?!`)|(`+)[^`\n]\2(?!`)/g, blank);
}

/**
 * The references to docs/ in the text of the file at repository path `path`:
 * { index, target: { file, fragment }, kind } with `kind` "path", "url" or
 * "link". `links: false` leaves out the relative links of a Markdown file (the
 * argus-links rule checks them when markdownlint lints the file).
 */
export function findReferences(text, { path, root = REPO_ROOT, markdown = /\.md$/i.test(path), links = markdown } = {}) {
  const found = [];
  const covered = [];
  const from = resolve(root, path);
  if (markdown) {
    const masked = maskCode(text);
    const destinations = [];
    for (const match of masked.matchAll(INLINE)) destinations.push({ value: match[1], index: match.index + match[0].indexOf(match[1], 2) });
    for (const match of masked.matchAll(DEFINITION)) destinations.push({ value: match[1], index: match.index + match[0].lastIndexOf(match[1]) });
    for (const match of masked.matchAll(ATTRIBUTE)) {
      const value = match[1] ?? match[2];
      destinations.push({ value, index: match.index + match[0].indexOf(value, match[0].indexOf("=")) });
    }
    for (const { value, index } of destinations) {
      covered.push([index, index + value.length]);
      if (!links) continue;
      const destination = value.replace(/^<(.*)>$/, "$1");
      const target = resolveLink(destination, from, root);
      if (!target) continue;
      const into = repoPath(target.file, root);
      if (into.startsWith("docs/") && /\.md$/i.test(into)) found.push({ index, target, kind: "link" });
    }
  }
  const inside = (index) => covered.some(([start, end]) => index >= start && index < end);
  for (const match of text.matchAll(GITHUB)) found.push({ index: match.index, target: { file: resolve(root, match[1]), fragment: match[2] ?? null }, kind: "url" });
  for (const match of text.matchAll(MENTION)) {
    if (inside(match.index)) continue;
    found.push({ index: match.index, target: { file: resolve(root, match[1]), fragment: match[2] ?? null }, kind: "path" });
  }
  return found.sort((a, b) => a.index - b.index);
}

function position(text, index) {
  let line = 1;
  let start = 0;
  for (let i = text.indexOf("\n"); i !== -1 && i < index; i = text.indexOf("\n", i + 1)) {
    line++;
    start = i + 1;
  }
  return { line, column: index - start + 1 };
}

/**
 * Checks the references to docs/ in `files` (repository paths): { scanned,
 * problems: [{ path, line, column, kind, problem }] } with `problem` from
 * `checkTarget`. Binary files are skipped; `skipLinks` names the Markdown
 * files whose relative links the argus-links rule checks in this run.
 */
export function checkReferences({ files, root = REPO_ROOT, skipLinks = new Set() }) {
  const problems = [];
  let scanned = 0;
  for (const path of files) {
    if (!isReferenceSource(path)) continue;
    let text;
    try {
      const bytes = readFileSync(resolve(root, path));
      if (bytes.subarray(0, 8000).includes(0)) continue;
      text = bytes.toString("utf8");
    } catch {
      continue; // deleted from the working tree
    }
    scanned++;
    const markdown = /\.md$/i.test(path);
    for (const reference of findReferences(text, { path, root, markdown, links: markdown && !skipLinks.has(path) })) {
      const problem = checkTarget(reference.target, root);
      if (problem) problems.push({ path, ...position(text, reference.index), kind: reference.kind, problem });
    }
  }
  return { scanned, problems };
}
