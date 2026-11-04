import { posix } from "node:path";
import { pathOf, type DocsPage } from "./source";
import { development, origin, repo, stable, type Edition } from "./site";

export const editionOf = (page: DocsPage): Edition =>
  page.slugs[0] === "dev" ? development : stable;

// Documents link to each other and to repository files with paths relative to
// docs/. Other documents open in the same edition; any other file opens on
// GitHub at the commit the edition was built from.
export function resolveHref(edition: Edition, source: string, href?: string) {
  if (!href || /^(?:[a-z][\w+.-]*:|\/|#)/i.test(href)) return href;
  const [target, hash] = href.split(/#(.*)/s);
  const file = posix.normalize(posix.join(posix.dirname(source), decodeURIComponent(target)));
  const route = edition.routes[file];
  return (
    (route ?? repo + "/blob/" + edition.ref + "/" + file.replace(/\/$/, "")) +
    (hash ? "#" + hash : "")
  );
}

export const markdownUrl = (page: DocsPage) => "/llms.mdx" + pathOf(page) + "content.md";

// A development page that the stable edition also has points search engines
// at the stable one, which describes the released version; pages only in
// development stand on their own until the next release.
export const canonicalPath = (page: DocsPage) =>
  (editionOf(page) === development && stable.routes[page.data.source]) || pathOf(page);

// The description of an edition's home, as its subtitle reads.
export const editionSummary = (edition: Edition) =>
  edition === development
    ? "main 分支上的最新文档，包含尚未发布的改动。"
    : "与 " + edition.label.replace("稳定版 ", "") + " 一同发布的使用指南、开发指南、架构与设计说明。";

export const pageDescription = (page: DocsPage) =>
  page.data.source === "docs/README.md" ? editionSummary(editionOf(page)) : page.data.description;

// The page's Markdown below its title, for language models and "Copy Markdown":
// links resolved to absolute addresses, headings without the [#id] that sets their anchors.
export async function markdownBody(page: DocsPage) {
  const edition = editionOf(page);
  return (await page.data.getText("processed"))
    .replace(/^(#{1,6} .*?) \[#[^\]\n]+\]$/gm, "$1")
    .replace(/(\]\()([^)\s]+)(\))/g, (_, open, href: string, close) => {
      let resolved = resolveHref(edition, page.data.source, href) ?? href;
      if (resolved.startsWith("#")) resolved = pathOf(page) + resolved;
      return open + (resolved.startsWith("/") ? origin + resolved : resolved) + close;
    });
}

export function articleSchema(page: DocsPage) {
  const url = origin + pathOf(page);
  return {
    "@context": "https://schema.org",
    "@type": "TechArticle",
    "@id": url + "#article",
    headline: page.data.heading,
    description: pageDescription(page),
    url,
    inLanguage: "zh-CN",
    dateModified: page.data.updated || undefined,
    isPartOf: { "@id": origin + "/#website" },
    about: { "@id": origin + "/#software" },
  };
}

export function pageTitle(page: DocsPage) {
  const edition = editionOf(page);
  const name = page.data.source === "docs/README.md" ? "Argus 文档" : page.data.heading;
  return name + (edition === development ? " · 开发版" : "") + (name === "Argus 文档" ? "" : " · Argus 文档");
}

export function breadcrumbSchema(page: DocsPage) {
  const edition = editionOf(page);
  const items = [
    { name: "Argus", item: origin + "/" },
    { name: edition.label, item: origin + edition.base },
  ];
  if (page.data.source !== "docs/README.md")
    items.push({ name: page.data.heading, item: origin + pathOf(page) });
  return {
    "@context": "https://schema.org",
    "@type": "BreadcrumbList",
    itemListElement: items.map((entry, i) => ({ "@type": "ListItem", position: i + 1, ...entry })),
  };
}
