// argus-headings: how headings are used (docs/development/docs-style.md#页面结构).
// A heading is followed by text before the next heading; a heading with
// subsections has at least two of them; headings hold no links, have no
// numbers in front, are not wholly bold or code, and go down to level 4 at
// most. The title's lead is argus-lead's.
import { t } from "../../console-language.mjs";
import { applies, blockKind, blocks, headingContent, headingLevel, ofType, settings, textOf } from "../tokens.mjs";

// 1. 安装, 1、安装, 一、安装, 第一章 (a version such as 0.6 版 is not a number in front).
const NUMBERED = /^(?:\d+[.、)）](?!\d)|[一二三四五六七八九十]+[、.．]|第[一二三四五六七八九十百\d]+[章节部分篇])/u;
// Steps (docs-style.md#分步): "1. 标题", "2. 标题" in order at one level, which
// the website shows as a step list. Any other heading at that level or above
// ends the steps.
const STEP = /^(\d+)\. \S/u;

export default {
  names: ["argus-headings"],
  description: t("标题之间有正文，下级标题至少两个，标题中不放链接、不编号", "Text between headings, at least two subsections, no links or numbers in headings"),
  tags: ["argus", "headings"],
  parser: "micromark",
  function: (params, onError) => {
    if (!applies(params)) return;
    const levels = settings(params).levels ?? 4;
    const list = blocks(params);
    const headings = list.filter((token) => blockKind(token) === "heading");
    for (let i = 0; i < list.length; i++) {
      const token = list[i];
      if (blockKind(token) !== "heading" || headingLevel(token) === 1) continue;
      const next = list[i + 1];
      if (!next) onError({ lineNumber: token.startLine, detail: t("标题下缺少正文", "nothing under the heading"), context: textOf(headingContent(token) ?? token) });
      else if (blockKind(next) === "heading") onError({ lineNumber: token.startLine, detail: t("标题之后先写一段正文，不叠放标题", "write some text after a heading before the next one"), context: textOf(headingContent(token) ?? token) });
    }
    for (const [index, heading] of headings.entries()) {
      const level = headingLevel(heading);
      const children = [];
      for (const next of headings.slice(index + 1)) {
        const nextLevel = headingLevel(next);
        if (nextLevel <= level) break;
        if (nextLevel === level + 1) children.push(next);
      }
      if (children.length === 1) onError({ lineNumber: children[0].startLine, detail: t("同一级的标题至少两个：只有一个下级标题时不分级", "a heading with subsections has at least two: do not divide for one"), context: textOf(headingContent(children[0]) ?? children[0]) });
    }
    // The number of the last step at each level.
    const steps = new Map();
    for (const heading of headings) {
      const content = headingContent(heading);
      if (!content) continue;
      const text = textOf(content).trim();
      const level = headingLevel(heading);
      for (const deeper of [...steps.keys()]) if (deeper > level) steps.delete(deeper);
      if (ofType(content.children, "link", "autolink", "literalAutolink").length) onError({ lineNumber: heading.startLine, detail: t("标题中不放链接：链接写在正文中", "no links in headings: put them in the text"), context: text });
      if (level > levels) onError({ lineNumber: heading.startLine, detail: t(`标题最多到 ${levels} 级`, `headings go down to level ${levels} at most`), context: text });
      const step = STEP.exec(text);
      if (step) {
        const number = Number(step[1]);
        if (number !== (steps.get(level) ?? 0) + 1) onError({ lineNumber: heading.startLine, detail: t("步骤标题在同一级从 1 开始连续编号", "step headings are numbered 1, 2, 3 at one level"), context: text });
        steps.set(level, number);
      } else {
        steps.delete(level);
        if (NUMBERED.test(text)) onError({ lineNumber: heading.startLine, detail: t("标题不编号；分步的标题写作“1. 标题”", "no numbers in front of headings; steps are written “1. Title”"), context: text });
      }
      const parts = content.children.filter((child) => !(child.type === "data" && !child.text.trim()));
      if (parts.length === 1 && (parts[0].type === "strong" || parts[0].type === "codeText")) onError({ lineNumber: heading.startLine, detail: t("不用加粗或代码格式包住整个标题", "no bold or code around a whole heading"), context: text });
    }
  },
};
