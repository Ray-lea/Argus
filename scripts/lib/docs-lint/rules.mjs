// The repository's markdownlint rules (customRules in .markdownlint-cli2.jsonc).
// Each takes `include` (repository path patterns it applies to, docs/** unless
// configured) and `exempt` (pages it skips while they are brought in line).
import alerts from "./rules/alerts.mjs";
import emphasis from "./rules/emphasis.mjs";
import headings from "./rules/headings.mjs";
import lead from "./rules/lead.mjs";
import links from "./rules/links.mjs";
import listItemLength from "./rules/list-item-length.mjs";
import paragraphLength from "./rules/paragraph-length.mjs";
import sectionBlocks from "./rules/section-blocks.mjs";
import sentenceLength from "./rules/sentence-length.mjs";
import tableCellLength from "./rules/table-cell-length.mjs";
import uiPath from "./rules/ui-path.mjs";
import wording from "./rules/wording.mjs";

export default [lead, headings, paragraphLength, sentenceLength, listItemLength, tableCellLength, sectionBlocks, alerts, emphasis, uiPath, wording, links];
