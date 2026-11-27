// argus-wording: words the documents do not use (docs/development/docs-style.md#语气与用词
// and #中英文排版), from the list in .markdownlint-cli2.jsonc: `words` as
// [{ match: [...], message, except?: [...] }] and `patterns` as
// [{ regex, message }]. Only the prose is read, not code, link addresses or
// HTML; a word quoted on its own (“你”) is a mention and passes, and `except`
// names longer words that contain a match and are fine (迷你 for 你).
import { t } from "../../console-language.mjs";
import { applies, join, locate, ofType, pieces, rangeOf, settings } from "../tokens.mjs";

const escape = (text) => text.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");

/** The matchers of a configuration, longest words first so that 你们 wins over 你. */
export function matchers({ words = [], patterns = [] }) {
  const entries = words.flatMap((entry) => entry.match.map((word) => ({ word, message: entry.message, except: entry.except ?? [] })));
  entries.sort((a, b) => b.word.length - a.word.length);
  const list = [];
  if (entries.length) list.push({ regex: new RegExp(entries.map((entry) => escape(entry.word)).join("|"), "gu"), entry: (found) => entries.find((entry) => entry.word === found) });
  for (const pattern of patterns) list.push({ regex: new RegExp(pattern.regex, "gu"), entry: () => ({ message: pattern.message, except: pattern.except ?? [] }) });
  return list;
}

/** The findings in a stretch of prose: { index, text, message }. */
export function findWords(text, list) {
  const found = [];
  for (const { regex, entry } of list)
    for (const match of text.matchAll(regex)) {
      const { message, except } = entry(match[0]);
      if (text[match.index - 1] === "“" && text[match.index + match[0].length] === "”") continue; // a mention
      const excused = except.some((longer) => {
        for (let at = text.indexOf(longer); at !== -1; at = text.indexOf(longer, at + 1)) if (at <= match.index && match.index + match[0].length <= at + longer.length) return true;
        return false;
      });
      if (!excused) found.push({ index: match.index, text: match[0], message });
    }
  return found.sort((a, b) => a.index - b.index);
}

export default {
  names: ["argus-wording"],
  description: t("不用文档规范列出的词与写法", "None of the words and spellings the style rules out"),
  tags: ["argus", "wording"],
  parser: "micromark",
  function: (params, onError) => {
    if (!applies(params)) return;
    const list = matchers(settings(params));
    if (!list.length) return;
    for (const part of ofType(params.parsers.micromark.tokens, "paragraph", "tableContent", "atxHeadingText", "setextHeadingText")) {
      const prose = pieces(part, { code: false });
      for (const finding of findWords(join(prose), list)) {
        const { line, column } = locate(prose, finding.index);
        const error = { lineNumber: line, detail: `“${finding.text}”：${finding.message}` };
        const range = rangeOf(params, line, column, finding.text.length);
        if (range) error.range = range;
        onError(error);
      }
    }
  },
};
