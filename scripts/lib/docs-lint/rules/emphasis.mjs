// argus-emphasis: bold marks interface elements only (docs/development/docs-style.md#格式语义).
// Reported: ** or __ left as text because CommonMark did not pair them (bold
// that starts or ends with punctuation next to Chinese), italics, bold of more
// than 40 characters (80 for a path with " → "), bold around inline code
// alone, interface names that end with an ellipsis or a colon, a space between
// the marks and Chinese text when the bold text begins or ends with Chinese
// (在**关于**页), and none when it begins or ends with a Latin letter or digit
// (选择 **Check**，). Unpaired marks stay in micromark "data" tokens; code
// spans, code blocks and escaped \*\* do not, so they are never reported.
import { t } from "../../console-language.mjs";
import { applies, excerpt, length, ofType, settings, textOf } from "../tokens.mjs";

const NOT_TEXT = new Set(["resource", "reference", "definition", "htmlText", "htmlFlow"]);
const HAN = /^\p{Script=Han}$/u;
const LATIN = /^[A-Za-z0-9]$/;

function* dataTokens(tokens) {
  for (const token of tokens) {
    if (NOT_TEXT.has(token.type)) continue;
    if (token.type === "data") yield token;
    yield* dataTokens(token.children);
  }
}

export default {
  names: ["argus-emphasis"],
  description: t("加粗只用于界面元素，写法见文档规范；不用斜体", "Bold marks interface elements as the style says; no italics"),
  tags: ["argus", "emphasis"],
  parser: "micromark",
  function: (params, onError) => {
    if (!applies(params)) return;
    const { maximum = 40, path_maximum: pathMaximum = 80 } = settings(params);
    const tokens = params.parsers.micromark.tokens;
    for (const token of dataTokens(tokens))
      for (const match of token.text.matchAll(/\*\*|__/g))
        onError({
          lineNumber: token.startLine,
          range: [token.startColumn + match.index, 2],
          detail: t(
            `${match[0]} 没有解析为加粗：加粗的内容以标点开头或结尾时，加粗标记外侧须是空格、标点或句子边界`,
            `${match[0]} did not become bold: when bold text starts or ends with punctuation, the marks need a space, punctuation or the end of the sentence outside`,
          ),
        });
    for (const emphasis of ofType(tokens, "emphasis")) onError({ lineNumber: emphasis.startLine, detail: t("不使用斜体", "no italics"), context: textOf(emphasis) });
    for (const strong of ofType(tokens, "strong")) {
      const inner = strong.children.find((child) => child.type === "strongText");
      if (!inner) continue;
      const text = textOf(inner).trim();
      const count = length(text);
      // An interface path names every level: it may run to `path_maximum`.
      const limit = text.includes(" → ") ? pathMaximum : maximum;
      if (count > limit) onError({ lineNumber: strong.startLine, detail: t(`加粗 ${count} 字，上限 ${limit}：加粗只用于界面元素，不用于强调`, `bold of ${count} characters, at most ${limit}: bold marks interface elements, not emphasis`), context: excerpt(text) });
      const parts = inner.children.filter((child) => !(child.type === "data" && !child.text.trim()));
      if (parts.length && parts.every((child) => child.type === "codeText")) onError({ lineNumber: strong.startLine, detail: t("加粗只包住行内代码：去掉加粗", "bold around inline code alone: drop the bold"), context: text });
      if (/(…|\.\.\.|：|:)$/u.test(text)) onError({ lineNumber: strong.startLine, detail: t("引用界面元素时去掉末尾的省略号与冒号", "drop the ellipsis or colon at the end of an interface name"), context: text });
      // At each mark: the character inside it, the spaces outside it and the character past them.
      const characters = [...text];
      const left = params.lines[strong.startLine - 1].slice(0, strong.startColumn - 1);
      const right = params.lines[strong.endLine - 1].slice(strong.endColumn - 1);
      const leftGap = /[ \t]*$/.exec(left)[0];
      const rightGap = /^[ \t]*/.exec(right)[0];
      const sides = [
        { inside: characters[0], gap: leftGap, beyond: [...left.slice(0, left.length - leftGap.length)].at(-1) },
        { inside: characters.at(-1), gap: rightGap, beyond: [...right.slice(rightGap.length)][0] },
      ];
      if (sides.some(({ inside, gap, beyond }) => HAN.test(inside ?? "") && gap && HAN.test(beyond ?? "")))
        onError({ lineNumber: strong.startLine, detail: t("加粗标记与相邻的汉字之间不加空格", "no space between the bold marks and the Chinese text next to them"), context: text });
      if (sides.some(({ inside, gap, beyond }) => LATIN.test(inside ?? "") && !gap && HAN.test(beyond ?? "")))
        onError({ lineNumber: strong.startLine, detail: t("加粗的英文与相邻的汉字之间加空格", "a space between bold Latin text and the Chinese text next to it"), context: text });
    }
  },
};
