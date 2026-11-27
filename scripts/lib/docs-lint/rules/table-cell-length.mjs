// argus-table-cell-length: a table cell holds a phrase or one sentence of at
// most 80 characters, without <br> line breaks, and is never empty: a cell
// with nothing to say reads "无" (docs/development/docs-style.md#表格).
import { t } from "../../console-language.mjs";
import { applies, excerpt, length, ofType, settings, textOf } from "../tokens.mjs";

export default {
  names: ["argus-table-cell-length"],
  description: t("单元格不超过 80 字，不用 <br>，不留空", "Table cells have at most 80 characters, no <br> and are not empty"),
  tags: ["argus", "length", "table"],
  parser: "micromark",
  function: (params, onError) => {
    if (!applies(params)) return;
    const maximum = settings(params).maximum ?? 80;
    for (const cell of ofType(params.parsers.micromark.tokens, "tableHeader", "tableData")) {
      const content = cell.children.find((child) => child.type === "tableContent");
      if (!content) {
        onError({ lineNumber: cell.startLine, detail: t("单元格不留空，没有内容时写“无”", "the cell is empty: write “无” when there is nothing to say") });
        continue;
      }
      for (const html of ofType(content.children, "htmlText"))
        if (/^<br\s*\/?>$/i.test(html.text))
          onError({ lineNumber: html.startLine, range: [html.startColumn, html.text.length], detail: t("单元格中不用 <br> 换行：较长的说明放在表格之后", "no <br> in cells: put a longer explanation after the table") });
      const text = textOf(content);
      const count = length(text);
      if (count > maximum) onError({ lineNumber: content.startLine, detail: t(`单元格 ${count} 字，上限 ${maximum}`, `cell of ${count} characters, at most ${maximum}`), context: excerpt(text) });
    }
  },
};
