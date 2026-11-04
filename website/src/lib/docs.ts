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
