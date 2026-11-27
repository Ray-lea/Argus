import assert from "node:assert/strict";
import test from "node:test";
import { contentFile, editionMetas, folderPages, navigation, routeFor } from "../../website/scripts/content.mjs";

const files = [
  "docs/README.md",
  "docs/start/README.md",
  "docs/start/quick-start.md",
  "docs/guide/README.md",
  "docs/guide/settings.md",
  "docs/guide/settings/permissions.md",
  "docs/guide/settings/updates.md",
  "docs/roadmap.md",
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
  "- [设置](guide/settings.md)：各设置分组",
  "  - [权限](guide/settings/permissions.md)",
  "  - [更新](guide/settings/updates.md)",
  "",
  "## 其他",
  "",
  "- [路线图](roadmap.md)",
].join("\n");
const nav = navigation(index, routes);
const metas = Object.fromEntries(
  Object.entries(editionMetas({ id: "dev", label: "开发版", note: "main", nav })).map(([path, json]) => [path, JSON.parse(json)]),
);

test("each chapter of the index is a section under a separator, in the index's order", () => {
  assert.equal(metas["meta.json"].root, true);
  assert.deepEqual(metas["meta.json"].pages, [
    "index",
    "---入门---",
    "start/quick-start",
    "start/index",
    "---使用指南---",
    "guide/index",
    "guide/settings",
    "---其他---",
    "roadmap",
  ]);
});

test("a page with pages indented under it opens a folder of them", () => {
  const settings = nav[1].items[1];
  assert.deepEqual(settings.children.map((child) => child.path), [
    "/docs/dev/guide/settings/permissions/",
    "/docs/dev/guide/settings/updates/",
  ]);
  assert.deepEqual(metas["guide/settings/meta.json"], { title: "设置", pages: ["permissions", "updates"] });
  assert.deepEqual([...folderPages(nav)], ["docs/guide/settings.md"]);
  // The page becomes the folder's index, at the same address.
  assert.equal(contentFile("docs/guide/settings.md", "dev", true), "docs/dev/guide/settings/index.md");
  assert.equal(routes["docs/guide/settings.md"], "/docs/dev/guide/settings/");
});

test("an indented item outside its parent's folder stays a page of the chapter", () => {
  const stray = navigation(
    ["## 使用指南", "", "- [设置](guide/settings.md)", "  - [路线图](roadmap.md)"].join("\n"),
    routes,
  );
  assert.deepEqual(stray[0].items.map((item) => item.file), ["docs/guide/settings.md", "docs/roadmap.md"]);
  assert.equal(stray[0].items[0].children, undefined);
});
