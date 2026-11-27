// argus-section-blocks: under one heading, up to the next heading of any
// level, at most 8 top-level blocks (paragraphs, lists, tables, code, alerts,
// HTML); more call for a subsection (docs/development/docs-style.md#页面结构).
import { t } from "../../console-language.mjs";
import { applies, blockKind, blocks, headingContent, settings, textOf } from "../tokens.mjs";

export default {
  names: ["argus-section-blocks"],
  description: t("同一标题下的块不超过 8 个", "At most 8 blocks under one heading"),
  tags: ["argus", "structure"],
  parser: "micromark",
  function: (params, onError) => {
    if (!applies(params)) return;
    const maximum = settings(params).maximum ?? 8;
    const sections = [{ heading: null, blocks: [] }];
    // A disclosure (<details> … </details>) counts as one block with what it holds.
    let disclosure = false;
    for (const token of blocks(params)) {
      const kind = blockKind(token);
      // The block's own source: the text of raw HTML is not among its data tokens.
      const html = kind === "html" ? token.text.trim() : "";
      if (disclosure) {
        if (html.endsWith("</details>")) disclosure = false;
        continue;
      }
      if (kind === "heading") sections.push({ heading: token, blocks: [] });
      else if (kind !== "break") sections.at(-1).blocks.push(token);
      if (html.startsWith("<details") && !html.endsWith("</details>")) disclosure = true;
    }
    for (const { heading, blocks: under } of sections) {
      if (under.length <= maximum) continue;
      const error = {
        lineNumber: (heading ?? under[0]).startLine,
        detail: t(`本节有 ${under.length} 个块，上限 ${maximum}（第 ${maximum + 1} 个在第 ${under[maximum].startLine} 行）：按主题拆分出下一级标题`, `${under.length} blocks in this section, at most ${maximum} (block ${maximum + 1} is on line ${under[maximum].startLine}): split it by topic`),
      };
      if (heading) error.context = textOf(headingContent(heading) ?? heading);
      onError(error);
    }
  },
};
