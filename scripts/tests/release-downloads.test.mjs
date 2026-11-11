import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import { copyFile, mkdir, mkdtemp, readFile, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { basename, dirname, join, resolve } from "node:path";
import test from "node:test";

async function fixture(t, version, translated = true) {
  const root = resolve(await mkdtemp(join(tmpdir(), "argus-release-downloads-")));
  t.after(async () => {
    assert.equal(dirname(root), resolve(tmpdir()));
    assert(basename(root).startsWith("argus-release-downloads-"));
    await rm(root, { recursive: true, force: true });
  });
  const repository = resolve(import.meta.dirname, "../..");
  await mkdir(join(root, "scripts/lib"), { recursive: true });
  for (const file of ["scripts/release.mjs", "scripts/lib/release-notes.ts", "scripts/lib/release-downloads.md"])
    await copyFile(join(repository, file), join(root, file));
  const zh = `# 更新日志\n\n## ${version} - 2026-11-10\n\n### 修复\n\n- 版本变更示例。\n`;
  const en = translated ? `# Changelog\n\n## ${version} - 2026-11-10\n\n### Fixes\n\n- Example release change.\n` : "# Changelog\n";
  await writeFile(join(root, "CHANGELOG.md"), zh);
  await writeFile(join(root, "CHANGELOG.en.md"), en);
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
    assert(body.includes("不支持应用内安装更新"));
    assert(body.includes("in-app update installation"));
    assert(!body.includes("{{version}}"));
    assert(body.indexOf("## 下载文件说明") > body.indexOf("- Example release change."));
    assert.equal(await readFile(join(f.root, "CHANGELOG.md"), "utf8"), f.zh);
    assert.equal(await readFile(join(f.root, "CHANGELOG.en.md"), "utf8"), f.en);
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
