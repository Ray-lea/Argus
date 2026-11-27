import assert from "node:assert/strict";
import test from "node:test";
import { editionMetas, navigation, routeFor } from "../../website/scripts/content.mjs";

const files = [
  "docs/README.md",
  "docs/start/README.md",
  "docs/start/quick-start.md",
  "docs/guide/README.md",
  "docs/guide/settings.md",
  "docs/roadmap.md",
  "docs/known-issues.md",
];
const routes = Object.fromEntries(files.map((file) => [file, routeFor(file, "/docs/dev/")]));
const index = [
  "# Argus 文档",
  "",
  "## 入门",
  "",
  "- [快速入门](start/quick-start.md)：安装与首次启动",
  "- [什么是 Argus](start/README.md)",
  "",
  "## 使用指南",
  "",
  "- [界面与操作](guide/README.md)",
  "- [设置](guide/settings.md)",
  "",
  "## 其他",
  "",
  "- [路线图](roadmap.md)",
  "- [已知问题](known-issues.md)",
].join("\n");
const edition = { id: "dev", label: "开发版", note: "main", nav: navigation(index, routes) };

test("a chapter whose pages share a folder becomes a sidebar folder in the index's order", () => {
  const metas = Object.fromEntries(Object.entries(editionMetas(edition)).map(([path, json]) => [path, JSON.parse(json)]));
  assert.deepEqual(metas["start/meta.json"], {
    title: "入门",
    icon: "Compass",
    defaultOpen: true,
    // The folder's README is listed like any other page, after the quick start.
    pages: ["quick-start", "index"],
  });
  assert.deepEqual(metas["guide/meta.json"].pages, ["index", "settings"]);
  assert.equal(metas["guide/meta.json"].title, "使用指南");
});

test("pages at the top of docs/ stay under a separator in the root folder", () => {
  const root = JSON.parse(editionMetas(edition)["meta.json"]);
  assert.equal(root.root, true);
  assert.deepEqual(root.pages, ["index", "start", "guide", "---其他---", "roadmap", "known-issues"]);
});

test("a folder shared by two chapters is claimed by the first, the second keeps a separator", () => {
  const guide = Object.fromEntries(Object.entries(routes).filter(([file]) => file.startsWith("docs/guide/")));
  const shared = {
    ...edition,
    nav: navigation(
      ["## 使用指南", "", "- [界面与操作](guide/README.md)", "", "## 更多", "", "- [设置](guide/settings.md)"].join("\n"),
      guide,
    ),
  };
  const metas = editionMetas(shared);
  assert.deepEqual(JSON.parse(metas["guide/meta.json"]).pages, ["index"]);
  assert.deepEqual(JSON.parse(metas["meta.json"]).pages, ["index", "guide", "---更多---", "guide/settings"]);
});
