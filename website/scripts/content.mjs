// Prepares the generated inputs of the website: the documentation of both
// editions as Fumadocs content, the release snapshot in site.json, and the
// images the pages link to. Files are written only when they change, so a
// running development server reloads just what moved.
import { execFileSync } from "node:child_process";
import { readFile, readdir, glob } from "node:fs/promises";
import { join, posix } from "node:path";
import MarkdownIt from "markdown-it";
import GithubSlugger from "github-slugger";
import {
  assetsDir,
  contentDir,
  prune,
  putIfChanged,
  repoRoot as root,
  siteFormat,
  websiteRoot,
  workDir,
} from "./paths.mjs";
import { repo, repositoryStats } from "./release.mjs";
import { prepareReplicaCatalogs } from "./locales.mjs";
import { languages } from "../src/lib/languages.ts";
import { labelTables, translateHtml } from "./translate-html.mjs";

const git = (args) =>
  execFileSync("git", args, { cwd: root, maxBuffer: 32 * 1024 * 1024 });
const fileAt = (ref, file) => git(["show", ref + ":" + file]);
const markdown = new MarkdownIt({ html: false });
export const bases = { stable: "/docs/", dev: "/docs/dev/" };
// Folders in parentheses add nothing to the URL: the stable edition lives at /docs/.
export const folders = { stable: "(stable)", dev: "dev" };
const notes = { stable: "随正式版发布", dev: "main 分支 · 含未发布改动" };
const icons = { stable: "BookMarked", dev: "GitBranch" };

// Read a repository file from the working tree (development) or from the documented commit.
const source = (docs, file) =>
  docs.worktree ? readFile(join(root, file)) : fileAt(docs.ref, file);

export function routeFor(file, base) {
  return (
    base +
    file
      .slice("docs/".length)
      .replace(/(^|\/)README\.md$/, "$1")
      .replace(/\.md$/, "/")
  );
}
// docs/guide/README.md is the folder's index page; other names keep their stem.
const pageName = (file) =>
  file.slice("docs/".length).replace(/(^|\/)README\.md$/, "$1index.md").replace(/\.md$/, "");
export const contentFile = (file, id) => "docs/" + folders[id] + "/" + pageName(file) + ".md";

// Sidebar icons (lucide names, resolved in src/lib/source.ts); a new document simply has none.
const pageIcons = {
  "docs/README.md": "BookOpen",
  "docs/guide/README.md": "MousePointerClick",
  "docs/guide/resident-mode.md": "Keyboard",
  "docs/guide/settings.md": "Settings",
  "docs/guide/hardware-data.md": "Cpu",
  "docs/development/README.md": "Terminal",
  "docs/development/build.md": "Hammer",
  "docs/development/performance.md": "Gauge",
  "docs/development/gpui.md": "AppWindow",
  "docs/development/release.md": "Rocket",
  "docs/development/website.md": "Globe",
  "docs/development/signing.md": "BadgeCheck",
  "docs/development/i18n.md": "Languages",
  "docs/development/security.md": "ShieldCheck",
  "docs/architecture/README.md": "LayoutDashboard",
  "docs/architecture/tech-stack.md": "Layers",
  "docs/architecture/core.md": "Activity",
  "docs/architecture/host.md": "Monitor",
  "docs/architecture/privileged-service.md": "Shield",
  "docs/architecture/administrator-startup.md": "KeyRound",
  "docs/architecture/task-manager.md": "ArrowLeftRight",
  "docs/architecture/frontend.md": "PanelsTopLeft",
  "docs/design/README.md": "Palette",
  "docs/design/charts.md": "ChartSpline",
  "docs/design/window.md": "PanelTop",
  "docs/design/hotkey.md": "Command",
  "docs/roadmap.md": "Map",
  "docs/known-issues.md": "TriangleAlert",
};

// A page's description: its opening paragraph up to the last whole sentence
// that fits the length search results show, cut with an ellipsis only when the
// first sentence alone is too long.
export function excerpt(text, limit = 150) {
  if (text.length <= limit) return text;
  const cut = text.slice(0, limit);
  const end = Math.max(...["。", "！", "？", ". "].map((mark) => cut.lastIndexOf(mark)));
  return end >= 40 ? cut.slice(0, end + 1).trimEnd() : cut.slice(0, limit - 1) + "…";
}

export function documentPage(file, text, id, { updated = "", label = "" } = {}) {
  const body = text.replace(/\r\n/g, "\n");
  const heading = /^# (.+)$/m.exec(body);
  if (!heading) throw Error("Missing document title in " + file);
  const title = heading[1].trim();
  let content = body
    .slice(heading.index + heading[0].length)
    .trimStart()
    // Older indexes open with an ASCII folder tree; the sidebar shows the same structure.
    .replace(/^`{3}[^\n]*\ndocs\/\n[\s\S]*?\n`{3}\s*/, "");
  // The documentation index becomes cards for each chapter group; keep its introduction.
  if (file === "docs/README.md") content = content.split(/^## /m)[0].trim() + "\n";
  const lead = markdown
    .parse(content, {})
    .find((t) => t.type === "inline" && t.content.length > 25);
  const summary = excerpt(
    lead?.children
      ?.map((t) => t.content)
      .join("")
      .replace(/\s+/g, " ") ?? "",
  );
  const front = {
    // The sidebar and search use the chapter name from the index; the page shows its own title.
    title: label || title,
    heading: title,
    description: summary || title,
    source: file,
    updated,
    // Earlier links to the page used GitHub's fragment for the title.
    anchor: new GithubSlugger().slug(title),
    ...(pageIcons[file] && { icon: pageIcons[file] }),
  };
  return {
    path: routeFor(file, bases[id]),
    file: contentFile(file, id),
    markdown:
      "---\n" +
      Object.entries(front)
        .map(([key, value]) => key + ": " + JSON.stringify(value))
        .join("\n") +
      "\n---\n\n" +
      content,
  };
}

export async function workspaceDocuments() {
  return (await Array.fromAsync(glob("docs/**/*.md", { cwd: root })))
    .map((name) => name.replaceAll("\\", "/"))
    .filter((name) => !name.endsWith("/AGENTS.md"))
    .sort();
}

// The documentation index lists every chapter in reading order; the sidebar follows it.
export function navigation(index, routes) {
  const groups = [];
  let group;
  for (const line of index.replace(/\r\n/g, "\n").split("\n")) {
    const heading = /^## (.+)$/.exec(line);
    if (heading) {
      group = { label: heading[1].trim(), items: [] };
      groups.push(group);
      continue;
    }
    const item = /^[-*] \[([^\]]+)\]\(([^)#]+)\)(?:\s*[：:]\s*(.+))?/.exec(line);
    if (!item || !group) continue;
    const file = posix.normalize(posix.join("docs", item[2]));
    if (routes[file] && !group.items.some((i) => i.file === file))
      group.items.push({
        label: item[1],
        file,
        path: routes[file],
        note: item[3]?.replaceAll("`", "").trim() ?? "",
      });
  }
  const listed = new Set(groups.flatMap((g) => g.items.map((i) => i.file)));
  const rest = Object.keys(routes).filter(
    (file) => file !== "docs/README.md" && !listed.has(file),
  );
  if (rest.length)
    groups.push({
      label: "其他页面",
      items: rest.map((file) => ({
        label: file.slice(5, -3),
        file,
        path: routes[file],
        note: "",
      })),
    });
  return groups.filter((g) => g.items.length);
}

// The edition folder's meta.json: a root folder, so the selector at the top of
// the sidebar lists both editions and moves to the same page in the other one.
export function editionMeta(edition) {
  return JSON.stringify(
    {
      title: edition.label,
      description: edition.note,
      icon: icons[edition.id],
      root: true,
      pages: [
        "index",
        ...edition.nav.flatMap((group) => [
          "---" + group.label + "---",
          ...group.items.map((item) => pageName(item.file)),
        ]),
      ],
    },
    null,
    2,
  );
}

async function prepareEdition({ id, label, requested, worktree }, written) {
  const base = bases[id];
  const ref = git(["rev-parse", "--verify", requested + "^{commit}"])
    .toString()
    .trim();
  const docs = { ref, worktree };
  const names = (
    worktree
      ? await workspaceDocuments()
      : git(["ls-tree", "-r", "-z", "--name-only", ref, "--", "docs"])
          .toString()
          .split("\0")
  ).filter((f) => f.startsWith("docs/") && f.endsWith(".md") && !f.endsWith("/AGENTS.md"));
  const routes = Object.fromEntries(names.map((file) => [file, routeFor(file, base)]));
  if (!routes["docs/README.md"]) throw Error("Missing " + label + " documentation index");
  const nav = navigation((await source(docs, "docs/README.md")).toString(), routes);
  const labels = new Map(nav.flatMap((group) => group.items.map((i) => [i.file, i.label])));
  // One git call gives the date of the last commit that touched each document in this edition.
  const updated = {};
  let date = "";
  for (const line of git(["log", "--format=%x00%cs", "--name-only", ref, "--", "docs"])
    .toString()
    .split("\n")) {
    if (line.startsWith("\0")) date = line.slice(1);
    else if (line && !updated[line]) updated[line] = date;
  }
  for (const file of names) {
    const page = documentPage(file, (await source(docs, file)).toString(), id, {
      updated: updated[file] ?? "",
      label: file === "docs/README.md" ? "文档首页" : labels.get(file),
    });
    await putIfChanged(contentDir, page.file, page.markdown);
    written.add(join(contentDir, page.file));
  }
  const edition = { id, label, note: notes[id], ref, base, folder: folders[id], routes, nav };
  const meta = "docs/" + folders[id] + "/meta.json";
  await putIfChanged(contentDir, meta, editionMeta(edition));
  written.add(join(contentDir, meta));
  return edition;
}

// The development edition follows the working tree locally and the deployed main commit in CI.
export function prepareDevelopmentEdition(written, worktree = !process.env.ARGUS_DOCS_DEV_REF) {
  return prepareEdition(
    {
      id: "dev",
      label: "开发版",
      requested: worktree ? "HEAD" : process.env.ARGUS_DOCS_DEV_REF,
      worktree,
    },
    written,
  );
}
export function prepareStableEdition(written, release) {
  return prepareEdition(
    { id: "stable", label: "稳定版 " + release.tag, requested: release.tag, worktree: false },
    written,
  );
}

function section(text, heading) {
  const match = text.match(
    new RegExp(
      "^## [^\\n]*" + heading + "[^\\n]*\\n([\\s\\S]*?)(?=^## |$(?![\\s\\S]))",
      "im",
    ),
  ) ?? text.match(new RegExp('<a id="' + heading.replaceAll(" ", "-") + '"></a>\\s*## [^\\n]*\\n([\\s\\S]*?)(?=^## |$(?![\\s\\S]))', "im"));
  if (!match) throw Error("Missing Markdown section: " + heading);
  return match[1].replace(/^<a id="[^"]+"><\/a>\s*$/gm, "").trim();
}
function render(text, rewrite) {
  const tokens = markdown.parse(text, {});
  for (const token of tokens)
    for (const part of token.children ?? []) {
      if (part.type === "link_open")
        part.attrSet("href", rewrite(part.attrGet("href")));
    }
  return markdown.renderer.render(tokens, markdown.options, {});
}

// Release notes of the published versions, newest first, as each language's
// changelog/<locale>.md reads at the stable tag: each version's intro and its
// groups of labelled entries, in plain text for summaries and as HTML with links.
const plain = (text) =>
  text
    .replace(/\[([^\]]*)\]\([^)]*\)/g, "$1")
    .replace(/\*\*([^*]+)\*\*/g, "$1")
    .replace(/`([^`]+)`/g, "$1")
    .trim();
// The section headings of each kind in every language, shared with the app and
// scripts/release.mjs. Reverts and other sections read as "other".
const changelogConf = JSON.parse(await readFile(join(root, "resources/changelog.json"), "utf8"));
const groupKinds = ["feature", "fix", "performance", "breaking", "upgrade", "known"].map((kind) => [
  kind,
  new Set(Object.values(changelogConf.sections[kind]).flat().map((name) => name.toLowerCase())),
]);
function inline(text, rewrite) {
  const tokens = markdown.parseInline(text, {});
  for (const token of tokens)
    for (const part of token.children ?? [])
      if (part.type === "link_open") part.attrSet("href", rewrite(part.attrGet("href")));
  return markdown.renderer.render(tokens, markdown.options, {});
}
export function releaseNotes(text, tag) {
  const rewrite = (href) =>
    /^[a-z]+:/i.test(href) || href.startsWith("#") ? href : repo + "/blob/" + tag + "/" + href;
  return text
    .replace(/\r\n/g, "\n")
    .split(/^## /m)
    .slice(1)
    .map((part) => {
      const [head, ...lines] = part.split("\n");
      const match = /^\[?(\d+\.\d+\.\d+)\]?(?:\(([^)]*)\))?\s+-\s+(\d{4}-\d{2}-\d{2})/.exec(head);
      if (!match) throw Error("Unreadable changelog heading: " + head);
      const intro = [],
        groups = [];
      let notice = "",
        group = null;
      for (const line of lines) {
        const heading = /^### (.+)$/.exec(line);
        const item = /^[-*] (?:\*\*([^*]+)\*\*\s*[：:]\s*)?(.+)$/.exec(line);
        if (heading) {
          const title = heading[1].trim();
          groups.push((group = { title, kind: groupKinds.find(([, names]) => names.has(title.toLowerCase()))?.[0] ?? "other", items: [] }));
        } else if (group && item) group.items.push({ label: item[1]?.trim() ?? "", md: item[2] });
        else if (group && /^\s+\S/.test(line) && group.items.length) group.items.at(-1).md += " " + line.trim();
        else if (!group && line.trim())
          // A paragraph in bold alone is a notice, such as a withdrawn release.
          if (/^\*\*[^*]+\*\*$/.test(line.trim())) notice = plain(line);
          else intro.push(line.trim());
      }
      return {
        version: match[1],
        date: match[3],
        compare: match[2] ?? "",
        notice,
        withdrawn: /撤回|withdrawn/i.test(notice),
        intro: plain(intro.join(" ")),
        introHtml: intro.length ? inline(intro.join(" "), rewrite) : "",
        groups: groups.map((g) => ({
          ...g,
          items: g.items.map(({ label, md }) => ({ label, text: plain(md), html: inline(md, rewrite) })),
        })),
      };
    });
}

// Every language's release notes by site language: its changelog at the stable
// tag (tags before the changelog/ folder have the Chinese and English ones at
// the repository root). A language the tag has no edition of takes its entries
// for the released versions from the development commit, so a translation
// shows before the next release; one without any is left out, and the site
// shows the source language instead. Withdrawn releases are those of the
// Chinese changelog.
async function changelogs(release, at) {
  const legacy = { "zh-CN": "CHANGELOG.md", "en-US": "CHANGELOG.en.md" };
  const files = (ref) =>
    new Set(git(["ls-tree", "-r", "--name-only", ref, "--", "changelog", ...Object.values(legacy)]).toString().split("\n"));
  const stable = files(release.tag);
  const development = at.worktree ? null : files(at.ref);
  const atStable = (locale) => {
    const file = [`changelog/${locale}.md`, legacy[locale]].find((name) => name && stable.has(name));
    return file ? fileAt(release.tag, file).toString() : null;
  };
  const atDevelopment = async (locale) => {
    const file = `changelog/${locale}.md`;
    if (at.worktree) return readFile(join(root, file), "utf8").catch(() => null);
    return development.has(file) ? fileAt(at.ref, file).toString() : null;
  };
  const source = releaseNotes(atStable("zh-CN"), release.tag);
  const released = new Set(source.map((note) => note.version));
  const withdrawn = new Set(source.filter((note) => note.withdrawn).map((note) => note.version));
  // The released entries alone: the development commit may hold one written ahead of a release.
  const releasedOnly = (text) =>
    text
      .replace(/\r\n/g, "\n")
      .split(/^(?=## )/m)
      .filter((part, i) => i === 0 || released.has(/^## \[?([^\]\s(]+)/.exec(part)?.[1]))
      .join("");
  const result = {};
  for (const { id, tag } of languages) {
    const text = atStable(tag) ?? (await atDevelopment(tag).then((dev) => dev && releasedOnly(dev)));
    if (!text) continue;
    result[id] = releaseNotes(text, release.tag).map((note) => ({ ...note, withdrawn: withdrawn.has(note.version) }));
  }
  return result;
}

// Commits per day over the five weeks up to the documented development commit.
function activity(ref) {
  const dates = git(["log", "--format=%cs", ref]).toString().trim().split("\n");
  const counts = new Map();
  for (const date of dates) counts.set(date, (counts.get(date) ?? 0) + 1);
  const first = dates.at(-1),
    end = Date.parse(dates[0] + "T00:00:00Z");
  const days = [];
  for (let i = 34; i >= 0; i--) {
    const day = new Date(end - i * 86400000).toISOString().slice(0, 10);
    if (day >= first) days.push([day, counts.get(day) ?? 0]);
  }
  return { total: dates.length, first, days };
}

// GitHub's figures for the repository, fetched at most hourly; without the
// network the last ones fetched stand in, and the pages leave them out when
// there are none.
async function repository() {
  const cache = join(workDir, "cache");
  let cached = null;
  try {
    cached = JSON.parse(await readFile(join(cache, "repository.json"), "utf8"));
  } catch {}
  if (cached && Date.now() - Date.parse(cached.checked) < 3600000) return cached;
  try {
    const stats = await repositoryStats();
    await putIfChanged(cache, "repository.json", JSON.stringify(stats));
    return stats;
  } catch (error) {
    console.warn("[website] GitHub repository figures unavailable: " + error.message);
    return cached;
  }
}

// Link previews use the documented build's process page.
async function previews(at) {
  for (const lang of ["zh", "en"])
    await putIfChanged(
      assetsDir,
      "preview-" + lang + "-dark.png",
      await source(at, "docs/images/screenshot-" + (lang === "en" ? "en-" : "") + "dark.png"),
    );
}

export async function prepareContent(release, { worktree = !process.env.ARGUS_DOCS_DEV_REF } = {}) {
  await prepareReplicaCatalogs();
  const written = new Set();
  const editions = [
    await prepareStableEdition(written, release),
    await prepareDevelopmentEdition(written, worktree),
  ];
  const docs = editions[1];
  // The download page and the English capture describe the build about to ship, as main does.
  const at = { ref: docs.ref, worktree };
  const privacy = await readFile(join(root, "PRIVACY.md"), "utf8");
  const translations = {};
  const privacyEnglish = render(section(privacy, "English"), href => href.startsWith("docs/") ? repo + "/blob/main/" + href : href);
  for (const { id: lang, readme: readmeFile, requirements: heading } of languages) {
    const en = lang === "en";
    const prefix = lang === "zh" ? "" : "/" + lang;
    const readme = (await source(at, readmeFile)).toString();
    const requirements = section(readme, heading);
    if (!requirements.trim()) throw Error("Missing system requirements: " + readmeFile);
    const messages = ["zh", "en"].includes(lang) ? {} : JSON.parse(await readFile(join(websiteRoot, "src/locales", lang + ".json"), "utf8"));
    translations[lang] = {
      operatingSystem:
        requirements
          .match(/^[-*]\s+\*\*[^*]+\*\*\s*[:：]\s*(.+)$/m)?.[1]
          ?.trim() ?? "Windows x64",
      requirements: render(requirements, (href) => {
        const [target, fragment] = href.split("#");
        return docs.routes[target]
          ? docs.routes[target] + (fragment ? "#" + fragment : "")
          : href;
      }),
      privacy: labelTables(lang === "zh" ? render(section(privacy, "简体中文"), href => href.startsWith("docs/") ? repo + "/blob/main/" + href : href) : en ? privacyEnglish : translateHtml(privacyEnglish, messages)),
      signing: render(section(readme, "code signing policy"), (href) =>
        href.startsWith("PRIVACY.md") ? prefix + "/privacy/" : href,
      ),
    };
  }
  await putIfChanged(assetsDir, "argus.svg", await readFile(join(root, "resources/logo.svg")));
  // Supporters' avatars for the thanks cards on the home page (scripts/supporters.mjs).
  const avatars = join(websiteRoot, "src/data/supporters");
  for (const file of await readdir(avatars)) await putIfChanged(assetsDir, "supporters/" + file, await readFile(join(avatars, file)));
  // Browsers ask for /favicon.ico regardless of the declared icon.
  await putIfChanged(join(websiteRoot, "public"), "favicon.ico", await readFile(join(root, "resources/icons/icon.ico")));
  await putIfChanged(
    assetsDir,
    "geist-license.txt",
    await readFile(new URL("./LICENSE", import.meta.resolve("@fontsource-variable/geist"))),
  );
  await previews(at);
  const contact = JSON.parse(
    await readFile(join(websiteRoot, "public-settings.json"), "utf8"),
  ).emailRoutingVerified;
  // Root folders are listed in the order of the switcher.
  await putIfChanged(contentDir, "docs/meta.json", JSON.stringify({ title: "文档", pages: editions.map((e) => e.folder) }, null, 2));
  written.add(join(contentDir, "docs/meta.json"));
  await prune(join(contentDir, "docs"), written);
  const changelog = await changelogs(release, at);
  await putIfChanged(
    contentDir,
    "site.json",
    JSON.stringify(
      {
        format: siteFormat,
        release,
        softwareLanguages: git(["ls-tree", "--name-only", release.tag + ":locales"]).toString().trim().split("\n").filter(tag => /^[a-z]{2}-[A-Z]{2}$/.test(tag)),
        translations,
        editions,
        contact,
        changelog,
        activity: activity(docs.ref),
        repository: await repository(),
      },
      null,
      2,
    ),
  );
  return { editions };
}
