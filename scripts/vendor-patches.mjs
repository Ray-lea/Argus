// The local patches of vendored GPUI, as diffs against the packages they were
// taken from. The local branch `vendor-upstream` holds the published packages
// of each upstream version, laid out as vendor/ is and without the files the
// snapshots leave out (scripts/lib/vendor-patches.mjs), so that
// `git diff vendor-upstream -- vendor/` is the whole patch set.
//
//   node scripts/vendor-patches.mjs init [--force]
//   node scripts/vendor-patches.mjs stat [--rev <commit>] [--upstream <ref>] [--limit N] [--json] [<path>…]
//   node scripts/vendor-patches.mjs diff [--rev <commit>] [--upstream <ref>] [<git diff option>…] [<path>…]
//   node scripts/vendor-patches.mjs import <version>
//   node scripts/vendor-patches.mjs upgrade <version>
//
// `init` creates the branch from the versions vendor/ is at, `import` adds a
// version on top of it. `stat` lists the added and removed lines of every
// vendored file against the upstream version the files are at (by their
// manifests), `diff` prints the patches; both compare the working tree, or
// <commit> with --rev. Paths are relative to vendor/ or to the repository.
// `upgrade` imports <version> and merges the patches into it in the working
// tree: files without patches take the new upstream file, patched files are
// merged three ways, and what needs a decision is listed. The packages come
// from Cargo's registry cache; a version not there is fetched with
// `cargo fetch` in a temporary project.
import { spawnSync } from "node:child_process";
import { createHash } from "node:crypto";
import { existsSync, mkdirSync, mkdtempSync, readdirSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { homedir, tmpdir } from "node:os";
import { dirname, isAbsolute, join, relative, resolve } from "node:path";
import { t } from "./lib/console-language.mjs";
import { formatRows, manifestVersion, mergePlan, packageFiles, parseDiffStat, sortRows, totals, UPSTREAM_BRANCH, VENDORED } from "./lib/vendor-patches.mjs";

const root = resolve(import.meta.dirname, "..");
const ref = `refs/heads/${UPSTREAM_BRANCH}`;

class Usage extends Error {}
const fail = (zh, en) => {
  throw new Usage(t(zh, en));
};

function git(args, { input, env, buffer = false } = {}) {
  const result = spawnSync("git", args, {
    cwd: root,
    input,
    env: env ? { ...process.env, ...env } : undefined,
    encoding: buffer ? "buffer" : "utf8",
    maxBuffer: 1024 ** 3,
  });
  if (result.error) throw result.error;
  if (result.status !== 0) throw new Error(`git ${args.slice(0, 2).join(" ")}: ${String(result.stderr).trim()}`);
  return result.stdout;
}
const tryGit = (args) => {
  const result = spawnSync("git", args, { cwd: root, encoding: "utf8" });
  return result.status === 0 ? result.stdout : null;
};

// Arguments: options with a value, flags, and the rest.
function parse(argv, valued, flags) {
  const options = {};
  const rest = [];
  for (let i = 0; i < argv.length; i++) {
    const arg = argv[i];
    if (valued.includes(arg)) {
      if (i + 1 >= argv.length) fail(`${arg} 缺少取值`, `${arg} needs a value`);
      options[arg.slice(2)] = argv[++i];
    } else if (flags.includes(arg)) options[arg.slice(2)] = true;
    else rest.push(arg);
  }
  return { options, rest };
}

// The vendored packages, without vendor/'s own files (AGENTS.md).
const PACKAGES = VENDORED.map((spec) => `vendor/${spec.name}/`);

/** Path arguments as pathspecs under vendor/; all vendored packages without any. */
function vendorPaths(args) {
  if (!args.length) return PACKAGES;
  return args.map((arg) => {
    const path = (isAbsolute(arg) ? relative(root, arg) : arg).replaceAll("\\", "/").replace(/^\.\//, "").replace(/\/$/, "");
    return path === "vendor" ? PACKAGES : path.startsWith("vendor/") ? path : `vendor/${path}`;
  }).flat();
}

/** The upstream versions the vendored manifests are at: in the working tree, or at `rev`. */
function vendoredVersions(rev) {
  const versions = {};
  for (const spec of VENDORED) {
    const path = `vendor/${spec.name}/Cargo.toml`;
    const manifest = rev ? tryGit(["show", `${rev}:${path}`]) : existsSync(join(root, path)) ? readFileSync(join(root, path), "utf8") : null;
    const version = manifest && manifestVersion(manifest);
    if (!version) fail(`${rev ?? "工作区"} 的 ${path} 中没有版本号`, `${path} has no version in ${rev ?? "the working tree"}`);
    versions[spec.name] = version;
  }
  return versions;
}

const describe = (versions) => VENDORED.map((spec) => `${spec.name} ${versions[spec.name]}`).join(", ");
const same = (a, b) => VENDORED.every((spec) => a[spec.name] === b[spec.name]);
const branchTip = () => tryGit(["rev-parse", "--verify", "--quiet", `${ref}^{commit}`])?.trim() ?? null;
const requireBranch = () =>
  branchTip() ??
  fail(
    `没有本地分支 ${UPSTREAM_BRANCH}：先运行 node scripts/vendor-patches.mjs init`,
    `there is no local branch ${UPSTREAM_BRANCH}: run node scripts/vendor-patches.mjs init first`,
  );

/** The commit of `vendor-upstream` (first parents from its tip) holding `versions`. */
function upstreamCommit(versions) {
  requireBranch();
  for (const commit of git(["rev-list", "--first-parent", ref]).split(/\s+/).filter(Boolean)) if (same(vendoredVersions(commit), versions)) return commit;
  return fail(
    `${UPSTREAM_BRANCH} 中没有 ${describe(versions)}：用 import 加入该版本`,
    `${UPSTREAM_BRANCH} has no commit with ${describe(versions)}: add it with import`,
  );
}

/** The published package `<name>-<version>.crate` in Cargo's registry cache, or null. */
function cachedCrate(name, version) {
  const cache = join(process.env.CARGO_HOME || join(homedir(), ".cargo"), "registry", "cache");
  if (!existsSync(cache)) return null;
  for (const registry of readdirSync(cache).sort()) {
    const path = join(cache, registry, `${name}-${version}.crate`);
    if (existsSync(path)) return path;
  }
  return null;
}

// Cargo downloads (and checks against the index) what a throwaway project
// depending on exactly these versions needs.
function fetchCrates(version) {
  const project = mkdtempSync(join(tmpdir(), "argus-vendor-fetch-"));
  try {
    const dependencies = VENDORED.map((spec) => `${spec.name} = "=${version}"`).join("\n");
    writeFileSync(join(project, "Cargo.toml"), `[package]\nname = "vendor-fetch"\nversion = "0.0.0"\nedition = "2024"\npublish = false\n\n[dependencies]\n${dependencies}\n\n[workspace]\n`);
    mkdirSync(join(project, "src"));
    writeFileSync(join(project, "src", "lib.rs"), "");
    console.log(t(`从 crates.io 获取 ${version} 版的包…`, `fetching the ${version} packages from crates.io…`));
    const result = spawnSync("cargo", ["fetch"], { cwd: project, stdio: "inherit" });
    if (result.status !== 0) fail("cargo fetch 失败", "cargo fetch failed");
  } finally {
    rmSync(project, { recursive: true, force: true });
  }
}

/** Commits the upstream packages of `version` (one for every vendored package) on top of `parent`. */
function commitUpstream(version, parent) {
  if (VENDORED.some((spec) => !cachedCrate(spec.name, version))) fetchCrates(version);
  const index = join(mkdtempSync(join(tmpdir(), "argus-vendor-index-")), "index");
  const env = { GIT_INDEX_FILE: index };
  const lines = [];
  const sums = [];
  try {
    for (const spec of VENDORED) {
      const path = cachedCrate(spec.name, version) ?? fail(`找不到 ${spec.name} ${version} 的包`, `no package for ${spec.name} ${version}`);
      const crate = readFileSync(path);
      sums.push(`${spec.name} ${version}: sha256 ${createHash("sha256").update(crate).digest("hex")}`);
      const files = packageFiles(crate, spec, version);
      for (const file of files) {
        // Hashed as if checked in at its vendored path, so the same attributes (line endings) apply.
        const target = `vendor/${spec.name}/${file.path}`;
        const blob = git(["hash-object", "-w", "--stdin", "--path", target], { input: file.data }).trim();
        lines.push(`100644 ${blob}\t${target}`);
      }
      console.log(`${spec.name} ${version}: ${t(`${files.length} 个文件`, `${files.length} files`)}`);
    }
    git(["update-index", "--add", "--index-info"], { input: `${lines.join("\n")}\n`, env });
    const tree = git(["write-tree"], { env }).trim();
    const message = [
      VENDORED.map((spec) => `${spec.name} ${version}`).join(", "),
      "",
      "The packages as published on crates.io, without the files the vendored",
      "snapshots leave out (scripts/lib/vendor-patches.mjs).",
      "",
      ...sums,
      "",
    ].join("\n");
    return git(["commit-tree", tree, ...(parent ? ["-p", parent] : []), "-F", "-"], { input: message }).trim();
  } finally {
    rmSync(dirname(index), { recursive: true, force: true });
  }
}

/** Blob ids by path of the vendored packages at `commit`. */
function blobs(commit) {
  const map = new Map();
  const listing = git(["ls-tree", "-r", "-z", "--full-tree", commit, "--", ...VENDORED.map((spec) => `vendor/${spec.name}/`)]);
  for (const entry of listing.split("\0")) {
    const match = /^\d+ blob ([0-9a-f]+)\t(.+)$/s.exec(entry);
    if (match) map.set(match[2], match[1]);
  }
  return map;
}

function init(argv) {
  const { options, rest } = parse(argv, [], ["--force"]);
  if (rest.length) fail("用法：init [--force]", "usage: init [--force]");
  const versions = vendoredVersions();
  const version = versions[VENDORED[0].name];
  if (!VENDORED.every((spec) => versions[spec.name] === version))
    fail(`各包的版本不一致（${describe(versions)}），init 只建立同一版本`, `the packages are at different versions (${describe(versions)}); init takes one version for all`);
  const tip = branchTip();
  if (tip && !options.force) {
    const held = vendoredVersions(tip);
    if (same(held, versions)) {
      console.log(t(`${UPSTREAM_BRANCH} 已是 ${describe(held)}`, `${UPSTREAM_BRANCH} already holds ${describe(held)}`));
      return;
    }
    fail(
      `${UPSTREAM_BRANCH} 已存在（${describe(held)}）：用 import 加入新版本，或用 --force 重建`,
      `${UPSTREAM_BRANCH} exists (${describe(held)}): add a version with import, or rebuild it with --force`,
    );
  }
  const commit = commitUpstream(version, null);
  git(["update-ref", "-m", "vendor-patches: init", ref, commit, ...(tip ? [] : [""])]);
  console.log(`${UPSTREAM_BRANCH}: ${commit.slice(0, 10)} ${describe(versions)}`);
}

/** Puts `version` on top of the branch, unless the tip holds it; the commit holding it. */
function importVersion(version) {
  const tip = requireBranch();
  const held = vendoredVersions(tip);
  if (VENDORED.every((spec) => held[spec.name] === version)) {
    console.log(t(`${UPSTREAM_BRANCH} 已是 ${describe(held)}`, `${UPSTREAM_BRANCH} already holds ${describe(held)}`));
    return tip;
  }
  const commit = commitUpstream(version, tip);
  git(["update-ref", "-m", `vendor-patches: import ${version}`, ref, commit, tip]);
  console.log(`${UPSTREAM_BRANCH}: ${commit.slice(0, 10)} ${VENDORED.map((spec) => `${spec.name} ${version}`).join(", ")}`);
  return commit;
}

/** The upstream commit a comparison starts from, and the git arguments of the comparison. */
function comparison(options) {
  const upstream = options.upstream ? git(["rev-parse", "--verify", `${options.upstream}^{commit}`]).trim() : upstreamCommit(vendoredVersions(options.rev));
  const revs = options.rev ? [upstream, git(["rev-parse", "--verify", `${options.rev}^{commit}`]).trim()] : [upstream];
  return { upstream, revs };
}

function stat(argv) {
  const { options, rest } = parse(argv, ["--rev", "--upstream", "--limit"], ["--json"]);
  const unknown = rest.find((arg) => arg.startsWith("-"));
  if (unknown) fail(`未知选项 ${unknown}`, `unknown option ${unknown}`);
  const paths = vendorPaths(rest);
  const { upstream, revs } = comparison(options);
  // Without renames, so that a file moved out of an upstream file counts as local lines.
  const diffWith = (mode) => git(["diff", mode, "--no-renames", "--no-ext-diff", "-z", ...revs, "--", ...paths]);
  const rows = sortRows(parseDiffStat(diffWith("--numstat"), diffWith("--name-status")));
  const sums = totals(rows);
  if (options.json) {
    console.log(JSON.stringify({ upstream, versions: vendoredVersions(upstream), rows, totals: Object.fromEntries(sums) }, null, 2));
    return;
  }
  const limit = options.limit ? Number(options.limit) : rows.length;
  if (!(limit >= 0)) fail("--limit 须为数字", "--limit takes a number");
  console.log(t(`相对 ${describe(vendoredVersions(upstream))}（${upstream.slice(0, 10)}）：`, `against ${describe(vendoredVersions(upstream))} (${upstream.slice(0, 10)}):`));
  for (const line of formatRows(rows.slice(0, limit))) console.log(line);
  if (limit < rows.length) console.log(t(`… 另有 ${rows.length - limit} 个文件`, `… ${rows.length - limit} more files`));
  console.log("");
  for (const [name, sum] of sums) {
    console.log(t(`${name || "合计"}：${sum.files} 个文件，+${sum.added} −${sum.removed}`, `${name || "total"}: ${sum.files} files, +${sum.added} -${sum.removed}`));
  }
}

function diff(argv) {
  const { options, rest } = parse(argv, ["--rev", "--upstream"], []);
  const flags = rest.filter((arg) => arg.startsWith("-"));
  const paths = vendorPaths(rest.filter((arg) => !arg.startsWith("-")));
  const { revs } = comparison(options);
  const result = spawnSync("git", ["diff", ...flags, ...revs, "--", ...paths], { cwd: root, stdio: "inherit" });
  process.exitCode = result.status ?? 1;
}

function upgrade(argv) {
  const [version, ...extra] = argv;
  if (!version || extra.length || version.startsWith("-")) fail("用法：upgrade <版本>", "usage: upgrade <version>");
  const dirty = git(["status", "--porcelain", "--untracked-files=all", "--", "vendor/"]).trim();
  if (dirty) fail(`vendor/ 有未提交的改动，先提交或另存：\n${dirty}`, `vendor/ has uncommitted changes; commit or set them aside first:\n${dirty}`);
  const current = vendoredVersions();
  if (!same(current, vendoredVersions("HEAD"))) fail("工作区的 vendor/ 与 HEAD 不一致", "vendor/ in the working tree differs from HEAD");
  if (VENDORED.every((spec) => current[spec.name] === version)) fail(`vendor/ 已是 ${version}`, `vendor/ is at ${version} already`);
  const base = upstreamCommit(current);
  const theirs = importVersion(version);
  const [baseBlobs, theirBlobs] = [blobs(base), blobs(theirs)];
  const plan = mergePlan(baseBlobs, blobs("HEAD"), theirBlobs);
  const scratch = mkdtempSync(join(tmpdir(), "argus-vendor-merge-"));
  const taken = [];
  const merged = [];
  const conflicted = [];
  const decisions = [];
  try {
    for (const step of plan) {
      const target = join(root, step.path);
      if (step.action === "take") {
        if (step.deleted) rmSync(target, { force: true });
        else {
          mkdirSync(dirname(target), { recursive: true });
          writeFileSync(target, git(["cat-file", "blob", theirBlobs.get(step.path)], { buffer: true }));
        }
        taken.push(step);
      } else if (step.action === "merge") {
        const [oldFile, newFile] = [join(scratch, "base"), join(scratch, "theirs")];
        writeFileSync(oldFile, git(["cat-file", "blob", baseBlobs.get(step.path)], { buffer: true }));
        writeFileSync(newFile, git(["cat-file", "blob", theirBlobs.get(step.path)], { buffer: true }));
        const name = step.path.split("/")[1];
        const result = spawnSync("git", ["merge-file", "--diff3", "-L", "vendored", "-L", `${name} ${current[name]}`, "-L", `${name} ${version}`, target, oldFile, newFile], {
          cwd: root,
          encoding: "utf8",
        });
        if (result.status === 0) merged.push(step);
        else if (result.status > 0 && result.status < 128) conflicted.push({ ...step, conflicts: result.status });
        else decisions.push({ ...step, reason: `git merge-file: ${String(result.stderr).trim() || "failed"}` });
      } else decisions.push(step);
    }
  } finally {
    rmSync(scratch, { recursive: true, force: true });
  }
  // Files new upstream are tracked without staging them, so that stat, diff
  // and git diff compare them as well.
  const added = taken.filter((step) => !step.deleted && !baseBlobs.has(step.path)).map((step) => step.path);
  if (added.length) git(["add", "--intent-to-add", "--", ...added]);
  console.log(t(`${describe(current)} → ${version}，基准 ${base.slice(0, 10)}，上游 ${theirs.slice(0, 10)}`, `${describe(current)} → ${version}, base ${base.slice(0, 10)}, upstream ${theirs.slice(0, 10)}`));
  console.log(t(`取上游文件（无本地补丁）：${taken.length}`, `taken from upstream (no local patch): ${taken.length}`));
  console.log(t(`自动合并：${merged.length}`, `merged cleanly: ${merged.length}`));
  for (const step of merged) console.log(`  ${step.path}`);
  console.log(t(`有冲突：${conflicted.length}`, `with conflicts: ${conflicted.length}`));
  for (const step of conflicted) console.log(`  ${step.path}  (${step.conflicts})`);
  console.log(t(`需要决定：${decisions.length}`, `needing a decision: ${decisions.length}`));
  for (const step of decisions) console.log(`  ${step.path}  ${step.reason}`);
  console.log(
    t(
      "解决冲突标记后，按 vendor/AGENTS.md 更新 ARGUS-PATCHES.md 并验证；stat 与 diff 此后相对新版本比较。",
      "Resolve the conflict markers, then update ARGUS-PATCHES.md and verify as vendor/AGENTS.md says; stat and diff now compare against the new version.",
    ),
  );
  if (conflicted.length || decisions.length) process.exitCode = 1;
}

const [command = "stat", ...argv] = process.argv.slice(2);
const commands = { init, stat, diff, upgrade, import: (args) => (args.length === 1 && !args[0].startsWith("-") ? importVersion(args[0]) : fail("用法：import <版本>", "usage: import <version>")) };
try {
  if (!Object.hasOwn(commands, command)) fail(`未知命令 ${command}（init、stat、diff、import、upgrade）`, `unknown command ${command} (init, stat, diff, import, upgrade)`);
  commands[command](argv);
} catch (error) {
  if (!(error instanceof Usage)) throw error;
  console.error(error.message);
  process.exitCode = 1;
}
