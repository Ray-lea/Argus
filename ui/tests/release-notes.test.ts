// Release notes as the update dialog reads them (ui/src/lib/release-notes.ts):
// entries in the format scripts/release.mjs writes and in hand-written ones.

import assert from "node:assert/strict";
import { test } from "node:test";
import {
  between,
  categoryOf,
  changelogEntries,
  compareVersions,
  isPrerelease,
  itemCount,
  parseNotes,
  spans,
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
  assert.equal(itemCount(blocks), 3);
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
