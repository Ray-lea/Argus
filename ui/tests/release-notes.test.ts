// Release notes as the update dialog reads them (ui/src/lib/release-notes.ts):
// entries in the format scripts/release.mjs writes and in hand-written ones.

import assert from "node:assert/strict";
import { test } from "node:test";
import {
  between,
  categoryOf,
  changeCount,
  changelogEntries,
  compareVersions,
  firstStart,
  isPrerelease,
  parseNotes,
  spans,
  strayHeadings,
} from "../src/lib/release-notes.ts";

test("versions follow SemVer precedence", () => {
  const ordered = [
    "0.1.0",
    "0.2.0-alpha",
    "0.2.0-beta.2",
    "0.2.0-beta.10",
    "0.2.0-rc.1",
    "0.2.0",
    "0.10.0",
    "1.0.0",
  ];
  for (let i = 1; i < ordered.length; i++) {
    assert.ok(compareVersions(ordered[i - 1], ordered[i]) < 0, `${ordered[i - 1]} < ${ordered[i]}`);
    assert.ok(compareVersions(ordered[i], ordered[i - 1]) > 0, `${ordered[i]} > ${ordered[i - 1]}`);
  }
  assert.equal(compareVersions("v1.2.3", "1.2.3"), 0);
  assert.equal(compareVersions("1.2.3+build.5", "1.2.3"), 0);
  assert.equal(compareVersions("dev", "1.2.3"), 0);
  assert.ok(isPrerelease("0.2.0-beta.1"));
  assert.ok(!isPrerelease("0.2.0"));
});

test("an update shows every release after the current one", () => {
  const releases = ["0.3.0", "0.2.1", "0.2.0", "0.2.0-beta.1", "0.1.0"].map((version) => ({
    version,
    date: "2026-10-20",
    notes: "",
  }));
  const versions = (from: string | null, to: string) => between(releases, from, to).map((r) => r.version);
  assert.deepEqual(versions("0.1.0", "0.3.0"), ["0.3.0", "0.2.1", "0.2.0", "0.2.0-beta.1"]);
  assert.deepEqual(versions("0.2.0", "0.2.1"), ["0.2.1"]);
  assert.deepEqual(versions("0.2.0-beta.1", "0.2.0"), ["0.2.0"]);
  assert.deepEqual(versions(null, "0.2.0"), ["0.2.0"]);
  assert.deepEqual(versions("0.3.0", "0.3.0"), []);
});

test("what changed shows on the first start of a newer version only", () => {
  // Shown once: the version is recorded, the next start finds it.
  assert.deepEqual(firstStart("0.2.0", "0.1.0", true, false), { kind: "updated", from: "0.1.0" });
  assert.deepEqual(firstStart("0.2.0", "0.2.0", true, true), { kind: "seen" });
  assert.deepEqual(firstStart("0.2.0", "0.2.0-beta.1", true, false), {
    kind: "updated",
    from: "0.2.0-beta.1",
  });
  // A fresh install has nothing to show; a downgrade neither.
  assert.deepEqual(firstStart("0.2.0", "", false, false), { kind: "record" });
  assert.deepEqual(firstStart("0.1.0", "0.2.0", true, false), { kind: "record" });
  // From a version that kept no record: an onboarding done, or the installer's relaunch.
  assert.deepEqual(firstStart("0.2.0", "", true, false), { kind: "updated", from: null });
  assert.deepEqual(firstStart("0.2.0", "", false, true), { kind: "updated", from: null });
});

test("changelog entries split at their headings", () => {
  const changelog = [
    "# 更新日志",
    "",
    "Intro with a [link](https://example.org).",
    "",
    "## [0.2.0](https://github.com/o/r/compare/v0.1.0...v0.2.0) - 2026-10-20",
    "",
    "### 修复",
    "",
    "- Fixed",
    "",
    "## 0.1.0 - 2026-10-16",
    "",
    "First.",
    "",
  ].join("\r\n");
  const entries = changelogEntries(changelog);
  assert.deepEqual(
    entries.map((e) => [e.version, e.date]),
    [
      ["0.2.0", "2026-10-20"],
      ["0.1.0", "2026-10-16"],
    ],
  );
  assert.equal(entries[0].notes, "### 修复\n\n- Fixed");
  assert.equal(entries[1].notes, "First.");
  assert.deepEqual(strayHeadings(changelog), []);
});

// scripts/release.mjs reads CHANGELOG.md with these functions and refuses stray
// headings before the tag is pushed.
test("a reworded entry heading is no entry and is reported", () => {
  const link = "## [0.2.0](https://github.com/o/r/compare/v0.1.0...v0.2.0)";
  const changelog = (heading: string) =>
    [
      "# 更新日志",
      "",
      heading,
      "",
      "### 修复",
      "",
      "- Fixed",
      "",
      "## 0.1.0 - 2026-10-16",
      "",
      "First.",
    ].join("\n");
  const versions = (heading: string) => changelogEntries(changelog(heading)).map((e) => e.version);
  // Spelled out: Biome writes escapes as the characters themselves.
  const nbsp = String.fromCodePoint(0xa0);
  const ideographicSpace = String.fromCodePoint(0x3000);

  for (const heading of [`${link} - 2026-10-20`, `${link} - 2026-10-20 \t`, "## 0.2.0 - 2026-10-20"]) {
    assert.deepEqual(versions(heading), ["0.2.0", "0.1.0"], heading);
    assert.deepEqual(strayHeadings(changelog(heading)), [], heading);
  }
  for (const heading of [
    `${link} - 2026-10-20（预览）`,
    `${link} - 2026/10/20`,
    `${link} – 2026-10-20`,
    `${link} - 2026-10-20${nbsp}`,
    `${link} - 2026-10-20${ideographicSpace}`,
    ` ${link} - 2026-10-20`,
  ]) {
    assert.deepEqual(versions(heading), ["0.1.0"], heading);
    assert.deepEqual(strayHeadings(changelog(heading)), [{ line: 3, text: heading }], heading);
  }
  // A heading of another version: release.mjs finds no entry for 0.2.0.
  assert.deepEqual(versions("## [v0.2.0](link) - 2026-10-20"), ["v0.2.0", "0.1.0"]);
});

test("a second-level heading inside an entry is reported", () => {
  const changelog = "## 0.2.0 - 2026-10-20\r\n\r\n## 亮点\r\n\r\n- One\r\n\r\n### 修复\r\n\r\n##\r\n";
  // Part of the entry, but no different from a reworded entry heading, so
  // release.mjs asks for `###`.
  assert.equal(changelogEntries(changelog)[0].notes, "## 亮点\n\n- One\n\n### 修复\n\n##");
  assert.deepEqual(strayHeadings(changelog), [
    { line: 3, text: "## 亮点" },
    { line: 9, text: "##" },
  ]);
  assert.deepEqual(strayHeadings("# 更新日志\n### 修复\n##亮点\n    ## code"), []);
});

test("generated entries lose their commits and scopes", () => {
  const blocks = parseNotes(
    [
      "### 新功能",
      "",
      "- **ui:** 进程页支持按列筛选 ([abc1234](https://github.com/o/r/commit/abc1234ffff))",
      "- **core,host:** 采集每个磁盘的队列长度 (def5678)",
      "",
      "### 修复",
      "",
      "- 托盘菜单偶尔不响应 ([0a1b2c3](https://github.com/o/r/commit/0a1b2c3))",
      "",
      "### 性能",
      "",
    ].join("\n"),
  );
  assert.deepEqual(blocks, [
    {
      kind: "group",
      category: "feat",
      title: "新功能",
      items: [[{ text: "进程页支持按列筛选" }], [{ text: "采集每个磁盘的队列长度" }]],
    },
    { kind: "group", category: "fix", title: "修复", items: [[{ text: "托盘菜单偶尔不响应" }]] },
  ]);
  assert.equal(changeCount(blocks), 3);
});

test("hand-written entries keep their leads, paragraphs and code", () => {
  const blocks = parseNotes(
    [
      "Argus 的首个公开版本。",
      "",
      "### 功能",
      "",
      "- **进程**：按应用查看，",
      "  可结束进程树。",
      "",
      "### 已知限制",
      "",
      "- 支持范围见[硬件数据](https://github.com/o/r/blob/main/docs/x.md#y)，运行 `start.bat`。",
    ].join("\n"),
  );
  assert.deepEqual(blocks[0], { kind: "text", spans: [{ text: "Argus 的首个公开版本。" }] });
  assert.deepEqual(blocks[1], {
    kind: "group",
    category: "feat",
    title: "功能",
    items: [[{ text: "进程", strong: true }, { text: "：按应用查看， 可结束进程树。" }]],
  });
  assert.deepEqual(blocks[2], {
    kind: "group",
    category: "known",
    title: "已知限制",
    items: [[{ text: "支持范围见硬件数据，运行 " }, { text: "start.bat", code: true }, { text: "。" }]],
  });
});

test("the count leaves out known limitations", () => {
  const count = (lines: string[]) => changeCount(parseNotes(lines.join("\n")));
  // As 0.1.0 is written: the limitations are no changes.
  assert.equal(
    count([
      "Argus 的首个公开版本。",
      "",
      "### 功能",
      "",
      "- One",
      "- Two",
      "",
      "### 已知限制",
      "",
      "- Unsigned",
    ]),
    2,
  );
  // As release.mjs writes a `feat!` commit: listed under both headings, counted twice.
  assert.equal(
    count([
      "### 破坏性变更",
      "",
      "- 特权服务需随新版更新 ([e3cc63f](https://github.com/o/r/commit/e3cc63f))",
      "",
      "### 新功能",
      "",
      "- 进程页显示磁盘读写速率 ([e3cc63f](https://github.com/o/r/commit/e3cc63f))",
      "",
      "### 修复",
      "",
      "- Fixed (0a1b2c3)",
      "",
      "### 性能",
      "",
      "- Faster (1a2b3c4)",
      "",
      "### 回退",
      "",
      "- Reverted (2b3c4d5)",
    ]),
    5,
  );
  // Commits before the convention, lists without a heading and headings written by hand.
  assert.equal(count(["### 其他", "", "- Old commit (3c4d5e6)"]), 1);
  assert.equal(count(["- One", "- Two"]), 2);
  assert.equal(count(["### 界面", "", "- One", "", "### Known issues", "", "- Two"]), 1);
  assert.equal(count(["只有内部改动（重构、文档、构建等），没有用户可见的变化。"]), 0);
  assert.equal(count(["### 破坏性变更", "", "- 需重新安装特权服务"]), 1);
});

test("lists without a heading and unknown headings are kept", () => {
  assert.deepEqual(parseNotes("- One\n- Two"), [
    { kind: "group", category: "other", title: "", items: [[{ text: "One" }], [{ text: "Two" }]] },
  ]);
  assert.equal(categoryOf("Breaking changes"), "breaking");
  assert.equal(categoryOf("  修复 "), "fix");
  assert.equal(categoryOf("Something else"), "other");
  assert.deepEqual(spans("a **b** [c](u) `d`"), [
    { text: "a " },
    { text: "b", strong: true },
    { text: " c " },
    { text: "d", code: true },
  ]);
});
