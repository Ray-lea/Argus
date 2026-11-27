import assert from "node:assert/strict";
import { test } from "node:test";
import { mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { lint } from "markdownlint-cli2/markdownlint/promise";
import parseJsonc from "markdownlint-cli2/parsers/jsonc";
import { anchorsOf, decodeFragment, headingText, headings } from "../lib/docs-lint/anchors.mjs";
import { autocorrectSupported, maskFences, unmask } from "../lib/docs-lint/autocorrect.mjs";
import { clearListings, existence, globToRegExp } from "../lib/docs-lint/paths.mjs";
import { checkReferences, findReferences, maskCode } from "../lib/docs-lint/references.mjs";
import rules from "../lib/docs-lint/rules.mjs";
import { AUTOCORRECT, EXCLUDED, PAGES, PUBLIC, isAutocorrectTarget, isMarkdownlintTarget, targets } from "../lib/docs-lint/scope.mjs";
import { sentences } from "../lib/docs-lint/tokens.mjs";

const root = join(import.meta.dirname, "..", "..");
const config = parseJsonc(readFileSync(join(root, ".markdownlint-cli2.jsonc"), "utf8"));

/** The errors one rule reports for a page of docs/ (its parameters from the repository's configuration, then `options`). */
async function check(rule, markdown, options = {}, name = "docs/guide/sample.md") {
  const results = await lint({ strings: { [name]: markdown }, config: { default: false, [rule]: { ...config.config[rule], ...options } }, customRules: rules });
  return results[name].filter((error) => error.ruleNames.includes(rule));
}
const details = (errors) => errors.map((error) => `${error.lineNumber}: ${error.errorDetail}`);
/** A page with a title and a lead around `lines`. */
const page = (...lines) => ["# 示例", "", "本页说明示例。", "", ...lines, ""].join("\n");
const long = (count) => "字".repeat(count);

/** A throwaway repository for the link checks: { path: content }. */
function repository(files) {
  const dir = mkdtempSync(join(tmpdir(), "argus-docs-lint-test-"));
  for (const [path, content] of Object.entries(files)) {
    mkdirSync(dirname(join(dir, path)), { recursive: true });
    writeFileSync(join(dir, path), content);
  }
  clearListings();
  return dir;
}

test("the configuration lists the files scope.mjs checks and configures every rule", () => {
  assert.deepEqual(config.globs, [...PAGES, ...PUBLIC]);
  assert.deepEqual(config.ignores, EXCLUDED);
  assert.deepEqual(config.overrides[0].filter, PUBLIC);
  for (const rule of rules) {
    const own = config.config[rule.names[0]];
    assert(own, `${rule.names[0]} has no entry in .markdownlint-cli2.jsonc`);
    assert(Array.isArray(own.include) && Array.isArray(own.exempt), `${rule.names[0]} needs include and exempt`);
    assert(rule.names[0].startsWith("argus-"));
  }
});

test("each tool's files: docs pages for everything, public Markdown for structure, AGENTS.md and the changelog for neither", () => {
  assert(isMarkdownlintTarget("docs/guide/settings.md"));
  assert(isMarkdownlintTarget("README.ja.md") && isMarkdownlintTarget(".github/CONTRIBUTING.md"));
  assert(!isMarkdownlintTarget("docs/AGENTS.md") && !isMarkdownlintTarget("changelog/zh-CN.md") && !isMarkdownlintTarget("crates/argus-core/README.md"));
  assert(isAutocorrectTarget("README.zh-TW.md") && !isAutocorrectTarget("README.ja.md") && !isAutocorrectTarget(".github/AGENTS.md"));
  assert(AUTOCORRECT.every((glob) => !globToRegExp(glob).test("changelog/zh-CN.md")));
  assert(globToRegExp("docs/**/*.md").test("docs/x.md") && globToRegExp("docs/**/*.md").test("docs/a/b/x.md") && !globToRegExp("README*.md").test("docs/README.md"));
  const all = ["docs/a.md", "docs/AGENTS.md", "README.md", "README.en.md", "src/x.rs"];
  assert.deepEqual(targets(all), { markdownlint: ["docs/a.md", "README.md", "README.en.md"], autocorrect: ["docs/a.md", "README.md"], outside: [] });
  assert.deepEqual(targets(all, ["src/x.rs", "README.en.md"]), { markdownlint: ["README.en.md"], autocorrect: [], outside: ["src/x.rs"] });
  assert.deepEqual(targets(all, []).markdownlint, [], "named files that are all missing check nothing");
});

test("sentences end at 。！？ with the quotes after them", () => {
  assert.deepEqual(
    sentences("先写结论。再写“原因。”最后").map((sentence) => sentence.text),
    ["先写结论。", "再写“原因。”", "最后"],
  );
});

test("argus-lead: the title is followed by a short lead paragraph", async () => {
  assert.deepEqual(await check("argus-lead", page("## 节", "", "正文。")), []);
  assert.match(details(await check("argus-lead", "# 示例\n\n- 列表\n- 列表\n"))[0], /^3: /);
  assert.match(details(await check("argus-lead", `# 示例\n\n${long(151)}。\n`))[0], /152/);
  assert.equal((await check("argus-lead", "# 示例\n\n## 节\n\n正文。\n")).length, 1);
});

test("argus-headings: text between headings, two subsections or none, no links", async () => {
  const good = page("## 甲", "", "正文。", "", "### 一", "", "正文。", "", "### 二", "", "正文。", "", "## 乙", "", "正文。");
  assert.deepEqual(await check("argus-headings", good), []);
  assert.match(details(await check("argus-headings", page("## 甲", "", "### 一", "", "正文。", "", "### 二", "", "正文。", "", "## 乙", "", "正文。")))[0], /^5: .*正文/);
  assert.match(details(await check("argus-headings", page("## 甲", "", "正文。", "", "### 一", "", "正文。", "", "## 乙", "", "正文。")))[0], /^9: /);
  const linked = await check("argus-headings", page("## [甲](other.md)", "", "正文。", "", "## 乙", "", "正文。"));
  assert.equal(linked.length, 1);
  assert.match(linked[0].errorDetail, /链接|link/);
  const outline = (heading) => page("## 甲", "", "正文。", "", "### 一", "", "正文。", "", "### 二", "", "正文。", "", heading, "", "正文。");
  assert.deepEqual(await check("argus-headings", outline("## 0.6 版的变化")), [], "a version is not a number in front");
  for (const [heading, pattern] of [
    ["## 1、安装", /编号|numbers/],
    ["## 2. 安装", /连续编号|numbered 1, 2, 3/],
    ["## **加粗**", /加粗|bold/],
    ["## `code`", /代码|code/],
  ])
    assert.match(details(await check("argus-headings", outline(heading))).join("\n"), pattern, heading);
  // Steps: "1. ", "2. " in order at one level (a step list on the website).
  const steps = page("## 1. 下载", "", "正文。", "", "## 2. 安装", "", "正文。", "", "## 常见问题", "", "正文。");
  assert.deepEqual(await check("argus-headings", steps), [], "numbered steps in order");
  const deep = page("## 甲", "", "正文。", "", "### 一", "", "正文。", "", "#### 1", "", "正文。", "", "##### 五级", "", "正文。", "", "##### 五级二", "", "正文。", "", "#### 2", "", "正文。", "", "### 二", "", "正文。", "", "## 乙", "", "正文。");
  assert.deepEqual(details(await check("argus-headings", deep)).map((detail) => detail.split(":")[0]), ["17", "21"], "levels 5 and below");
});

test("argus-paragraph-length: 250 characters and 6 sentences", async () => {
  assert.deepEqual(await check("argus-paragraph-length", page(`${long(249)}。`, "", "一。二。三。四。五。六。")), []);
  assert.match(details(await check("argus-paragraph-length", page(`${long(250)}。`)))[0], /251/);
  assert.match(details(await check("argus-paragraph-length", page("一。二。三。四。五。六。七。")))[0], /7/);
  assert.deepEqual(await check("argus-paragraph-length", page(`- ${long(300)}`)), [], "list items are measured by argus-list-item-length");
});

test("argus-sentence-length: 120 characters in paragraphs, list items and cells", async () => {
  assert.deepEqual(await check("argus-sentence-length", page(`${long(119)}。${long(119)}。`)), []);
  assert.match(details(await check("argus-sentence-length", page(`短句。${long(121)}。`)))[0], /122/);
  assert.equal((await check("argus-sentence-length", page("| 名称 | 说明 |", "| --- | --- |", `| 甲 | ${long(121)} |`, "| 乙 | 无 |", "| 丙 | 无 |"))).length, 1);
  assert.equal((await check("argus-sentence-length", page(`- ${long(121)}`))).length, 1);
});

test("argus-list-item-length: 200 characters, two levels", async () => {
  assert.deepEqual(await check("argus-list-item-length", page(`- ${long(200)}`, `  - ${long(200)}`)), []);
  assert.match(details(await check("argus-list-item-length", page("- 短", `- ${long(201)}`)))[0], /^6: .*201/);
  assert.match(details(await check("argus-list-item-length", page("- 一", "  - 二", "    - 三")))[0], /^7: /);
});

test("argus-table-cell-length: 80 characters, no <br>, no empty cells", async () => {
  const table = (cell) => page("| 名称 | 说明 |", "| --- | --- |", `| 甲 | ${cell} |`, "| 乙 | 无 |", "| 丙 | 无 |");
  assert.deepEqual(await check("argus-table-cell-length", table(long(80))), []);
  assert.match(details(await check("argus-table-cell-length", table(long(81))))[0], /81/);
  assert.match(details(await check("argus-table-cell-length", table("一<br>二")))[0], /<br>/);
  assert.match(details(await check("argus-table-cell-length", table("")))[0], /无/);
});

test("argus-section-blocks: at most 8 blocks under a heading", async () => {
  const blocks = (count) => Array.from({ length: count }, (_, i) => [`段落 ${i}。`, ""]).flat();
  assert.deepEqual(await check("argus-section-blocks", page("## 节", "", ...blocks(8))), []);
  assert.match(details(await check("argus-section-blocks", page("## 节", "", ...blocks(9))))[0], /^5: .*9/);
  // A disclosure counts once with the paragraphs it holds.
  const disclosure = (i) => ["<details>", `<summary>问题 ${i}？</summary>`, "", `回答 ${i}。`, "", `补充 ${i}。`, "", "</details>", ""];
  const faq = page("## 常见问题", "", ...[1, 2, 3, 4, 5, 6, 7, 8].flatMap(disclosure));
  assert.deepEqual(await check("argus-section-blocks", faq), []);
});

test("argus-alerts: GitHub alerts only, with a body, not nested, not adjacent", async () => {
  assert.deepEqual(await check("argus-alerts", page("> [!WARNING]", "> 不要在开发机上运行安装程序。", "", "正文。", "", "> [!NOTE]", "> 说明。")), []);
  for (const quote of [["> 普通引用。"], ["> [!DANGER]", "> 正文。"], ["> [!NOTE]"], ["> [!NOTE] 正文。"], ["> [!NOTE]", "> ## 标题", "> 正文。"], ["> [!NOTE]", "> 一。二。三。四。"], ["> [!NOTE]", "> > 嵌套。"]])
    assert((await check("argus-alerts", page(...quote))).length >= 1, quote.join(" / "));
  assert.match(details(await check("argus-alerts", page("> [!NOTE]", "> 说明。", "", "> [!TIP]", "> 提示。")))[0], /^8: .*相邻|adjacent/);
});

test("argus-emphasis: bold for interface names, written as the style says", async () => {
  assert.deepEqual(await check("argus-emphasis", page("在**关于**页单击**复制诊断信息**。选择 **Check**，在**设置 → 权限 → 特权服务**中。代码 `**` 与 \\*\\* 不算。")), []);
  const cases = [
    ["单击**“设置”**后。", /\*\*/],
    ["这是*斜体*。", /斜体|italic/],
    [`单击**${long(41)}**。`, /41/],
    ["运行 **`pnpm docs:lint`** 检查。", /代码|code/],
    ["单击**运行新任务…**。", /省略号|ellipsis/],
    ["单击**名称：**。", /冒号|colon/],
    ["在 **关于** 页。", /不加空格|no space/],
    ["选择**Check**后。", /加空格|a space/],
  ];
  for (const [line, pattern] of cases) assert.match(details(await check("argus-emphasis", page(line))).join("\n"), pattern, line);
  assert.deepEqual(await check("argus-emphasis", page(`在**${long(30)} → ${long(30)}**中。`)), [], "paths may run to 80");
  assert.equal((await check("argus-emphasis", page(`在**${long(41)} → ${long(41)}**中。`))).length, 1);
});

test("argus-ui-path: paths in bold with “ → ”", async () => {
  assert.deepEqual(await check("argus-ui-path", page("打开**设置 → 常规**。层级之间用“ → ”连接。")), []);
  for (const line of ["打开“设置 → 常规”。", "打开**设置 > 常规**。", "打开**设置 -> 常规**。", "打开**设置→常规**。"]) assert.equal((await check("argus-ui-path", page(line))).length, 1, line);
});

test("argus-wording: the configured words, not their mentions or code", async () => {
  assert.deepEqual(await check("argus-wording", page("文档不用“你”与“点击”。迷你窗口。运行 `点击` 与 `你好`。加载中……完成！")), []);
  const found = details(await check("argus-wording", page("你可以点击其它帐户，其实蛮快的。")));
  assert.equal(found.length, 6, found.join("\n"));
  assert.match(details(await check("argus-wording", page("包括进程、服务……等。")))[0], /等/);
  assert.match(details(await check("argus-wording", page("真的吗？？完成。。")))[0], /标点|punctuation/);
  assert.match(details(await check("argus-wording", page("见 Codex 的说明。")))[0], /AGENTS\.md/);
});

test("argus-links: files, their case and anchors; relative links in docs; descriptive text", async () => {
  const dir = repository({
    "docs/a.md": "# 甲\n",
    "docs/b.md": "# 乙\n\n## 系统兼容范围\n\n## 设置（默认）\n\n<a id=\"custom\"></a>\n",
    "src/main.rs": "",
  });
  try {
    const run = (markdown) => check("argus-links", markdown, { root: dir }, join(dir, "docs", "a.md"));
    assert.deepEqual(await run("见[兼容范围](b.md#系统兼容范围)、[默认](b.md#%E8%AE%BE%E7%BD%AE%E9%BB%98%E8%AE%A4)、[自定义](b.md#custom)、[源码](../src/main.rs#L1)、[本页](#甲)与 [GitHub](https://github.com/Ray-lea/Argus/issues/1)。\n"), []);
    const cases = [
      ["[乙](b.md#不存在)", /锚点|anchor/],
      ["[丙](c.md)", /不存在|no such file/],
      ["[乙](B.md)", /大小写|case/],
      ["[乙](https://github.com/Ray-lea/Argus/blob/main/docs/b.md)", /相对路径|relative/],
      ["[这里](b.md)", /链接文字|link text/],
    ];
    for (const [link, pattern] of cases) assert.match(details(await run(`见${link}。\n`)).join("\n"), pattern, link);
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
});

test("anchors as GitHub and the website make them", () => {
  const anchors = anchorsOf(
    [
      "# 文档规范",
      "## 系统兼容范围",
      "## 设置（默认）",
      "## 重复",
      "## 重复",
      "## 🚀 快速开始",
      "## 🛠️ 构建",
      "## 使用 `pnpm docs:lint` 检查",
      "## [链接](x.md) 与 **加粗**",
      '<a id="custom"></a> <a name="legacy"></a>',
      "```markdown",
      "## 代码里的标题",
      "```",
    ].join("\n\n"),
  );
  for (const anchor of ["文档规范", "系统兼容范围", "设置默认", "重复", "重复-1", "-快速开始", "️-构建", "使用-pnpm-docslint-检查", "链接-与-加粗", "custom", "legacy"]) assert(anchors.has(anchor), anchor);
  assert(!anchors.has("代码里的标题"));
  assert.equal(headingText("C++ 与 `a  b` \\*"), "C++ 与 a  b *");
  assert.deepEqual(headings("标题\n===\n\n正文\n").map((heading) => heading.level), [1]);
  assert.equal(decodeFragment("%E7%B3%BB%E7%BB%9F"), "系统");
  assert.equal(decodeFragment("user-content-custom"), "custom");
});

test("references to docs/ from other files: paths, GitHub addresses and relative links", () => {
  const found = (text, path) => findReferences(text, { path, root: "/repo" }).map((reference) => [reference.kind, reference.target.fragment]);
  assert.deepEqual(found("见 [规范](../docs/x.md#y) 与 docs/a.md#b。", ".github/CONTRIBUTING.md"), [["link", "y"], ["path", "b"]]);
  assert.deepEqual(found("// docs/guide/settings.md#更新 and docs/x.md", "scripts/a.mjs"), [["path", "更新"], ["path", null]]);
  assert.deepEqual(found("see https://github.com/Ray-lea/Argus/blob/main/docs/x.md#%E7%B3%BB", "src/a.rs"), [["url", "%E7%B3%BB"]]);
  assert.deepEqual(found("website/content/docs/x.md and ../docs/x.md in code", "scripts/a.mjs"), []);
  assert.deepEqual(found("`[更新](settings.md#更新)`\n\n```\n[x](../docs/y.md)\n```\n", "docs/z.md"), [], "links in code are examples");
  assert.equal(maskCode("a `b` c").length, 7);

  const dir = repository({
    "docs/x.md": "# 标题\n\n## 系统兼容范围\n",
    "AGENTS.md": "见 `docs/x.md#系统兼容范围`、docs/x.md#%E7%B3%BB%E7%BB%9F%E5%85%BC%E5%AE%B9%E8%8C%83%E5%9B%B4 与 docs/x.md#不存在。\n",
    ".github/CONTRIBUTING.md": "见[规范](../docs/x.md#系统兼容范围)与[缺失](../docs/missing.md)。\n",
    "scripts/a.mjs": "// https://github.com/Ray-lea/Argus/blob/main/docs/x.md#标题\n",
    "image.png": "\u0000binary docs/missing.md",
  });
  try {
    const files = ["docs/x.md", "AGENTS.md", ".github/CONTRIBUTING.md", "scripts/a.mjs", "image.png"];
    const { scanned, problems } = checkReferences({ files, root: dir });
    assert.equal(scanned, 4, "binary files are skipped");
    assert.deepEqual(
      problems.map((problem) => [problem.path, problem.line, problem.problem.problem]),
      [
        ["AGENTS.md", 1, "anchor"],
        [".github/CONTRIBUTING.md", 1, "missing"],
      ],
    );
    assert.equal(checkReferences({ files, root: dir, skipLinks: new Set([".github/CONTRIBUTING.md"]) }).problems.length, 1, "argus-links checks the links of linted files");
    assert.equal(existence(join(dir, "DOCS", "x.md"), dir), "case");
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
});

test("AutoCorrect reads copies without fenced code, and --fix keeps the code as written", () => {
  const markdown = "1. 运行：\r\n\r\n   ```powershell\r\n   node a.mjs <EXE路径>\r\n   ```\r\n\r\n中文English\r\n";
  const { text, masked } = maskFences(markdown);
  assert(!text.includes("EXE路径") && text.includes("中文English"));
  assert.equal(text.split("\n").length, markdown.split("\n").length);
  const fixed = text.replace("中文English", "中文 English");
  assert.equal(unmask(markdown, fixed, masked), markdown.replace("中文English", "中文 English"));
  assert.equal(unmask(markdown, "one line", masked), null);
  assert(autocorrectSupported("win32", "x64") && !autocorrectSupported("win32", "arm64"));
});
