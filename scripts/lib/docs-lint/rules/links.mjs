// argus-links: relative links and images lead to files that exist, with the
// same case (GitHub is case-sensitive), and their #anchors are on the target
// page, made as GitHub makes them. Pages in docs/ link to the repository's
// files by relative path, not by https://github.com/Ray-lea/Argus/blob/…
// (docs/development/docs-style.md#链接与图片). Fragments of the page itself
// are MD051's. Link text is not one of `prohibited_texts` (这里, 此处…):
// MD059 cannot take Chinese ones, its normalization drops every character
// outside ASCII and so matches any Chinese link text.
import { isAbsolute, resolve } from "node:path";
import { t } from "../../console-language.mjs";
import { matchesAny, repoPath } from "../paths.mjs";
import { checkTarget, describe, resolveLink } from "../targets.mjs";
import { applies, isComment, ofType, rootOf, settings, textOf, walk } from "../tokens.mjs";

const ABSOLUTE = /^https?:\/\/github\.com\/Ray-lea\/Argus\/(?:blob|tree)\//i;
const ATTRIBUTE = /\b(?:href|src)\s*=\s*(?:"([^"]*)"|'([^']*)')/gi;

/** Link destinations of a page: { value, line } for links, images, definitions and HTML href/src. */
function* destinations(tokens) {
  for (const token of walk(tokens)) {
    if (token.type === "resourceDestinationString" || token.type === "definitionDestinationString") yield { value: token.text, line: token.startLine };
    else if ((token.type === "htmlFlow" && !isComment(token)) || token.type === "htmlText")
      for (const match of token.text.matchAll(ATTRIBUTE)) {
        const before = token.text.slice(0, match.index);
        yield { value: match[1] ?? match[2], line: token.startLine + before.split("\n").length - 1 };
      }
  }
}

export default {
  names: ["argus-links"],
  description: t("仓库内的链接指向存在的文件与锚点", "Links inside the repository lead to files and anchors that exist"),
  tags: ["argus", "links"],
  parser: "micromark",
  function: (params, onError) => {
    if (!applies(params)) return;
    const root = rootOf(params);
    const config = settings(params);
    const from = isAbsolute(params.name) ? params.name : resolve(root, params.name);
    const relativeOnly = matchesAny(repoPath(from, root), config.relative_only ?? ["docs/**"]);
    const prohibited = new Set(config.prohibited_texts ?? []);
    for (const link of ofType(params.parsers.micromark.tokens, "link")) {
      const label = link.children.find((child) => child.type === "label");
      const text = label ? textOf(label).trim() : "";
      if (prohibited.has(text)) onError({ lineNumber: link.startLine, detail: t(`链接文字写目标页面或章节的名称，不写“${text}”`, `link text names the page or section it leads to, not “${text}”`), context: link.text });
    }
    for (const { value, line } of destinations(params.parsers.micromark.tokens)) {
      const destination = value
        .trim()
        .replace(/^<(.*)>$/, "$1")
        .replace(/\\([!-/:-@[-`{-~])/g, "$1");
      if (relativeOnly && ABSOLUTE.test(destination)) {
        onError({ lineNumber: line, detail: t("链接本仓库的文件用相对路径，不用 GitHub 上的地址", "link to files of this repository by relative path, not by their address on GitHub"), context: destination });
        continue;
      }
      const target = resolveLink(destination, from, root);
      const problem = target && checkTarget(target, root);
      if (problem) onError({ lineNumber: line, detail: describe(problem, t), context: destination });
    }
  },
};
