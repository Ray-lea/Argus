// argus-paragraph-length: a paragraph has at most 250 characters and 6
// sentences (docs/development/docs-style.md#段落与句子). Paragraphs in list
// items are measured as items by argus-list-item-length.
import { t } from "../../console-language.mjs";
import { ALERT_MARK, LISTS, applies, excerpt, hasAncestor, length, ofType, sentences, settings, textOf } from "../tokens.mjs";

export default {
  names: ["argus-paragraph-length"],
  description: t("段落不超过 250 字、6 句", "Paragraphs have at most 250 characters and 6 sentences"),
  tags: ["argus", "length"],
  parser: "micromark",
  function: (params, onError) => {
    if (!applies(params)) return;
    const { maximum = 250, sentences: most = 6 } = settings(params);
    for (const paragraph of ofType(params.parsers.micromark.tokens, "paragraph")) {
      if (hasAncestor(paragraph, LISTS)) continue;
      const text = textOf(paragraph).replace(ALERT_MARK, "");
      const count = length(text);
      if (count > maximum) onError({ lineNumber: paragraph.startLine, detail: t(`段落 ${count} 字，上限 ${maximum}`, `paragraph of ${count} characters, at most ${maximum}`), context: excerpt(text) });
      const said = sentences(text).length;
      if (said > most) onError({ lineNumber: paragraph.startLine, detail: t(`段落 ${said} 句，上限 ${most}`, `paragraph of ${said} sentences, at most ${most}`), context: excerpt(text) });
    }
  },
};
