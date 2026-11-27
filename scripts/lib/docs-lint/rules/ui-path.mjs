// argus-ui-path: an interface path is bold with " → " between its levels,
// **设置 → 权限 → 特权服务** (docs/development/docs-style.md#格式语义). Reported:
// a path in Chinese quotes, and inside bold >, ->, => or an arrow without one
// space on each side.
import { t } from "../../console-language.mjs";
import { CODE, applies, join, locate, ofType, pieces } from "../tokens.mjs";

// Chinese quotes around levels with a path separator between them; a quoted
// separator alone (“ → ”) names the separator and passes.
const QUOTED = /“[^“”]*?[^\s“”>=→-][ \t]*(?:→|->|=>|>)[ \t]*[^\s“”>=→-][^“”]*?”/gu;
const BAD_ARROW = /(?:^|[^ ])→|→(?:[^ ]|$)| {2,}→|→ {2,}/u;

export default {
  names: ["argus-ui-path"],
  description: t("界面路径加粗，层级之间用“ → ”", "Interface paths are bold, with “ → ” between the levels"),
  tags: ["argus", "emphasis"],
  parser: "micromark",
  function: (params, onError) => {
    if (!applies(params)) return;
    const tokens = params.parsers.micromark.tokens;
    for (const part of ofType(tokens, "paragraph", "tableContent", "atxHeadingText", "setextHeadingText")) {
      const list = pieces(part, { code: false });
      const text = join(list);
      for (const match of text.matchAll(QUOTED)) {
        const { line } = locate(list, match.index);
        onError({ lineNumber: line, detail: t("界面路径用加粗，不用引号，层级之间用“ → ”", "interface paths are bold, not quoted, with “ → ” between the levels"), context: match[0] });
      }
    }
    for (const strong of ofType(tokens, "strong")) {
      const text = join(pieces(strong, { code: false })).replaceAll(CODE, "");
      if (/>/.test(text)) onError({ lineNumber: strong.startLine, detail: t("界面路径的层级之间用“ → ”，不用 >、->、=>", "use “ → ” between the levels of a path, not >, -> or =>"), context: text });
      else if (BAD_ARROW.test(text)) onError({ lineNumber: strong.startLine, detail: t("“→”前后各留一个空格", "one space on each side of “→”"), context: text });
    }
  },
};
