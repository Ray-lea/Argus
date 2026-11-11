// Release helper: Conventional Commits lint, SemVer bump, changelog and the
// notes the release workflow publishes. Run `node scripts/release.mjs help`.

import { execFileSync } from "node:child_process";
import { createHash, createPublicKey, verify } from "node:crypto";
import { existsSync, readFileSync, writeFileSync } from "node:fs";
import { basename, dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
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

const HELP = `用法：node scripts/release.mjs <命令>

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

const APP_CONF = "resources/app.json";

function fail(message) {
  console.error(`[错误] ${message}`);
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
  if (!m) errors.push("首行应为 `<type>(<scope>)!: <描述>`，scope 与 ! 可省略，冒号后跟一个空格");
  else if (!TYPES.includes(m.groups.type))
    errors.push(`未知的 type「${m.groups.type}」，可用：${TYPES.join("、")}`);
  if ([...header].length > MAX_HEADER) errors.push(`首行超过 ${MAX_HEADER} 个字符`);
  if (second !== undefined && second.trim() !== "") errors.push("首行与正文之间要空一行");
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
    if (args.length < 2) fail("缺少要检查的文本");
    const errors = lintMessage(args.slice(1).join(" "));
    if (errors.length)
      fail(`「${args.slice(1).join(" ")}」不符合 Conventional Commits：\n  - ${errors.join("\n  - ")}`);
    console.log("[通过] 符合 Conventional Commits");
    return;
  }
  if (args.length !== 1) fail("用法：lint <修订范围> 或 lint --message <文本>");
  const commits = readCommits(args[0]);
  let bad = 0;
  for (const commit of commits) {
    const errors = lintMessage(commit.message);
    if (!errors.length) continue;
    bad++;
    console.error(`${commit.hash.slice(0, 7)} ${commit.header}\n  - ${errors.join("\n  - ")}`);
  }
  if (bad) fail(`${commits.length} 个提交中有 ${bad} 个不符合 Conventional Commits`);
  console.log(`[通过] ${commits.length} 个提交符合 Conventional Commits`);
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
  if (!list) fail(`${CARGO_TOML} 里找不到 workspace members`);
  return [...list.matchAll(/"([^"]+)"/g)].map(([, dir]) => {
    const manifest = read(`${dir}/Cargo.toml`);
    if (!/^version\.workspace = true$/m.test(manifest)) fail(`${dir}/Cargo.toml 没有继承工作区版本号`);
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
    found.push([`Cargo.lock（${name}）`, lockPattern(name).exec(lock)?.[2]]);
  const version = found[0][1];
  if (found.some(([, v]) => !v || v !== version)) {
    fail(`各处版本号不一致：\n${found.map(([file, v]) => `  ${file}: ${v ?? "未找到"}`).join("\n")}`);
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
      `${file} 第 ${line} 行「${heading.trim()}」不是版本条目的标题。条目标题与 ${CHANGELOG} 相同，条目内的小节用 \`###\``,
  );
  const entry = changelogEntries(text).find((e) => e.version === version);
  if (!entry?.notes) return [...problems, `${file} 没有 ${version} 的条目，或条目没有内容`];
  if (source && entry.date !== source.date) {
    problems.push(`${file} 中 ${version} 的日期 ${entry.date} 与 ${CHANGELOG} 中的 ${source.date} 不同`);
  }
  if (source) {
    const [mine, theirs] = [outline(entry.notes), outline(source.notes)];
    if (mine.join() !== theirs.join()) {
      problems.push(
        `${file} 中 ${version} 的小节与条目数（${mine.join("/")}）与 ${CHANGELOG}（${theirs.join("/")}）不同，译文应逐条对应`,
      );
    }
  }
  const left = untranslatedLines(locale, entry.notes);
  if (translated && left.length) {
    const sample = left.slice(0, 3).map((line) => (line.length > 60 ? `${line.slice(0, 60)}…` : line));
    problems.push(`${file} 中 ${version} 的条目还有 ${left.length} 行未翻译，例如：\n      ${sample.join("\n      ")}`);
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
      `第 ${line} 行「${heading.trim()}」不是版本条目的标题，应用和发布工作流不会从这里分出条目。` +
      "条目标题为 `## [版本](比较链接) - YYYY-MM-DD`（日期后不能有其他文字），条目内的小节用 `###`",
  );
  if (!changelogEntries(text).find((e) => e.version === version)?.notes) {
    problems.push(`没有 ${version} 的条目，或条目没有内容`);
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
    if (git("status", "--porcelain")) fail("工作区有未提交的改动，先提交或暂存");
    const branch = git("rev-parse", "--abbrev-ref", "HEAD");
    if (branch !== "main") fail(`只能在 main 分支发布，当前在 ${branch}`);
  }

  const prevTag = tryGit("describe", "--tags", "--abbrev=0", "--match", "v[0-9]*");
  const commits = readCommits(prevTag ? `${prevTag}..HEAD` : "HEAD");
  if (prevTag && !commits.length) fail(`${prevTag} 之后没有新提交`);

  let next;
  if (!target) next = prevTag ? increment(current, inferLevel(commits, current)) : current;
  else if (["major", "minor", "patch"].includes(target)) next = increment(current, target);
  else next = parseVersion(target.replace(/^v/, "")) ?? fail(`「${target}」不是有效的语义化版本号`);
  // Without a tag the current version has never been released, so it may be released as is.
  const order = compare(next, current);
  if (order < 0 || (order === 0 && prevTag)) {
    fail(`新版本 ${formatVersion(next)} 必须高于当前版本 ${formatVersion(current)}`);
  }
  const version = formatVersion(next);
  const tag = `v${version}`;
  if (tryGit("rev-parse", "-q", "--verify", `refs/tags/${tag}`)) fail(`标签 ${tag} 已存在`);

  // Notes written ahead of time (e.g. for the first release) are kept; only
  // their heading is brought up to date.
  const before = readChangelog(SOURCE);
  const written = before !== null && headingPattern(version).test(before);
  const changelog = written
    ? before.replace(headingPattern(version), () => sectionHeading(version, prevTag))
    : prepend(before ?? changelogHead(SOURCE), renderSection(version, prevTag, commits));
  const problems = changelogProblems(changelog, version);
  if (problems.length) fail(`${CHANGELOG} 需要先修改，未改动任何文件：\n  - ${problems.join("\n  - ")}`);
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
    if (problems.length) fail(`${file} 需要先修改，未改动任何文件：\n  - ${problems.join("\n  - ")}`);
    const notes = changelogEntries(updated).find((e) => e.version === version).notes;
    const untranslated = translationProblems(updated, locale, version, entry).length > 0;
    return { file, text: updated, kept, notes, untranslated };
  });
  const pending = translations.filter((t) => t.untranslated).map((t) => t.file);

  console.log(
    `版本：${formatVersion(current)} → ${version}（上一个标签：${prevTag ?? "无"}，${commits.length} 个提交）\n`,
  );
  if (written) console.log(`沿用 ${CHANGELOG} 中已写好的发布说明。\n`);
  for (const t of translations) if (t.kept) console.log(`沿用 ${t.file} 中已写好的发布说明。\n`);
  if (dryRun) {
    console.log(`${CHANGELOG}：\n\n${sectionHeading(version, prevTag)}\n\n${entry.notes}\n`);
    for (const t of translations) {
      console.log(`${t.file}${t.untranslated ? "（待译）" : ""}：\n\n${sectionHeading(version, prevTag)}\n\n${t.notes}\n`);
    }
    return;
  }

  writeVersion(version);
  write(CHANGELOG, changelog);
  for (const t of translations) write(t.file, t.text);
  try {
    // Cargo.lock was edited by hand above; make sure Cargo agrees with it.
    execFileSync("cargo", ["metadata", "--locked", "--offline", "--format-version", "1"], {
      cwd: root,
      stdio: ["ignore", "ignore", "inherit"],
    });
  } catch {
    fail("Cargo.lock 与清单不一致。版本文件已修改，未提交；用 git restore . 撤销");
  }
  git("add", CARGO_TOML, "Cargo.lock", ...VERSION_FILES, ...CHANGELOGS);
  // Empty when nothing changed: a first release of the current version with
  // its notes written ahead of time on the same day.
  git("commit", "--quiet", "--allow-empty", "-m", `chore(release): ${tag}`);
  git("tag", "-a", tag, "-m", `Argus ${tag}`);
  const translate = pending.length
    ? `\n先把以下文件中 ${version} 的条目译成对应语言，与中文条目逐条对应，否则 verify 不通过：\n  ${pending.join("\n  ")}`
    : "";
  console.log(`已提交 chore(release): ${tag} 并创建标签 ${tag}。检查 changelog/ 下的各语言更新日志，修改后用
git commit --amend 提交、git tag -fa ${tag} -m "Argus ${tag}" 移动标签。${translate}
推送前核对，推送标签即开始发布：

  node scripts/release.mjs verify ${tag}
  git push --atomic origin main ${tag}

推送前撤销：git tag -d ${tag} && git reset --hard HEAD~1`);
}

function cmdVerify([tag]) {
  if (!tag?.startsWith("v") || !parseVersion(tag.slice(1))) fail(`标签「${tag ?? ""}」不是 v<语义化版本号>`);
  const version = readVersion();
  if (version !== tag.slice(1)) fail(`标签 ${tag} 与版本号 ${version} 不一致`);
  const text = readChangelog(SOURCE);
  const problems = text === null ? [`没有 ${CHANGELOG}`] : changelogProblems(text, version);
  if (problems.length)
    fail(`${CHANGELOG} 不能用于发布 ${tag}（发布请用 pnpm release）：\n  - ${problems.join("\n  - ")}`);
  const source = readEntries().find((e) => e.version === version);
  const translationsProblems = TRANSLATIONS.flatMap((locale) => {
    const translated = readChangelog(locale);
    return translated === null
      ? [`没有 ${changelogFile(locale)}`]
      : translationProblems(translated, locale, version, source);
  });
  if (translationsProblems.length)
    fail(`其他语言的更新日志不能用于发布 ${tag}：\n  - ${translationsProblems.join("\n  - ")}`);
  requirePubkey();
  // Before pushing, the files checked above must be the ones the tag carries.
  // In the workflow the checkout is the tagged commit itself, so this holds.
  const tagged = tryGit("rev-parse", "-q", "--verify", `refs/tags/${tag}^{commit}`);
  if (tagged && tagged !== tryGit("rev-parse", "HEAD")) {
    fail(`标签 ${tag} 没有指向当前提交。修改发布提交后用 git tag -fa ${tag} -m "Argus ${tag}" 移动标签`);
  }
  const files = [CARGO_TOML, "Cargo.lock", ...VERSION_FILES, ...CHANGELOGS];
  const changed = tryGit("status", "--porcelain", "--", ...files);
  if (changed)
    fail(`以下文件有未提交的改动，标签里没有这些改动：\n${changed}\n用 git commit --amend 提交后移动标签`);
  console.log(`[通过] ${tag} 与版本号、${LANGUAGES.length} 种语言的更新日志、更新公钥一致`);
}

/** Without it the released app can verify, and therefore install, no update. */
function requirePubkey() {
  if (!JSON.parse(read(APP_CONF)).updater?.pubkey) fail(`${APP_CONF} 没有配置更新签名公钥`);
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
  if (!source?.notes) fail(`${CHANGELOG} 里没有 ${version ?? "（未指定版本）"} 的条目`);
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
  if (!version || !output) fail("用法：release-notes <版本> <输出文件>");
  const target = parseVersion(version) ?? fail(`「${version}」不是有效的语义化版本号`);
  let entries = readEntries();
  for (const locale of TRANSLATIONS) {
    const translated = readEntries(locale).filter((e) => !untranslatedLines(locale, e.notes).length);
    entries = withTranslation(entries, locale, translated);
  }
  if (!entries.some((e) => e.version === version)) fail(`${CHANGELOG} 里没有 ${version} 的条目`);
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
  if (Buffer.byteLength(text) > MAX_NOTES_BYTES) fail(`${version} 的更新内容超过应用接受的 ${MAX_NOTES_BYTES} 字节`);
  writeFileSync(output, text);
  const counts = releases.map((e) => `${e.version}（${Object.keys(e.translations ?? {}).length} 种译文）`);
  const dropped = all - releases.length;
  console.log(
    `已写出 ${output}：${counts.join("、")}` +
      (dropped ? `\n为不超过应用接受的 ${MAX_NOTES_BYTES} 字节，省略了较早的 ${dropped} 个版本` : ""),
  );
}

/** The manifest the in-app updater reads (latest.json and beta.json). */
function cmdManifest([version, installer, signature, releaseNotes, releaseNotesSignature]) {
  if (!version || !installer || !signature || Boolean(releaseNotes) !== Boolean(releaseNotesSignature))
    fail("用法：manifest <版本> <安装包> <签名文件> [<更新内容> <更新内容的签名文件>]");
  const notes = readNotes(version) ?? fail(`${CHANGELOG} 里没有 ${version} 的条目`);
  const repo = publicRepo() ?? fail("找不到 GitHub 仓库地址");
  const asset = {
    signature: readFileSync(signature, "utf8").trim(),
    url: `${repo}/releases/download/v${version}/${encodeURIComponent(basename(installer))}`,
  };
  const manifest = {
    version,
    notes: plainNotes(notes),
    pub_date: new Date().toISOString(),
    // The updater looks for `{os}-{arch}-{installer}` first, then `{os}-{arch}`.
    platforms: { "windows-x86_64-nsis": asset, "windows-x86_64": asset },
  };
  if (releaseNotes) {
    // Kept as the exact text that was signed; the app parses it after checking.
    const text = readFileSync(releaseNotes, "utf8");
    if (JSON.parse(text).version !== version) fail(`${releaseNotes} 不是 ${version} 的更新内容`);
    manifest.argus = { releaseNotes: text, signature: readFileSync(releaseNotesSignature, "utf8").trim() };
  }
  console.log(JSON.stringify(manifest, null, 2));
}

/**
 * Verifies a minisign signature the way the updater does, signed version included,
 * so a release signed with a key that does not match the public key never ships.
 */
function cmdCheckSignature([version, file, signature]) {
  if (!version || !file || !signature) fail("用法：check-signature <版本> <文件> <签名文件>");
  // Tauri stores both as base64 of the minisign text files.
  const lines = (base64) => Buffer.from(base64.trim(), "base64").toString("utf8").split("\n");
  const pubkey =
    JSON.parse(read(APP_CONF)).updater?.pubkey || fail(`${APP_CONF} 没有配置更新签名公钥`);
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
    fail(`签名使用的密钥与 ${APP_CONF} 中的公钥不同`);
  }
  const data = readFileSync(file);
  // "ED" signs the BLAKE2b-512 hash of the file, legacy "Ed" the file itself.
  const message =
    sig.subarray(0, 2).toString() === "ED" ? createHash("blake2b512").update(data).digest() : data;
  if (!verify(null, message, key, sig.subarray(10))) fail("签名与文件内容不符");
  const signedComment = Buffer.concat([sig.subarray(10), Buffer.from(comment)]);
  if (!verify(null, signedComment, key, Buffer.from(globalLine, "base64")))
    fail("签名中的可信注释没有通过校验");
  if (!comment.split("\t").includes(`version:${version}`)) fail(`签名中的版本不是 ${version}：${comment}`);
  console.log(`[通过] 签名来自 ${APP_CONF} 中的公钥，版本为 ${version}`);
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
else fail(`未知命令：${command}\n\n${HELP}`);
