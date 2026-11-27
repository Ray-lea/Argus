// Checks the documents as docs/development/docs-style.md#检查工具 describes:
// markdownlint-cli2 with the repository's rules (.markdownlint-cli2.jsonc and
// scripts/lib/docs-lint), AutoCorrect for the Chinese typography
// (.autocorrectrc), and every reference to docs/ in the repository's files.
//
//   node scripts/docs-lint.mjs [--fix] [FILE…]      (pnpm docs:lint)
//
// Without FILE, every document in scope; with FILE, the named files that are
// in scope (scripts/lib/docs-lint/scope.mjs). The references to docs/ are
// always checked in full. --fix lets markdownlint and AutoCorrect rewrite
// what they can. Exits with 1 when a check reports problems.
import { spawnSync } from "node:child_process";
import { existsSync } from "node:fs";
import { resolve } from "node:path";
import { main as markdownlint } from "markdownlint-cli2";
import { t } from "./lib/console-language.mjs";
import { autocorrectSupported, runAutocorrect } from "./lib/docs-lint/autocorrect.mjs";
import { REPO_ROOT, repoPath } from "./lib/docs-lint/paths.mjs";
import { checkReferences } from "./lib/docs-lint/references.mjs";
import { targets } from "./lib/docs-lint/scope.mjs";
import { describe } from "./lib/docs-lint/targets.mjs";

const cwd = process.cwd();
const argv = process.argv.slice(2);
const unknown = argv.filter((arg) => arg.startsWith("-") && arg !== "--fix");
if (unknown.length) {
  console.error(t(`未知的选项：${unknown.join(" ")}\n用法：node scripts/docs-lint.mjs [--fix] [文件…]`, `unknown option: ${unknown.join(" ")}\nusage: node scripts/docs-lint.mjs [--fix] [FILE…]`));
  process.exit(2);
}
const fix = argv.includes("--fix");
const named = argv.filter((arg) => !arg.startsWith("-")).map((arg) => repoPath(resolve(cwd, arg)));
process.chdir(REPO_ROOT);

// Tracked files and new ones that are not ignored, as they are on disk.
const git = spawnSync("git", ["ls-files", "-z", "--cached", "--others", "--exclude-standard"], { encoding: "utf8", windowsHide: true, maxBuffer: 64 * 1024 * 1024 });
if (git.status !== 0) {
  console.error(t(`无法列出仓库中的文件：${git.stderr}`, `cannot list the repository's files: ${git.stderr}`));
  process.exit(2);
}
const all = [...new Set(git.stdout.split("\0").filter(Boolean))].filter((path) => existsSync(path)).sort();

for (const path of named.filter((path) => !existsSync(path))) console.log(t(`跳过不存在的文件：${path}`, `skipped, no such file: ${path}`));
const chosen = named.length ? named.filter((path) => existsSync(path)) : null;
const { markdownlint: linted, autocorrect: corrected, outside } = targets(all, chosen);
for (const path of outside) console.log(t(`不在文档检查的范围内：${path}`, `not among the checked documents: ${path}`));

const failed = [];

console.log(`\n=== markdownlint-cli2：${t(`${linted.length} 个文件`, `${linted.length} file(s)`)}`);
if (linted.length) {
  let reported = 0;
  const code = await markdownlint({
    directory: REPO_ROOT,
    argv: ["--no-globs", ...(fix ? ["--fix"] : []), ...linted.map((path) => `:${path}`)],
    // Under the configuration file, which does not set them: the banner and the
    // progress lines (which list every file) are left out.
    optionsDefault: { noBanner: true, noProgress: true },
    logMessage: (message) => console.log(message),
    logError: (message) => {
      reported++;
      console.error(message);
    },
  });
  if (code !== 0) failed.push("markdownlint-cli2");
  console.log(code === 0 ? t("通过", "passed") : t(`${reported} 处问题`, `${reported} problem(s)`));
}

console.log(`\n=== AutoCorrect：${t(`${corrected.length} 个文件`, `${corrected.length} file(s)`)}`);
if (corrected.length && !autocorrectSupported()) {
  console.log(t(`autocorrect-node 没有适用于 ${process.platform}-${process.arch} 的预编译包，跳过；拉取请求的检查会运行它`, `autocorrect-node has no prebuilt binding for ${process.platform}-${process.arch}: skipped here, pull request checks run it`));
} else if (corrected.length && runAutocorrect(corrected, { root: REPO_ROOT, fix }) !== 0) failed.push("AutoCorrect");

console.log(`\n=== ${t("指向 docs/ 的引用", "References to docs/")}`);
const { scanned, problems } = checkReferences({ files: all, skipLinks: new Set(linted) });
for (const problem of problems) console.error(`${problem.path}:${problem.line}:${problem.column} ${describe(problem.problem, t)}`);
console.log(problems.length ? t(`${scanned} 个文件中有 ${problems.length} 处问题`, `${problems.length} problem(s) in ${scanned} files`) : t(`${scanned} 个文件，通过`, `${scanned} files, passed`));
if (problems.length) failed.push(t("引用", "references"));

console.log(failed.length ? t(`\n未通过：${failed.join("、")}`, `\nfailed: ${failed.join(", ")}`) : t("\n文档检查通过", "\ndocuments pass"));
process.exit(failed.length ? 1 : 0);
