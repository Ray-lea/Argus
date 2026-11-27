// argus-lead: the block after the page's title is the lead, a paragraph of at
// most 150 characters; the website shows it as the page's summary
// (docs/development/docs-style.md#页面结构).
import { t } from "../../console-language.mjs";
import { applies, blockKind, blocks, excerpt, headingContent, headingLevel, length, settings, textOf } from "../tokens.mjs";

export default {
  names: ["argus-lead"],
  description: t("一级标题之后是不超过 150 字的导语段落", "The title is followed by a lead paragraph of at most 150 characters"),
  tags: ["argus", "structure"],
  parser: "micromark",
  function: (params, onError) => {
    if (!applies(params)) return;
    const maximum = settings(params).maximum ?? 150;
    const list = blocks(params);
    const at = list.findIndex((token) => headingLevel(token) === 1);
    if (at === -1) return; // MD041 and MD025 report the title
    const title = list[at];
    const lead = list[at + 1];
    if (!lead || blockKind(lead) === "heading") {
      onError({ lineNumber: title.startLine, detail: t("一级标题之后缺少导语", "no lead after the title"), context: textOf(headingContent(title) ?? title) });
      return;
    }
    if (blockKind(lead) !== "paragraph") {
      onError({
        lineNumber: lead.startLine,
        detail: t("导语须是段落：表格、列表、代码块与告警块放在导语之后", "the lead must be a paragraph: tables, lists, code and alerts come after it"),
      });
      return;
    }
    const text = textOf(lead);
    const count = length(text);
    if (count > maximum) onError({ lineNumber: lead.startLine, detail: t(`导语 ${count} 字，上限 ${maximum}`, `lead of ${count} characters, at most ${maximum}`), context: excerpt(text) });
  },
};
