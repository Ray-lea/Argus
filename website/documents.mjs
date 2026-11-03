import { execFileSync } from "node:child_process";
import { posix } from "node:path";
import MarkdownIt from "markdown-it";
import GithubSlugger from "github-slugger";
import { repo } from "./release.mjs";

export const escape = (value) =>
  String(value).replace(
    /[&<>"']/g,
    (c) =>
      ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;" })[
        c
      ],
  );
const git = (root, args) =>
  execFileSync("git", args, { cwd: root, maxBuffer: 32 * 1024 * 1024 });
export const fromTag = (root, tag, file) =>
  git(root, ["show", `${tag}:${file}`]);
const urlFor = (file) =>
  "/" +
  (file.endsWith("/README.md")
    ? file.slice(0, -"README.md".length)
    : file.slice(0, -3) + "/");

export function releasedDocs(root, tag) {
  const names = git(root, [
    "ls-tree",
    "-r",
    "-z",
    "--name-only",
    tag,
    "--",
    "docs",
  ])
    .toString()
    .split("\0")
    .filter(Boolean);
  const files = names.filter(
    (file) => file.endsWith(".md") && !file.endsWith("/AGENTS.md"),
  );
  if (!files.includes("docs/README.md"))
    throw Error(`No documentation in ${tag}`);
  const routes = new Map(files.map((f) => [f, urlFor(f)]));
  const pages = files.map((file) => {
    const source = fromTag(root, tag, file).toString("utf8");
    const rendered = markdown(source, (link) => {
      if (/^(?:[a-z][\w+.-]*:|\/\/|#)/i.test(link)) return link;
      const [target, fragment] = link.split(/#(.*)/s);
      let decoded;
      try {
        decoded = decodeURIComponent(target);
      } catch {
        decoded = target;
      }
      const dest = posix.normalize(posix.join(posix.dirname(file), decoded));
      const resolved =
        routes.get(dest) ??
        (names.includes(dest) && !dest.endsWith(".md")
          ? `${repo}/raw/refs/tags/${tag}/${dest}`
          : `${repo}/blob/${tag}/${dest}`);
      return resolved + (fragment !== undefined ? `#${fragment}` : "");
    });
    return { file, path: routes.get(file), ...rendered };
  });
  return pages;
}

export function markdown(source, rewrite = (url) => url) {
  const md = new MarkdownIt({
    html: false,
    linkify: false,
    typographer: false,
  });
  const tokens = md.parse(source, {});
  const slugger = new GithubSlugger();
  const headings = [];
  for (let i = 0; i < tokens.length; i++) {
    const token = tokens[i];
    if (token.type === "heading_open") {
      const text = (tokens[i + 1]?.children ?? [])
        .map((c) => c.content)
        .join("");
      const id = slugger.slug(text);
      token.attrSet("id", id);
      headings.push({ level: Number(token.tag.slice(1)), text, id });
    }
    for (const child of token.children ?? []) {
      const key =
        child.type === "link_open"
          ? "href"
          : child.type === "image"
            ? "src"
            : null;
      if (key) child.attrSet(key, rewrite(child.attrGet(key) ?? ""));
    }
  }
  const text = tokens
    .filter((t) => t.type === "inline")
    .map((t) => t.content)
    .join(" ");
  return {
    title: headings.find((h) => h.level === 1)?.text ?? "",
    headings,
    text,
    html: md.renderer.render(tokens, md.options, {}),
  };
}

export const groups = [
  ["guide", "使用指南"],
  ["development", "开发指南"],
  ["architecture", "架构"],
  ["design", "设计"],
];
export function sidebar(pages, current) {
  const link = (p) =>
    `<a href="${p.path}"${p.path === current ? ' aria-current="page"' : ""}>${escape(p.title)}</a>`;
  const sections = groups.map(([dir, label]) => {
    const items = pages
      .filter((p) => p.file.startsWith(`docs/${dir}/`))
      .sort(
        (a, b) =>
          Number(b.file.endsWith("/README.md")) -
          Number(a.file.endsWith("/README.md")),
      );
    return `<section><h3>${label}</h3>${items.map(link).join("")}</section>`;
  });
  return `<a href="/docs/" class="index-link">文档首页</a>${sections.join("")}<section><h3>项目</h3>${pages
    .filter((p) => !p.file.slice(5).includes("/") && p.path != "/docs/")
    .map(link)
    .join("")}</section>`;
}
