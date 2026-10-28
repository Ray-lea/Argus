// Collects the license texts of the third-party code shipped in argus.exe:
// the Rust crates linked into it and the npm packages bundled into the UI.
// Writes THIRD-PARTY-NOTICES.txt; `--check` only reports whether it is current.
// Needs the crate sources locally (any build or `cargo fetch` downloads them).
// Usage: node scripts/notices.mjs [--check]

import { execFileSync, execSync } from "node:child_process";
import { createHash } from "node:crypto";
import { existsSync, readdirSync, readFileSync, writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const root = join(dirname(fileURLToPath(import.meta.url)), "..");
const manifestRoot = root;
const OUT = "THIRD-PARTY-NOTICES.txt";
const TARGET = "x86_64-pc-windows-msvc";
const LICENSE_FILE = /^(licen[cs]e|copying|copyright|notice|unlicense)([-_.].*)?$/i;
const NOT_TEXT = /\.(rs|toml|json|js|ts|html|py|sh)$/i;

function fail(message) {
  console.error(`[错误] ${message}`);
  process.exit(1);
}

/** License texts shipped in a package directory, normalized. */
function licenseTexts(dir) {
  if (!existsSync(dir)) return [];
  return readdirSync(dir, { withFileTypes: true })
    .filter((e) => e.isFile() && LICENSE_FILE.test(e.name) && !NOT_TEXT.test(e.name))
    .map((e) => e.name)
    .sort()
    .map((name) =>
      readFileSync(join(dir, name), "utf8")
        .replace(/^﻿/, "")
        .replace(/\r\n?/g, "\n")
        .replace(/[ \t]+$/gm, "")
        .trim(),
    )
    .filter(Boolean);
}

/** Crates linked into argus.exe: normal dependencies, without proc macros (compile-time only). */
function crates() {
  let metadata;
  try {
    metadata = JSON.parse(
      execFileSync(
        "cargo",
        ["metadata", "--format-version", "1", "--locked", "--offline", "--filter-platform", TARGET],
        { cwd: manifestRoot, encoding: "utf8", maxBuffer: 256 * 1024 * 1024, stdio: ["ignore", "pipe", "pipe"] },
      ),
    );
  } catch (e) {
    fail(`cargo metadata 失败（需要本地已有依赖源码，先运行 cargo fetch）：${e.stderr ?? e.message}`);
  }
  const packages = new Map(metadata.packages.map((p) => [p.id, p]));
  const nodes = new Map(metadata.resolve.nodes.map((n) => [n.id, n]));
  const procMacro = (p) => p.targets.some((t) => t.kind.includes("proc-macro"));
  const names = ["argus"];
  const apps = metadata.packages.filter((p) => names.includes(p.name) && p.source === null);
  const seen = new Set();
  const stack = apps.map((app) => app.id);
  while (stack.length) {
    const id = stack.pop();
    if (seen.has(id)) continue;
    seen.add(id);
    for (const dep of nodes.get(id).deps) {
      if (dep.dep_kinds.some((k) => k.kind === null) && !procMacro(packages.get(dep.pkg)))
        stack.push(dep.pkg);
    }
  }
  return [...seen]
    .map((id) => packages.get(id))
    .filter((p) => p.source !== null || !metadata.workspace_members.includes(p.id))
    .map((p) => ({
      name: p.name,
      version: p.version,
      license: p.license ?? (p.license_file ? `见 ${p.license_file}` : "未声明"),
      url: p.repository ?? `https://crates.io/crates/${p.name}`,
      texts: licenseTexts(dirname(p.manifest_path)),
    }));
}

/** npm packages bundled into the UI: the production dependencies and theirs. */
function npmPackages() {
  // A command line, not an argument list: pnpm is a .cmd shim that needs a shell.
  const tree = JSON.parse(
    execSync("pnpm list --prod --json --depth Infinity", {
      cwd: root,
      encoding: "utf8",
      maxBuffer: 64 * 1024 * 1024,
    }),
  )[0];
  const found = new Map();
  const walk = (deps) => {
    for (const [name, dep] of Object.entries(deps ?? {})) {
      const key = `${name}@${dep.version}`;
      if (found.has(key)) continue;
      const manifest = JSON.parse(readFileSync(join(dep.path, "package.json"), "utf8"));
      // Type declarations only; nothing of them ends up in the bundle.
      if (!manifest.main && !manifest.module && !manifest.exports) continue;
      const repo = typeof manifest.repository === "string" ? manifest.repository : manifest.repository?.url;
      found.set(key, {
        name,
        version: dep.version,
        license: manifest.license ?? "未声明",
        url: (repo ?? `https://www.npmjs.com/package/${name}`).replace(/^git\+/, "").replace(/\.git$/, ""),
        texts: licenseTexts(dep.path),
      });
      walk(dep.dependencies);
    }
  };
  walk(tree.dependencies);
  return [...found.values()];
}

function vendored() {
  return ["pawnio", "cpu-layouts"].map((component) => {
    const dir = join(root, "crates/argus-service/vendor", component);
    const manifest = JSON.parse(readFileSync(join(dir, "manifest.json"), "utf8"));
    for (const [name, hash] of Object.entries(manifest.sha256)) {
      const actual = createHash("sha256").update(readFileSync(join(dir, name))).digest("hex");
      if (actual !== hash) fail(`${component} module/source hash mismatch: ${name}`);
    }
    return { ...manifest, texts: [manifest.copyright, ...licenseTexts(dir)] };
  });
}

function render(rust, npm, native) {
  const all = [...rust, ...npm, ...native];
  const byName = (a, b) => a.name.localeCompare(b.name, "en") || a.version.localeCompare(b.version, "en");
  const line = (p) => `  ${p.name} ${p.version}（${p.license}）${p.url}`;
  const rule = "=".repeat(80);
  const out = [
    "Argus 第三方组件许可声明",
    "",
    "Argus 的发布版本包含下列第三方组件。各组件按其各自的许可证授权；",
    "随组件附带的许可证文本收录于后，相同的文本只收录一次。",
    "",
    "Argus 静态链接了 Rust 标准库，其许可证为 MIT OR Apache-2.0，",
    "见 https://github.com/rust-lang/rust/blob/master/COPYRIGHT。",
    "",
    "本文件由 scripts/notices.mjs 生成，请勿手动编辑。",
    "",
    `Rust 组件（${rust.length} 个）`,
    ...rust.sort(byName).map(line),
    "",
    `界面组件（${npm.length} 个）`,
    ...npm.sort(byName).map(line),
    "",
    `内核传感器模块（${native.length} 个组件）`,
    ...native.sort(byName).map(line),
  ];

  // Most texts are the same license with a different copyright line. Each
  // license body appears once, preceded by every copyright line that came
  // with it, so no notice is lost.
  // A notice line ("Copyright (c) 2019 …", "Copyright 2015-2020 …"), not prose
  // such as Apache's wrapped "copyright notice that is included in …".
  const COPYRIGHT = /^\s*(copyright\s*(\(c\)|©|\d{4})|\(c\)\s*\d{4}|©\s*\d{4})/i;
  const groups = new Map();
  for (const p of all) {
    for (const text of p.texts) {
      const lines = text.split("\n");
      const body = lines
        .filter((l) => !COPYRIGHT.test(l))
        .join("\n")
        .replace(/\n{3,}/g, "\n\n")
        .trim();
      const key = body.replace(/\s+/g, " ");
      const group = groups.get(key) ?? { body, users: new Set(), copyrights: new Set() };
      group.users.add(`${p.name} ${p.version}`);
      for (const l of lines.filter((l) => COPYRIGHT.test(l))) group.copyrights.add(l.trim());
      groups.set(key, group);
    }
  }
  const sorted = [...groups.values()].sort(
    (a, b) => b.users.size - a.users.size || a.body.localeCompare(b.body, "en"),
  );
  for (const { body, users, copyrights } of sorted) {
    out.push("", rule, `适用于：${[...users].sort((a, b) => a.localeCompare(b, "en")).join("、")}`, rule, "");
    if (copyrights.size) out.push(...[...copyrights].sort((a, b) => a.localeCompare(b, "en")), "");
    out.push(body);
  }

  const missing = all.filter((p) => p.texts.length === 0);
  if (missing.length) {
    out.push(
      "",
      rule,
      "下列组件未附带许可证文本，按其声明的许可证授权（许可证全文见 https://spdx.org/licenses/）：",
      rule,
      "",
      ...missing.sort(byName).map(line),
    );
  }
  return `${out.join("\n")}\n`;
}

const text = render(crates(), npmPackages(), [...vendored(),{name:"Native installer templates (Tauri-derived)",version:"2.12.3",license:"MIT OR Apache-2.0",url:"https://github.com/tauri-apps/tauri",texts:[readFileSync(join(root,"packaging/windows/LICENSE-TAURI-MIT"),"utf8"),readFileSync(join(root,"packaging/windows/LICENSE-TAURI-APACHE"),"utf8")]}]);
const path = join(root, OUT);
if (process.argv.includes("--check")) {
  if (!existsSync(path) || readFileSync(path, "utf8") !== text) {
    fail(`${OUT} 与当前依赖不符，运行 node scripts/notices.mjs 重新生成后提交`);
  }
  console.log(`[通过] ${OUT} 与当前依赖一致`);
} else {
  writeFileSync(path, text);
  const kb = Math.round(Buffer.byteLength(text) / 1024);
  console.log(`已写入 ${OUT}（${kb} KB）`);
}
