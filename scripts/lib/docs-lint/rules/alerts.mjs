// argus-alerts: block quotes are GitHub alerts only. Each starts with a line of
// its own, [!NOTE], [!TIP], [!IMPORTANT], [!WARNING] or [!CAUTION], has a body
// of at most 3 sentences, holds no heading and no other quote, and two alerts
// are never adjacent blocks (docs/development/docs-style.md#告警块).
import { t } from "../../console-language.mjs";
import { ALERT_MARK, ALERT_TYPES, applies, blockKind, blocks, hasAncestor, join, ofType, pieces, sentences, settings } from "../tokens.mjs";

export default {
  names: ["argus-alerts"],
  description: t("引用块只用作告警块，写法见文档规范", "Block quotes are alerts only, written as the style says"),
  tags: ["argus", "alerts"],
  parser: "micromark",
  function: (params, onError) => {
    if (!applies(params)) return;
    const most = settings(params).sentences ?? 3;
    const tokens = params.parsers.micromark.tokens;
    for (const quote of ofType(tokens, "blockQuote")) {
      if (hasAncestor(quote, ["blockQuote"])) {
        onError({ lineNumber: quote.startLine, detail: t("告警块不嵌套", "alerts do not nest") });
        continue;
      }
      const lines = params.lines.slice(quote.startLine - 1, quote.endLine).map((line) => line.replace(/^ {0,3}> ?/, ""));
      const first = lines[0].trim();
      const mark = /^\[!([A-Za-z]+)\]/.exec(first);
      if (!mark) {
        onError({
          lineNumber: quote.startLine,
          detail: t("不使用普通引用块：告警块以单独一行的 [!NOTE]、[!TIP]、[!IMPORTANT]、[!WARNING] 或 [!CAUTION] 开头", "no plain block quotes: an alert starts with a line of [!NOTE], [!TIP], [!IMPORTANT], [!WARNING] or [!CAUTION]"),
        });
        continue;
      }
      if (!ALERT_TYPES.includes(mark[1])) {
        onError({ lineNumber: quote.startLine, detail: t(`没有 [!${mark[1]}] 这种告警块：用 ${ALERT_TYPES.map((type) => `[!${type}]`).join("、")}`, `there is no [!${mark[1]}] alert: use ${ALERT_TYPES.map((type) => `[!${type}]`).join(", ")}`) });
        continue;
      }
      if (first !== mark[0]) onError({ lineNumber: quote.startLine, detail: t(`${mark[0]} 单独占一行，正文从下一行开始`, `${mark[0]} takes a line of its own; the body starts on the next one`) });
      else if (!lines.slice(1).some((line) => line.trim())) onError({ lineNumber: quote.startLine, detail: t("告警块缺少正文", "the alert has no body") });
      for (const heading of ofType(quote.children, "atxHeading", "setextHeading")) onError({ lineNumber: heading.startLine, detail: t("告警块中不放标题", "no headings in alerts") });
      // The quote's own paragraphs (content tokens are their parents), not those of a nested quote.
      const body = ofType(quote.children, "paragraph")
        .filter((paragraph) => paragraph.parent?.parent === quote)
        .map((paragraph) => join(pieces(paragraph)))
        .join(" ")
        .replace(ALERT_MARK, "");
      const said = sentences(body).length;
      if (said > most) onError({ lineNumber: quote.startLine, detail: t(`告警块 ${said} 句，上限 ${most}：正文的主要内容不放进告警块`, `an alert of ${said} sentences, at most ${most}: the main content belongs in the text`) });
    }
    let previous = null;
    for (const token of blocks(params)) {
      if (blockKind(token) === "quote" && previous && blockKind(previous) === "quote")
        onError({ lineNumber: token.startLine, detail: t("两个告警块不相邻：合并，或把其中一个写进正文", "two alerts are not adjacent: merge them, or move one into the text") });
      previous = token;
    }
  },
};
