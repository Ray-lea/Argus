// Release helper: Conventional Commits lint, SemVer bump, changelog and the
// notes the release workflow publishes. Run `node scripts/release.mjs help`.

import { execFileSync } from "node:child_process";
import { createHash, createPublicKey, verify } from "node:crypto";
import { existsSync, readFileSync, writeFileSync } from "node:fs";
import { basename, dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { t } from "./lib/console-language.mjs";
// The app's own changelog parser (plain TypeScript, run by Node's type
// stripping): the workflow publishes exactly the entries the app finds.
import { changelogEntries, outline, strayHeadings, untranslatedLines, withTranslation } from "./lib/release-notes.ts";

const root = join(dirname(fileURLToPath(import.meta.url)), "..");

const TYPES = ["feat", "fix", "perf", "refactor", "docs", "style", "test", "build", "ci", "chore", "revert"];
const MAX_HEADER = 100;
const HEADER =
  /^(?<type>[a-z]+)(?:\((?<scope>[a-z0-9][a-z0-9-]*(?:,[a-z0-9][a-z0-9-]*)*)\))?(?<bang>!)?: (?<subject>\S.*)$/;
const SEMVER =
  /^(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)(?:-((?:0|[1-9]\d*|\d*[a-zA-Z-][0-9a-zA-Z-]*)(?:\.(?:0|[1-9]\d*|\d*[a-zA-Z-][0-9a-zA-Z-]*))*))?$/;

/**
 * The changelog's languages (the source first), with their names and titles,
 * and the headings of each kind of section in every language. The app and the
 * website read the same file to tell the kinds apart.
 */
const CHANGELOG_CONF = JSON.parse(read("resources/changelog.json"));
const LANGUAGES = CHANGELOG_CONF.languages;
const SOURCE = LANGUAGES[0].locale;
const ENGLISH = "en-US";
/** The languages translated from the source, entry for entry. */
const TRANSLATIONS = LANGUAGES.slice(1).map((language) => language.locale);
/** One file per language, named after its locale as in `locales/`. */
const changelogFile = (locale) => `changelog/${locale}.md`;
const CHANGELOG = changelogFile(SOURCE);
const CHANGELOGS = LANGUAGES.map((language) => changelogFile(language.locale));

// Changelog sections in order, by commit type, with the kind of their heading
// in resources/changelog.json. Other types (refactor, docs, ci, ...) are
// internal and stay out of the release notes.
const SECTIONS = [
  ["breaking", "breaking"],
  ["feat", "feature"],
  ["fix", "fix"],
  ["perf", "performance"],
  ["revert", "revert"],
  ["known", "known"], // written by hand
  ["other", "other"], // commits that predate the convention
];
/** The heading `pnpm release` writes for sections of `kind` in `locale`. */
const kindHeading = (kind, locale) => CHANGELOG_CONF.sections[kind][locale][0];

/** Changelog entries the update dialog can show: the release and the ones before it. */
const NOTES_RELEASES = 15;
/** The largest release notes the app accepts (MAX_NOTES_BYTES in src-gpui/src/updater.rs). */
const MAX_NOTES_BYTES = 512 * 1024;

/**
 * The line that links the editions, as at the top of the READMEs: `shown`
 * languages in bold, the others linked by `href`, in the order of
 * resources/changelog.json.
 */
function languageLine(locales, shown, href) {
  return LANGUAGES.filter((language) => locales.includes(language.locale))
    .map(({ locale, name }) => (shown.includes(locale) ? `**${name}**` : `[${name}](${href(locale)})`))
    .join(" · ");
}

/** The head of a changelog written from scratch: its title and the line linking the other languages. */
function changelogHead(locale) {
  const { title } = LANGUAGES.find((language) => language.locale === locale);
  const line = languageLine(
    LANGUAGES.map((language) => language.locale),
    [locale],
    (other) => `${other}.md`,
  );
  return `# ${title}\n\n${line}\n`;
}

const CARGO_TOML = "Cargo.toml";
const VERSION_FILES = ["package.json", "resources/app.json"];
const CARGO_VERSION = /(\[workspace\.package\][^[]*?\nversion = ")([^"]+)(")/;
// Top-level key only: these files are indented with two spaces.
const JSON_VERSION = /^( {2}"version": ")([^"]+)(")/m;

const HELP_ZH = `用法：node scripts/release.mjs <命令>

  bump [major|minor|patch|<版本>] [--dry-run]
      计算下一个版本，更新版本号与 ${CHANGELOG}，在 changelog/ 下其他语言的文件中写入待译条目，
      提交并打标签（不推送）。不指定级别时按上一个标签以来的提交推断；没有任何标签时发布当前版本。
      --dry-run 只打印版本号和各语言的发布说明，不改文件。
  lint <修订范围>          检查范围内的提交信息，例如 origin/main..HEAD
  lint --message <文本>    检查一条提交信息（例如拉取请求标题）
  verify <标签>            核对标签与各处版本号、${CHANGELOG} 条目、其他语言中已译完且逐条对应的条目、
                           更新公钥；本地已有该标签时，还核对它指向当前提交、版本文件与各语言
                           更新日志都已提交（发布工作流的第一步；推送标签前先在本地运行）
  notes <版本>             打印 GitHub Release 正文：其他语言更新日志的链接、中英文发布说明及双语
                           下载文件说明（发布工作流使用）
  release-notes <版本> <输出文件>
                           写出应用内更新对话框显示的更新内容：该版本及之前最近 ${NOTES_RELEASES} 个
                           版本的条目，附各语言译文（JSON，发布工作流签名后放进清单）
  manifest <版本> <安装包> <签名文件> [<更新内容> <更新内容的签名文件>]
                           打印应用内更新读取的清单 JSON（发布工作流使用）
  check-signature <版本> <文件> <签名文件>
                           用 resources/app.json 中的公钥校验更新签名及其中的版本号（发布工作流使用）`;

const HELP_EN = `Usage: node scripts/release.mjs <command>

  bump [major|minor|patch|<version>] [--dry-run]
      Computes the next version, updates the version files and ${CHANGELOG}, writes entries to
      translate into the other languages' files under changelog/, then commits and tags (no push).
      Without a level, infers it from the commits since the last tag; with no tag at all, releases
      the current version.
      --dry-run only prints the version and the release notes in every language; no file changes.
  lint <revision range>    Checks the commit messages in the range, e.g. origin/main..HEAD
  lint --message <text>    Checks one commit message (e.g. a pull request title)
  verify <tag>             Checks the tag against the version files, the ${CHANGELOG} entry, finished
                           entries matching it item for item in the other languages, and the update
                           public key; when the tag exists locally, also that it points at the current
                           commit and the version files and changelogs are committed (first step of the
                           release workflow; run it locally before pushing the tag)
  notes <version>          Prints the GitHub Release body: links to the other languages' changelogs,
                           the Chinese and English release notes and the bilingual download guide
                           (used by the release workflow)
  release-notes <version> <output file>
                           Writes the notes the in-app update dialog shows: the entries of this version
                           and up to ${NOTES_RELEASES} releases before it, with their translations (JSON;
                           the release workflow signs it and puts it into the manifest)
  manifest <version> <installer> <signature file> [<notes> <notes signature file>]
                           Prints the manifest JSON the in-app updater reads (used by the release workflow)
  check-signature <version> <file> <signature file>
                           Verifies an update signature and its signed version with the public key in
                           resources/app.json (used by the release workflow)`;

const HELP = t(HELP_ZH, HELP_EN);

const APP_CONF = "resources/app.json";

function fail(message) {
  console.error(`${t("[错误]", "[error]")} ${message}`);
  process.exit(1);
}

function git(...args) {
  return execFileSync("git", args, {
    cwd: root,
    encoding: "utf8",
    stdio: ["ignore", "pipe", "pipe"],
  }).trimEnd();
}

function tryGit(...args) {
  try {
    return git(...args);
  } catch {
    return null;
  }
}

function read(file) {
  return readFileSync(join(root, file), "utf8");
}

function write(file, text) {
  writeFileSync(join(root, file), text);
}

// --- Conventional Commits ---

function lines(message) {
  return message.replace(/\r\n/g, "\n").trim().split("\n");
}

/** Returns the parsed header, or null when the message is not a Conventional Commit. */
function parse(message) {
  const [header, ...rest] = lines(message);
  const m = HEADER.exec(header);
  if (!m || !TYPES.includes(m.groups.type)) return null;
  const note = /^BREAKING[ -]CHANGE: *(.+)$/m.exec(rest.join("\n"))?.[1];
  return { ...m.groups, breaking: Boolean(m.groups.bang || note), note };
}

function lintMessage(message) {
  const [header = "", second] = lines(message);
  const errors = [];
  const m = HEADER.exec(header);
  if (!m)
    errors.push(
      t(
        "首行应为 `<type>(<scope>)!: <描述>`，scope 与 ! 可省略，冒号后跟一个空格",
        "The first line must be `<type>(<scope>)!: <description>`; scope and ! are optional, and one space follows the colon",
      ),
    );
  else if (!TYPES.includes(m.groups.type))
    errors.push(
      t(
        `未知的 type「${m.groups.type}」，可用：${TYPES.join("、")}`,
        `Unknown type "${m.groups.type}"; use one of: ${TYPES.join(", ")}`,
      ),
    );
  if ([...header].length > MAX_HEADER)
    errors.push(t(`首行超过 ${MAX_HEADER} 个字符`, `The first line is longer than ${MAX_HEADER} characters`));
  if (second !== undefined && second.trim() !== "")
    errors.push(t("首行与正文之间要空一行", "Leave a blank line between the first line and the body"));
  return errors;
}

/** Commits in `range`, oldest first, without merges. */
function readCommits(range) {
  const log = git("log", "--no-merges", "--reverse", "--format=%H%x1f%B%x1e", range);
  return log
    .split("\x1e")
    .map((record) => record.trim())
    .filter(Boolean)
    .map((record) => {
      const [hash, message] = record.split("\x1f");
      return { hash, message, header: lines(message)[0], parsed: parse(message) };
    });
}

function cmdLint(args) {
  if (args[0] === "--message") {
    if (args.length < 2) fail(t("缺少要检查的文本", "Missing the text to check"));
    const text = args.slice(1).join(" ");
    const errors = lintMessage(text);
    if (errors.length)
      fail(
        t(
          `「${text}」不符合 Conventional Commits：\n  - ${errors.join("\n  - ")}`,
          `"${text}" does not follow Conventional Commits:\n  - ${errors.join("\n  - ")}`,
        ),
      );
    console.log(t("[通过] 符合 Conventional Commits", "[pass] Follows Conventional Commits"));
    return;
  }
  if (args.length !== 1)
    fail(t("用法：lint <修订范围> 或 lint --message <文本>", "Usage: lint <revision range> or lint --message <text>"));
  const commits = readCommits(args[0]);
  let bad = 0;
  for (const commit of commits) {
    const errors = lintMessage(commit.message);
    if (!errors.length) continue;
    bad++;
    console.error(`${commit.hash.slice(0, 7)} ${commit.header}\n  - ${errors.join("\n  - ")}`);
  }
  if (bad)
    fail(
      t(
        `${commits.length} 个提交中有 ${bad} 个不符合 Conventional Commits`,
        `${bad} of ${commits.length} commits do not follow Conventional Commits`,
      ),
    );
  console.log(
    t(`[通过] ${commits.length} 个提交符合 Conventional Commits`, `[pass] ${commits.length} commits follow Conventional Commits`),
  );
}

// --- SemVer ---

function parseVersion(text) {
  const m = SEMVER.exec(text);
  return m && { major: +m[1], minor: +m[2], patch: +m[3], pre: m[4] ? m[4].split(".") : [] };
}

function formatVersion(v) {
  return `${v.major}.${v.minor}.${v.patch}${v.pre.length ? `-${v.pre.join(".")}` : ""}`;
}

/** SemVer precedence (§11). */
function compare(a, b) {
  for (const key of ["major", "minor", "patch"]) if (a[key] !== b[key]) return a[key] - b[key];
  // A version without a prerelease ranks above its prereleases.
  if (!a.pre.length || !b.pre.length) return b.pre.length - a.pre.length;
  for (let i = 0; i < Math.max(a.pre.length, b.pre.length); i++) {
    const [x, y] = [a.pre[i], b.pre[i]];
    if (x === y) continue;
    if (x === undefined) return -1;
    if (y === undefined) return 1;
    const [nx, ny] = [/^\d+$/.test(x), /^\d+$/.test(y)];
    if (nx && ny) return Number(x) - Number(y);
    if (nx !== ny) return nx ? -1 : 1;
    return x < y ? -1 : 1;
  }
  return 0;
}

/** Same rules as `npm version`: a prerelease is first promoted to its release. */
function increment(v, level) {
  const pre = v.pre.length > 0;
  if (level === "major") {
    return pre && v.minor === 0 && v.patch === 0
      ? { ...v, pre: [] }
      : { major: v.major + 1, minor: 0, patch: 0, pre: [] };
  }
  if (level === "minor") {
    return pre && v.patch === 0 ? { ...v, pre: [] } : { ...v, minor: v.minor + 1, patch: 0, pre: [] };
  }
  return pre ? { ...v, pre: [] } : { ...v, patch: v.patch + 1, pre: [] };
}

function inferLevel(commits, current) {
  let level = "patch";
  for (const { parsed } of commits) {
    if (parsed?.breaking) level = "major";
    else if (parsed?.type === "feat" && level === "patch") level = "minor";
  }
  // SemVer §4: 0.y.z is initial development, so breaking changes bump the
  // minor version until 1.0.0 is released explicitly.
  return level === "major" && current.major === 0 ? "minor" : level;
}

// --- Version files ---

function workspaceMembers() {
  const list = /^members = \[([^\]]*)\]/m.exec(read(CARGO_TOML))?.[1];
  if (!list) fail(t(`${CARGO_TOML} 里找不到 workspace members`, `No workspace members in ${CARGO_TOML}`));
  return [...list.matchAll(/"([^"]+)"/g)].map(([, dir]) => {
    const manifest = read(`${dir}/Cargo.toml`);
    if (!/^version\.workspace = true$/m.test(manifest))
      fail(t(`${dir}/Cargo.toml 没有继承工作区版本号`, `${dir}/Cargo.toml does not inherit the workspace version`));
    return /^name = "([^"]+)"/m.exec(manifest)[1];
  });
}

function lockPattern(name) {
  return new RegExp(`(\\[\\[package\\]\\]\\nname = "${name}"\\nversion = ")([^"]+)(")`);
}

/** The version shared by every version file; fails if they disagree. */
function readVersion() {
  const found = [[CARGO_TOML, CARGO_VERSION.exec(read(CARGO_TOML))?.[2]]];
  for (const file of VERSION_FILES) found.push([file, JSON_VERSION.exec(read(file))?.[2]]);
  const lock = read("Cargo.lock");
  for (const name of workspaceMembers())
    found.push([t(`Cargo.lock（${name}）`, `Cargo.lock (${name})`), lockPattern(name).exec(lock)?.[2]]);
  const version = found[0][1];
  if (found.some(([, v]) => !v || v !== version)) {
    const list = found.map(([file, v]) => `  ${file}: ${v ?? t("未找到", "not found")}`).join("\n");
    fail(t(`各处版本号不一致：\n${list}`, `The version files disagree:\n${list}`));
  }
  return version;
}

function writeVersion(version) {
  const replace = (file, pattern) => write(file, read(file).replace(pattern, `$1${version}$3`));
  replace(CARGO_TOML, CARGO_VERSION);
  for (const file of VERSION_FILES) replace(file, JSON_VERSION);
  for (const name of workspaceMembers()) replace("Cargo.lock", lockPattern(name));
}

// --- Changelog ---

function repoUrl() {
  const url = tryGit("remote", "get-url", "origin");
  const m = url && /github\.com[:/]([^/]+\/[^/]+?)(?:\.git)?\/?$/.exec(url);
  return m ? `https://github.com/${m[1]}` : null;
}

function renderSection(version, prevTag, commits) {
  const repo = repoUrl();
  const link = (hash) => (repo ? `[${hash.slice(0, 7)}](${repo}/commit/${hash})` : hash.slice(0, 7));
  const entry = ({ hash, header, parsed }, text) => {
    if (!parsed) return `- ${header} (${link(hash)})`;
    const scope = parsed.scope ? `**${parsed.scope}:** ` : "";
    return `- ${scope}${text ?? parsed.subject} (${link(hash)})`;
  };
  const groups = new Map(SECTIONS.map(([key]) => [key, []]));
  for (const commit of commits) {
    const { parsed } = commit;
    if (parsed?.breaking) groups.get("breaking").push(entry(commit, parsed.note));
    const key = parsed ? parsed.type : "other";
    if (groups.has(key)) groups.get(key).push(entry(commit));
  }
  const body = SECTIONS.filter(([key]) => groups.get(key).length)
    .map(([key, kind]) => `### ${kindHeading(kind, SOURCE)}\n\n${groups.get(key).join("\n")}`)
    .join("\n\n");
  return `${sectionHeading(version, prevTag)}\n\n${body || "只有内部改动（重构、文档、构建等），没有用户可见的变化。"}\n`;
}

/** "## [1.2.0](compare link) - 2026-10-12"; the first release has no link. */
function sectionHeading(version, prevTag) {
  const repo = repoUrl();
  const date = new Date().toLocaleDateString("sv-SE"); // YYYY-MM-DD, local time
  const title = repo && prevTag ? `[${version}](${repo}/compare/${prevTag}...v${version})` : version;
  return `## ${title} - ${date}`;
}

function escapeRegExp(text) {
  return text.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
}

/**
 * The heading of an entry for `version` written ahead of time, with any text
 * after the dash: `pnpm release` replaces it with the dated one. Everything
 * else reads entries with changelogEntries, which needs that date.
 */
const headingPattern = (version) =>
  new RegExp(`^## \\[?${escapeRegExp(version)}\\]?(?:\\([^)]*\\))? - .*$`, "m");

/** The text of `locale`'s changelog, or null when it has none. */
function readChangelog(locale) {
  const file = changelogFile(locale);
  return existsSync(join(root, file)) ? read(file) : null;
}

/** Every entry of `locale`'s changelog, newest first: version, date and body. */
function readEntries(locale = SOURCE) {
  const text = readChangelog(locale);
  return text === null ? [] : changelogEntries(text);
}

/** Body of the changelog entry for `version`, without its heading; null when missing or empty. */
function readNotes(version) {
  return readEntries().find((e) => e.version === version)?.notes || null;
}

/**
 * The entry to translate into `locale`, drafted from the source one: the same
 * items, with the section headings already in that language. `verify` refuses
 * it while any of the source text is left.
 */
function draft(notes, locale) {
  return notes.replace(/^(#{3,6}[ \t]+)(.+?)[ \t]*$/gm, (line, hashes, title) => {
    for (const names of Object.values(CHANGELOG_CONF.sections)) {
      const at = names[SOURCE].indexOf(title);
      if (at >= 0) return `${hashes}${names[locale][at] ?? names[locale][0]}`;
    }
    return line;
  });
}

/**
 * What keeps `locale`'s entry for `version` (in `text`) from going out beside
 * the source entry `source`: missing, dated differently, with other sections
 * or items than the source, or, when it must be `translated`, source text
 * still in it.
 */
function translationProblems(text, locale, version, source, { translated = true } = {}) {
  const file = changelogFile(locale);
  const problems = strayHeadings(text).map(
    ({ line, text: heading }) =>
      t(
        `${file} 第 ${line} 行「${heading.trim()}」不是版本条目的标题。条目标题与 ${CHANGELOG} 相同，条目内的小节用 \`###\``,
        `${file} line ${line} "${heading.trim()}" is not a version entry heading. Entry headings are the same as in ${CHANGELOG}; sections inside an entry use \`###\``,
      ),
  );
  const entry = changelogEntries(text).find((e) => e.version === version);
  if (!entry?.notes)
    return [...problems, t(`${file} 没有 ${version} 的条目，或条目没有内容`, `${file} has no entry for ${version}, or the entry is empty`)];
  if (source && entry.date !== source.date) {
    problems.push(
      t(
        `${file} 中 ${version} 的日期 ${entry.date} 与 ${CHANGELOG} 中的 ${source.date} 不同`,
        `In ${file}, the date of ${version} (${entry.date}) differs from ${source.date} in ${CHANGELOG}`,
      ),
    );
  }
  if (source) {
    const [mine, theirs] = [outline(entry.notes), outline(source.notes)];
    if (mine.join() !== theirs.join()) {
      problems.push(
        t(
          `${file} 中 ${version} 的小节与条目数（${mine.join("/")}）与 ${CHANGELOG}（${theirs.join("/")}）不同，译文应逐条对应`,
          `In ${file}, the sections and item counts of ${version} (${mine.join("/")}) differ from ${CHANGELOG} (${theirs.join("/")}); the translation must match item for item`,
        ),
      );
    }
  }
  const left = untranslatedLines(locale, entry.notes);
  if (translated && left.length) {
    const sample = left.slice(0, 3).map((line) => (line.length > 60 ? `${line.slice(0, 60)}…` : line));
    problems.push(
      t(
        `${file} 中 ${version} 的条目还有 ${left.length} 行未翻译，例如：\n      ${sample.join("\n      ")}`,
        `In ${file}, the entry for ${version} still has ${left.length} untranslated lines, e.g.:\n      ${sample.join("\n      ")}`,
      ),
    );
  }
  return problems;
}

/**
 * What keeps the entry for `version` in `text` from reaching the release, the
 * update dialog and the app as written. Checked before the tag is pushed: the
 * release workflow fails on the same rules only after it has started.
 */
function changelogProblems(text, version) {
  const problems = strayHeadings(text).map(
    ({ line, text: heading }) =>
      t(
        `第 ${line} 行「${heading.trim()}」不是版本条目的标题，应用和发布工作流不会从这里分出条目。` +
          "条目标题为 `## [版本](比较链接) - YYYY-MM-DD`（日期后不能有其他文字），条目内的小节用 `###`",
        `Line ${line} "${heading.trim()}" is not a version entry heading; the app and the release workflow do not start an entry there. ` +
          "Entry headings are `## [version](compare link) - YYYY-MM-DD` (nothing after the date); sections inside an entry use `###`",
      ),
  );
  if (!changelogEntries(text).find((e) => e.version === version)?.notes) {
    problems.push(t(`没有 ${version} 的条目，或条目没有内容`, `No entry for ${version}, or the entry is empty`));
  }
  return problems;
}

function prepend(text, section) {
  const first = /^## /m.exec(text);
  const at = first ? first.index : text.length;
  const head = text.slice(0, at).trimEnd();
  const tail = text.slice(at);
  return `${head}\n\n${section}${tail ? `\n${tail}` : ""}`;
}

// --- Commands ---

function cmdBump(args) {
  const dryRun = args.includes("--dry-run");
  const target = args.find((arg) => !arg.startsWith("--"));
  const current = parseVersion(readVersion());
  if (!dryRun) {
    requirePubkey();
    if (git("status", "--porcelain"))
      fail(t("工作区有未提交的改动，先提交或暂存", "The working tree has uncommitted changes; commit or stash them first"));
    const branch = git("rev-parse", "--abbrev-ref", "HEAD");
    if (branch !== "main") fail(t(`只能在 main 分支发布，当前在 ${branch}`, `Releases are made from main only; this is ${branch}`));
  }

  const prevTag = tryGit("describe", "--tags", "--abbrev=0", "--match", "v[0-9]*");
  const commits = readCommits(prevTag ? `${prevTag}..HEAD` : "HEAD");
  if (prevTag && !commits.length) fail(t(`${prevTag} 之后没有新提交`, `No new commits since ${prevTag}`));

  let next;
  if (!target) next = prevTag ? increment(current, inferLevel(commits, current)) : current;
  else if (["major", "minor", "patch"].includes(target)) next = increment(current, target);
  else next = parseVersion(target.replace(/^v/, "")) ?? fail(notSemver(target));
  // Without a tag the current version has never been released, so it may be released as is.
  const order = compare(next, current);
  if (order < 0 || (order === 0 && prevTag)) {
    fail(
      t(
        `新版本 ${formatVersion(next)} 必须高于当前版本 ${formatVersion(current)}`,
        `The new version ${formatVersion(next)} must be higher than the current ${formatVersion(current)}`,
      ),
    );
  }
  const version = formatVersion(next);
  const tag = `v${version}`;
  if (tryGit("rev-parse", "-q", "--verify", `refs/tags/${tag}`)) fail(t(`标签 ${tag} 已存在`, `Tag ${tag} already exists`));

  // Notes written ahead of time (e.g. for the first release) are kept; only
  // their heading is brought up to date.
  const before = readChangelog(SOURCE);
  const written = before !== null && headingPattern(version).test(before);
  const changelog = written
    ? before.replace(headingPattern(version), () => sectionHeading(version, prevTag))
    : prepend(before ?? changelogHead(SOURCE), renderSection(version, prevTag, commits));
  const problems = changelogProblems(changelog, version);
  if (problems.length) fail(needsEditing(CHANGELOG, problems));
  const entry = changelogEntries(changelog).find((e) => e.version === version);

  // The entry in every other language: kept when written ahead of time, else
  // drafted from the source one, to be translated before the tag is pushed.
  const translations = TRANSLATIONS.map((locale) => {
    const file = changelogFile(locale);
    const text = readChangelog(locale);
    const kept = text !== null && headingPattern(version).test(text);
    const updated = kept
      ? text.replace(headingPattern(version), () => sectionHeading(version, prevTag))
      : prepend(text ?? changelogHead(locale), `${sectionHeading(version, prevTag)}\n\n${draft(entry.notes, locale)}\n`);
    const problems = translationProblems(updated, locale, version, entry, { translated: false });
    if (problems.length) fail(needsEditing(file, problems));
    const notes = changelogEntries(updated).find((e) => e.version === version).notes;
    const untranslated = translationProblems(updated, locale, version, entry).length > 0;
    return { file, text: updated, kept, notes, untranslated };
  });
  const pending = translations.filter((translation) => translation.untranslated).map((translation) => translation.file);

  console.log(
    t(
      `版本：${formatVersion(current)} → ${version}（上一个标签：${prevTag ?? "无"}，${commits.length} 个提交）\n`,
      `Version: ${formatVersion(current)} → ${version} (last tag: ${prevTag ?? "none"}, ${commits.length} commits)\n`,
    ),
  );
  const keeping = (file) => t(`沿用 ${file} 中已写好的发布说明。\n`, `Keeping the release notes already written in ${file}.\n`);
  if (written) console.log(keeping(CHANGELOG));
  for (const translation of translations) if (translation.kept) console.log(keeping(translation.file));
  if (dryRun) {
    const colon = t("：", ":");
    console.log(`${CHANGELOG}${colon}\n\n${sectionHeading(version, prevTag)}\n\n${entry.notes}\n`);
    for (const translation of translations) {
      const marker = translation.untranslated ? t("（待译）", " (to translate)") : "";
      console.log(`${translation.file}${marker}${colon}\n\n${sectionHeading(version, prevTag)}\n\n${translation.notes}\n`);
    }
    return;
  }

  writeVersion(version);
  write(CHANGELOG, changelog);
  for (const translation of translations) write(translation.file, translation.text);
  try {
    // Cargo.lock was edited by hand above; make sure Cargo agrees with it.
    execFileSync("cargo", ["metadata", "--locked", "--offline", "--format-version", "1"], {
      cwd: root,
      stdio: ["ignore", "ignore", "inherit"],
    });
  } catch {
    fail(
      t(
        "Cargo.lock 与清单不一致。版本文件已修改，未提交；用 git restore . 撤销",
        "Cargo.lock does not match the manifests. The version files are modified but not committed; undo with git restore .",
      ),
    );
  }
  git("add", CARGO_TOML, "Cargo.lock", ...VERSION_FILES, ...CHANGELOGS);
  // Empty when nothing changed: a first release of the current version with
  // its notes written ahead of time on the same day.
  git("commit", "--quiet", "--allow-empty", "-m", `chore(release): ${tag}`);
  git("tag", "-a", tag, "-m", `Argus ${tag}`);
  const list = pending.join("\n  ");
  const translate = pending.length
    ? t(
        `\n先把以下文件中 ${version} 的条目译成对应语言，与中文条目逐条对应，否则 verify 不通过：\n  ${list}`,
        `\nTranslate the ${version} entry in these files first, item for item with the Chinese entry, or verify fails:\n  ${list}`,
      )
    : "";
  console.log(
    t(
      `已提交 chore(release): ${tag} 并创建标签 ${tag}。检查 changelog/ 下的各语言更新日志，修改后用
git commit --amend 提交、git tag -fa ${tag} -m "Argus ${tag}" 移动标签。${translate}
推送前核对，推送标签即开始发布：

  node scripts/release.mjs verify ${tag}
  git push --atomic origin main ${tag}

推送前撤销：git tag -d ${tag} && git reset --hard HEAD~1`,
      `Committed chore(release): ${tag} and created tag ${tag}. Check the changelogs under changelog/; after editing,
commit with git commit --amend and move the tag with git tag -fa ${tag} -m "Argus ${tag}".${translate}
Check before pushing; pushing the tag starts the release:

  node scripts/release.mjs verify ${tag}
  git push --atomic origin main ${tag}

To undo before pushing: git tag -d ${tag} && git reset --hard HEAD~1`,
    ),
  );
}

/** A changelog that `pnpm release` must not write as it is. */
function needsEditing(file, problems) {
  const list = problems.join("\n  - ");
  return t(
    `${file} 需要先修改，未改动任何文件：\n  - ${list}`,
    `${file} needs editing first; no files were changed:\n  - ${list}`,
  );
}

function cmdVerify([tag]) {
  if (!tag?.startsWith("v") || !parseVersion(tag.slice(1)))
    fail(t(`标签「${tag ?? ""}」不是 v<语义化版本号>`, `Tag "${tag ?? ""}" is not v<semantic version>`));
  const version = readVersion();
  if (version !== tag.slice(1)) fail(t(`标签 ${tag} 与版本号 ${version} 不一致`, `Tag ${tag} does not match version ${version}`));
  const text = readChangelog(SOURCE);
  const problems = text === null ? [missing(CHANGELOG)] : changelogProblems(text, version);
  if (problems.length) {
    const list = problems.join("\n  - ");
    fail(
      t(
        `${CHANGELOG} 不能用于发布 ${tag}（发布请用 pnpm release）：\n  - ${list}`,
        `${CHANGELOG} cannot be released as ${tag} (release with pnpm release):\n  - ${list}`,
      ),
    );
  }
  const source = readEntries().find((e) => e.version === version);
  const translationsProblems = TRANSLATIONS.flatMap((locale) => {
    const translated = readChangelog(locale);
    return translated === null
      ? [missing(changelogFile(locale))]
      : translationProblems(translated, locale, version, source);
  });
  if (translationsProblems.length) {
    const list = translationsProblems.join("\n  - ");
    fail(
      t(
        `其他语言的更新日志不能用于发布 ${tag}：\n  - ${list}`,
        `The other languages' changelogs cannot be released as ${tag}:\n  - ${list}`,
      ),
    );
  }
  requirePubkey();
  // Before pushing, the files checked above must be the ones the tag carries.
  // In the workflow the checkout is the tagged commit itself, so this holds.
  const tagged = tryGit("rev-parse", "-q", "--verify", `refs/tags/${tag}^{commit}`);
  if (tagged && tagged !== tryGit("rev-parse", "HEAD")) {
    fail(
      t(
        `标签 ${tag} 没有指向当前提交。修改发布提交后用 git tag -fa ${tag} -m "Argus ${tag}" 移动标签`,
        `Tag ${tag} does not point at the current commit. After changing the release commit, move the tag with git tag -fa ${tag} -m "Argus ${tag}"`,
      ),
    );
  }
  const files = [CARGO_TOML, "Cargo.lock", ...VERSION_FILES, ...CHANGELOGS];
  const changed = tryGit("status", "--porcelain", "--", ...files);
  if (changed)
    fail(
      t(
        `以下文件有未提交的改动，标签里没有这些改动：\n${changed}\n用 git commit --amend 提交后移动标签`,
        `These files have uncommitted changes the tag does not carry:\n${changed}\nCommit them with git commit --amend, then move the tag`,
      ),
    );
  console.log(
    t(
      `[通过] ${tag} 与版本号、${LANGUAGES.length} 种语言的更新日志、更新公钥一致`,
      `[pass] ${tag} matches the version, the changelogs in ${LANGUAGES.length} languages and the update public key`,
    ),
  );
}

const missing = (file) => t(`没有 ${file}`, `${file} is missing`);
const noEntry = (version) =>
  t(`${CHANGELOG} 里没有 ${version ?? "（未指定版本）"} 的条目`, `${CHANGELOG} has no entry for ${version ?? "(no version given)"}`);
const noPubkey = () => t(`${APP_CONF} 没有配置更新签名公钥`, `${APP_CONF} has no update signing public key`);
const notSemver = (version) => t(`「${version}」不是有效的语义化版本号`, `"${version}" is not a valid semantic version`);

/** Without it the released app can verify, and therefore install, no update. */
function requirePubkey() {
  if (!JSON.parse(read(APP_CONF)).updater?.pubkey) fail(noPubkey());
}

/** The repository's web address, from the workflow's environment or the `origin` remote. */
function publicRepo() {
  return process.env.GITHUB_REPOSITORY
    ? `${process.env.GITHUB_SERVER_URL ?? "https://github.com"}/${process.env.GITHUB_REPOSITORY}`
    : repoUrl();
}

/** GitHub's anchor for a heading: lower case, punctuation dropped, spaces as hyphens. */
function anchor(heading) {
  return heading
    .toLowerCase()
    .replace(/[^\p{L}\p{M}\p{N}\p{Pc} -]/gu, "")
    .replaceAll(" ", "-");
}

/**
 * The GitHub release's changes followed by the bilingual download guide. The
 * release shows the Chinese and English notes in full; a line at the top, as
 * in the READMEs, links the entry in each other language's changelog.
 */
function cmdNotes([version]) {
  const source = version && readEntries().find((e) => e.version === version);
  if (!source?.notes) fail(noEntry(version));
  const notes = source.notes;
  const english = readEntries(ENGLISH).find((e) => e.version === version)?.notes;
  const shown = english ? [SOURCE, ENGLISH] : [SOURCE];
  const linked = TRANSLATIONS.filter(
    (locale) => !shown.includes(locale) && readEntries(locale).some((e) => e.version === version && e.notes),
  );
  const repo = publicRepo();
  const href = (locale) =>
    `${repo ? `${repo}/blob/v${version}/` : ""}${changelogFile(locale)}#${anchor(`${version} - ${source.date}`)}`;
  const languages = linked.length ? `${languageLine([...shown, ...linked], shown, href)}\n\n` : "";
  const changes = english ? `## 简体中文\n\n${notes}\n\n## English\n\n${english}` : notes;
  const downloads = read("scripts/lib/release-downloads.md").trim().replaceAll("{{version}}", version);
  console.log(`${languages}${changes}\n\n---\n\n${downloads}`);
}

/**
 * Release notes as versions before 0.2 show them: plain text, no links or
 * Markdown, and no commit scopes (core, ui, ...), which mean nothing to users.
 */
function plainNotes(markdown) {
  return markdown
    .replace(/ \(\[[0-9a-f]{7}\]\([^)]*\)\)/g, "")
    .replace(/ \([0-9a-f]{7}\)$/gm, "")
    .replace(/^### /gm, "")
    .replace(/^- \*\*[a-z0-9][a-z0-9,-]*:\*\* /gm, "· ")
    .replace(/^- /gm, "· ")
    .replace(/\*\*([^*]+)\*\*/g, "$1")
    .replace(/\[([^\]]+)\]\([^)]+\)/g, "$1");
}

/**
 * The notes the update dialog shows, as one JSON text: the entries of
 * `version` and the releases before it, so someone skipping versions sees
 * every change, each with its translations by locale (`translations`, which
 * versions before 0.3 ignore; an entry still untranslated is left out, so the
 * app falls back as it does for a missing one). Older releases are dropped
 * while the text is larger than the app accepts. The release workflow signs
 * the file as written, bound to the version, and the manifest carries it
 * verbatim; the app shows nothing that fails the signature.
 */
function cmdReleaseNotes([version, output]) {
  if (!version || !output)
    fail(t("用法：release-notes <版本> <输出文件>", "Usage: release-notes <version> <output file>"));
  const target = parseVersion(version) ?? fail(notSemver(version));
  let entries = readEntries();
  for (const locale of TRANSLATIONS) {
    const translated = readEntries(locale).filter((e) => !untranslatedLines(locale, e.notes).length);
    entries = withTranslation(entries, locale, translated);
  }
  if (!entries.some((e) => e.version === version)) fail(noEntry(version));
  const releases = entries
    .filter((e) => {
      const v = parseVersion(e.version);
      return v && compare(v, target) <= 0;
    })
    .slice(0, NOTES_RELEASES);
  const all = releases.length;
  let text = JSON.stringify({ version, releases });
  while (Buffer.byteLength(text) > MAX_NOTES_BYTES && releases.length > 1) {
    releases.pop();
    text = JSON.stringify({ version, releases });
  }
  if (Buffer.byteLength(text) > MAX_NOTES_BYTES)
    fail(
      t(
        `${version} 的更新内容超过应用接受的 ${MAX_NOTES_BYTES} 字节`,
        `The notes for ${version} exceed the ${MAX_NOTES_BYTES} bytes the app accepts`,
      ),
    );
  writeFileSync(output, text);
  const counts = releases.map((e) => {
    const count = Object.keys(e.translations ?? {}).length;
    return t(`${e.version}（${count} 种译文）`, `${e.version} (${count} translations)`);
  });
  const dropped = all - releases.length;
  console.log(
    t(`已写出 ${output}：${counts.join("、")}`, `Wrote ${output}: ${counts.join(", ")}`) +
      (dropped
        ? t(
            `\n为不超过应用接受的 ${MAX_NOTES_BYTES} 字节，省略了较早的 ${dropped} 个版本`,
            `\nLeft out the ${dropped} oldest releases to stay within the ${MAX_NOTES_BYTES} bytes the app accepts`,
          )
        : ""),
  );
}

/** The manifest the in-app updater reads (latest.json and beta.json). */
function cmdManifest(rawArgs) {
  let arm64Installer = null;
  let arm64Signature = null;
  const positional = [];
  for (let i = 0; i < rawArgs.length; i++) {
    if (rawArgs[i] === "--arm64") {
      arm64Installer = rawArgs[++i];
      arm64Signature = rawArgs[++i];
    } else {
      positional.push(rawArgs[i]);
    }
  }
  const [version, installer, signature, releaseNotes, releaseNotesSignature] = positional;
  if (
    !version ||
    !installer ||
    !signature ||
    Boolean(arm64Installer) !== Boolean(arm64Signature) ||
    Boolean(releaseNotes) !== Boolean(releaseNotesSignature)
  )
    fail(
      t(
        "用法：manifest <版本> <x64安装包> <x64签名文件> [--arm64 <arm64安装包> <arm64签名文件>] [<更新内容> <更新内容的签名文件>]",
        "Usage: manifest <version> <x64 installer> <x64 signature> [--arm64 <arm64 installer> <arm64 signature>] [<notes> <notes signature>]",
      ),
    );
  const notes = readNotes(version) ?? fail(noEntry(version));
  const repo = publicRepo() ?? fail(t("找不到 GitHub 仓库地址", "GitHub repository address not found"));
  const assetFor = (file, sig, expectedSuffix) => {
    const name = basename(file);
    if (!name.endsWith(expectedSuffix))
      fail(t(`安装包名称与架构不符：${name}`, `Installer filename does not match architecture: ${name}`));
    return {
      signature: readFileSync(sig, "utf8").trim(),
      url: `${repo}/releases/download/v${version}/${encodeURIComponent(name)}`,
    };
  };
  const x64Asset = assetFor(installer, signature, "_x64-setup.exe");
  const platforms = { "windows-x86_64-nsis": x64Asset, "windows-x86_64": x64Asset };
  if (arm64Installer) {
    const arm64Asset = assetFor(arm64Installer, arm64Signature, "_arm64-setup.exe");
    platforms["windows-aarch64-nsis"] = arm64Asset;
    platforms["windows-aarch64"] = arm64Asset;
  }
  const manifest = {
    version,
    notes: plainNotes(notes),
    pub_date: new Date().toISOString(),
    // The updater looks for `{os}-{arch}-{installer}` first, then `{os}-{arch}`.
    platforms,
  };
  if (releaseNotes) {
    // Kept as the exact text that was signed; the app parses it after checking.
    const text = readFileSync(releaseNotes, "utf8");
    if (JSON.parse(text).version !== version)
      fail(t(`${releaseNotes} 不是 ${version} 的更新内容`, `${releaseNotes} is not the notes for ${version}`));
    manifest.argus = { releaseNotes: text, signature: readFileSync(releaseNotesSignature, "utf8").trim() };
  }
  console.log(JSON.stringify(manifest, null, 2));
}

/**
 * Verifies a minisign signature the way the updater does, signed version included,
 * so a release signed with a key that does not match the public key never ships.
 */
function cmdCheckSignature([version, file, signature]) {
  if (!version || !file || !signature)
    fail(t("用法：check-signature <版本> <文件> <签名文件>", "Usage: check-signature <version> <file> <signature file>"));
  // Tauri stores both as base64 of the minisign text files.
  const lines = (base64) => Buffer.from(base64.trim(), "base64").toString("utf8").split("\n");
  const pubkey = JSON.parse(read(APP_CONF)).updater?.pubkey || fail(noPubkey());
  // Public key: algorithm (2) + key id (8) + Ed25519 key (32).
  const pk = Buffer.from(lines(pubkey)[1], "base64");
  // Signature: algorithm (2) + key id (8) + Ed25519 signature (64), then the
  // trusted comment and the global signature over signature + comment.
  const [, sigLine = "", commentLine = "", globalLine = ""] = lines(readFileSync(signature, "utf8"));
  const sig = Buffer.from(sigLine, "base64");
  const comment = commentLine.replace(/^trusted comment: /, "");
  const key = createPublicKey({
    key: { kty: "OKP", crv: "Ed25519", x: pk.subarray(10).toString("base64url") },
    format: "jwk",
  });
  if (sig.length !== 74 || !sig.subarray(2, 10).equals(pk.subarray(2, 10))) {
    fail(t(`签名使用的密钥与 ${APP_CONF} 中的公钥不同`, `The signing key differs from the public key in ${APP_CONF}`));
  }
  const data = readFileSync(file);
  // "ED" signs the BLAKE2b-512 hash of the file, legacy "Ed" the file itself.
  const message =
    sig.subarray(0, 2).toString() === "ED" ? createHash("blake2b512").update(data).digest() : data;
  if (!verify(null, message, key, sig.subarray(10)))
    fail(t("签名与文件内容不符", "The signature does not match the file"));
  const signedComment = Buffer.concat([sig.subarray(10), Buffer.from(comment)]);
  if (!verify(null, signedComment, key, Buffer.from(globalLine, "base64")))
    fail(t("签名中的可信注释没有通过校验", "The trusted comment in the signature failed verification"));
  if (!comment.split("\t").includes(`version:${version}`))
    fail(t(`签名中的版本不是 ${version}：${comment}`, `The signed version is not ${version}: ${comment}`));
  console.log(
    t(`[通过] 签名来自 ${APP_CONF} 中的公钥，版本为 ${version}`, `[pass] Signed with the public key in ${APP_CONF}, version ${version}`),
  );
}

const [command, ...args] = process.argv.slice(2);
const commands = {
  bump: cmdBump,
  lint: cmdLint,
  verify: cmdVerify,
  notes: cmdNotes,
  "release-notes": cmdReleaseNotes,
  manifest: cmdManifest,
  "check-signature": cmdCheckSignature,
};
if (commands[command]) commands[command](args);
else if (!command || command === "help" || command === "--help") console.log(HELP);
else fail(`${t("未知命令：", "Unknown command: ")}${command}\n\n${HELP}`);
