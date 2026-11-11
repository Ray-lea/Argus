import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import { copyFile, mkdir, mkdtemp, readFile, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { basename, dirname, join, resolve } from "node:path";
import test from "node:test";

async function fixture(t, version, translated = true, others = {}) {
  const root = resolve(await mkdtemp(join(tmpdir(), "argus-release-downloads-")));
  t.after(async () => {
    assert.equal(dirname(root), resolve(tmpdir()));
    assert(basename(root).startsWith("argus-release-downloads-"));
    await rm(root, { recursive: true, force: true });
  });
  const repository = resolve(import.meta.dirname, "../..");
  for (const dir of ["scripts/lib", "resources", "changelog"]) await mkdir(join(root, dir), { recursive: true });
  for (const file of ["scripts/release.mjs", "scripts/lib/release-notes.ts", "scripts/lib/release-downloads.md", "resources/changelog.json"])
    await copyFile(join(repository, file), join(root, file));
  const zh = `# 更新日志\n\n## ${version} - 2026-11-10\n\n### 修复\n\n- 版本变更示例。\n`;
  const en = translated ? `# Changelog\n\n## ${version} - 2026-11-10\n\n### Fixes\n\n- Example release change.\n` : "# Changelog\n";
  await writeFile(join(root, "changelog/zh-CN.md"), zh);
  await writeFile(join(root, "changelog/en-US.md"), en);
  for (const [locale, text] of Object.entries(others)) await writeFile(join(root, `changelog/${locale}.md`), text);
  const run = (...args) => execFileSync(process.execPath, [join(root, "scripts/release.mjs"), ...args], {
    cwd: root, encoding: "utf8", windowsHide: true,
  });
  return { root, zh, en, run };
}

test("stable and prerelease GitHub notes include both artifact names and the bilingual guide", async t => {
  for (const version of ["1.2.3", "1.3.0-beta.2"]) {
    const f = await fixture(t, version);
    const body = f.run("notes", version);
    assert(body.includes("- 版本变更示例。"));
    assert(body.includes("- Example release change."));
    assert.equal(body.split("## 下载文件说明 / Download files").length - 1, 1);
    for (const file of [`Argus_${version}_x64-setup.exe`, `Argus_${version}_x64.exe`])
      assert.equal(body.split(file).length - 1, 2, file);
    assert(body.includes("安装版（推荐）"));
    assert(body.includes("Installer (recommended)"));
    assert(body.includes("需手动下载并替换 EXE"));
    assert(body.includes("download and replace the EXE manually"));
    assert(!body.includes("{{version}}"));
    // A row with a missing or extra cell breaks the comparison on the release page.
    const rows = body.split("\n").filter((line) => line.startsWith("|"));
    assert.equal(rows.length, 14);
    for (const row of rows) assert.equal(row.split("|").length, 5, row);
    assert(body.indexOf("## 下载文件说明") > body.indexOf("- Example release change."));
    assert.equal(await readFile(join(f.root, "changelog/zh-CN.md"), "utf8"), f.zh);
    assert.equal(await readFile(join(f.root, "changelog/en-US.md"), "utf8"), f.en);
    // Without other languages there is nothing to link.
    assert(body.startsWith("## 简体中文\n"));
  }
});

test("the guide stays bilingual when an older release has no translated changes", async t => {
  const f = await fixture(t, "0.1.0", false);
  const body = f.run("notes", "0.1.0");
  assert(body.includes("### 简体中文"));
  assert(body.includes("### English"));
  assert(body.includes("Argus_0.1.0_x64-setup.exe"));
});

test("the GitHub download guide does not enter signed in-app release notes", async t => {
  const f = await fixture(t, "1.2.3");
  f.run("release-notes", "1.2.3", "notes.json");
  const notes = JSON.parse(await readFile(join(f.root, "notes.json"), "utf8"));
  assert.equal(notes.releases.length, 1);
  assert.equal(notes.releases[0].notes, "### 修复\n\n- 版本变更示例。");
  assert.equal(notes.releases[0].translations["en-US"], "### Fixes\n\n- Example release change.");
});

const german = "# Änderungsprotokoll\n\n## 1.2.3 - 2026-11-10\n\n### Fehlerbehebungen\n\n- Beispielhafte Änderung.\n";

test("other languages are linked at the top of the GitHub notes, as in the READMEs", async t => {
  const ja = "# 更新履歴\n\n## 1.2.3 - 2026-11-10\n\n### 修正\n\n- 変更の例です。\n";
  const f = await fixture(t, "1.2.3", true, { "de-DE": german, "ja-JP": ja });
  const [first, blank, ...rest] = f.run("notes", "1.2.3").split("\n");
  assert.equal(
    first,
    "**简体中文** · **English** · [Deutsch](changelog/de-DE.md#123---2026-11-10) · [日本語](changelog/ja-JP.md#123---2026-11-10)",
  );
  assert.equal(blank, "");
  const body = rest.join("\n");
  assert(body.startsWith("## 简体中文\n"));
  assert(!body.includes("Beispielhafte"));
  assert(!body.includes("変更の例"));
});

test("signed in-app notes carry every finished translation and leave drafts out", async t => {
  // A draft as pnpm release writes it: the heading translated, the item not yet.
  const ja = "# 更新履歴\n\n## 1.2.3 - 2026-11-10\n\n### 修正\n\n- 版本变更示例。\n";
  const f = await fixture(t, "1.2.3", true, { "de-DE": german, "ja-JP": ja });
  f.run("release-notes", "1.2.3", "notes.json");
  const notes = JSON.parse(await readFile(join(f.root, "notes.json"), "utf8"));
  assert.deepEqual(Object.keys(notes.releases[0].translations).sort(), ["de-DE", "en-US"]);
  assert.equal(notes.releases[0].translations["de-DE"], "### Fehlerbehebungen\n\n- Beispielhafte Änderung.");
});
