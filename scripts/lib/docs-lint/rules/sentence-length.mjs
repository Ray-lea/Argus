// argus-sentence-length: a sentence of a paragraph, list item or table cell,
// ended by 。！？, has at most 120 characters (docs/development/docs-style.md#段落与句子
// gives 50 for explanations and 35 for steps as the aim, 120 as the limit).
import { t } from "../../console-language.mjs";
import { ALERT_MARK, applies, excerpt, join, length, locate, ofType, pieces, sentences, settings } from "../tokens.mjs";

export default {
  names: ["argus-sentence-length"],
  description: t("句子不超过 120 字", "Sentences have at most 120 characters"),
  tags: ["argus", "length"],
  parser: "micromark",
  function: (params, onError) => {
    if (!applies(params)) return;
    const maximum = settings(params).maximum ?? 120;
    for (const part of ofType(params.parsers.micromark.tokens, "paragraph", "tableContent")) {
      if (part.parent?.type === "tableDelimiter") continue;
      const list = pieces(part);
      const whole = join(list);
      const mark = ALERT_MARK.exec(whole)?.[0].length ?? 0;
      for (const sentence of sentences(whole.slice(mark))) {
        const count = length(sentence.text);
        if (count <= maximum) continue;
        const lead = sentence.text.length - sentence.text.trimStart().length;
        const { line } = locate(list, mark + sentence.start + lead);
        onError({ lineNumber: line, detail: t(`句子 ${count} 字，上限 ${maximum}`, `sentence of ${count} characters, at most ${maximum}`), context: excerpt(sentence.text) });
      }
    }
  },
};
