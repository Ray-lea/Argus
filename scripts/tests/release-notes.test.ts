// Changelog entries as scripts/release.mjs reads them (scripts/lib/release-notes.ts):
// entries in the format release.mjs writes and in hand-written ones.

import assert from "node:assert/strict";
import { test } from "node:test";
import { changelogEntries, strayHeadings, withTranslation } from "../lib/release-notes.ts";

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

test("an entry takes the translation of the same version", () => {
  const zh = changelogEntries(
    [
      "# 更新日志",
      "",
      "## [1.1.0](x) - 2026-10-21",
      "",
      "### 修复",
      "",
      "- 一处修复",
      "",
      "## 1.0.0 - 2026-10-01",
      "",
      "首个版本。",
    ].join("\n"),
  );
  const en = changelogEntries(
    ["# Changelog", "", "## [1.1.0](x) - 2026-10-21", "", "### Fixes", "", "- A fix"].join("\n"),
  );
  const english = "### Fixes\n\n- A fix";
  const [latest, first] = withTranslation(zh, "en-US", en);
  assert.deepEqual(latest.translations, { "en-US": english });
  assert.equal(first.translations, undefined);
});
