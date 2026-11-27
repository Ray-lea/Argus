// argus-list-item-length: the text of a list item itself (not of the lists
// nested in it) has at most 200 characters, and lists nest at most two levels
// (docs/development/docs-style.md#列表). micromark has no token for an item:
// an item runs from its listItemPrefix to the next one of the same list.
import { t } from "../../console-language.mjs";
import { LISTS, applies, excerpt, join, length, ofType, pieces, settings } from "../tokens.mjs";

function depth(list) {
  let levels = 1;
  for (let parent = list.parent; parent; parent = parent.parent) if (LISTS.includes(parent.type)) levels++;
  return levels;
}

export default {
  names: ["argus-list-item-length"],
  description: t("列表项不超过 200 字，列表最多嵌套两层", "List items have at most 200 characters; lists nest at most two levels"),
  tags: ["argus", "length"],
  parser: "micromark",
  function: (params, onError) => {
    if (!applies(params)) return;
    const { maximum = 200, depth: deepest = 2 } = settings(params);
    for (const list of ofType(params.parsers.micromark.tokens, ...LISTS)) {
      const level = depth(list);
      if (level === deepest + 1) {
        onError({ lineNumber: list.startLine, detail: t(`第 ${level} 层列表：列表最多嵌套 ${deepest} 层，较长的内容改为下一级标题`, `a list at level ${level}: lists nest at most ${deepest} levels; turn long content into a subsection`) });
      }
      const items = [];
      for (const child of list.children) {
        if (child.type === "listItemPrefix") items.push({ line: child.startLine, parts: [] });
        else if (child.type === "content") items.at(-1)?.parts.push(child);
      }
      for (const item of items) {
        const text = item.parts.map((part) => join(pieces(part))).join(" ");
        const count = length(text);
        if (count > maximum) onError({ lineNumber: item.line, detail: t(`列表项 ${count} 字，上限 ${maximum}`, `list item of ${count} characters, at most ${maximum}`), context: excerpt(text) });
      }
    }
  },
};
