import type { Metadata } from "next";
import { localized, origin, type Language } from "./site";

export interface PageInfo {
  lang: Language;
  /** The path without the language prefix, e.g. "/download/". */
  path: string;
  title: string;
  description: string;
  /** Whether the page exists in both languages. */
  alternate?: boolean;
  noindex?: boolean;
}

// Head tags of the site's own pages: canonical and language alternates,
// Open Graph. The root layouts add the icon, site name and Twitter card.
export function pageMetadata({
  lang,
  path,
  title,
  description,
  alternate = true,
  noindex = false,
}: PageInfo): Metadata {
  const url = origin + localized(lang, path);
  return {
    title: { absolute: title },
    description,
    // A page kept out of the index names no canonical address.
    alternates: {
      canonical: noindex ? undefined : url,
      languages: alternate
        ? {
            "zh-CN": origin + localized("zh", path),
            en: origin + localized("en", path),
            "x-default": origin + path,
          }
        : undefined,
    },
    robots: noindex ? { index: false, follow: true } : undefined,
    // A page's openGraph replaces the layout's, so the shared fields repeat here.
    openGraph: {
      type: "website",
      siteName: "Argus",
      title,
      description,
      url,
      images: origin + "/assets/preview-" + lang + "-dark.png",
      locale: lang === "zh" ? "zh_CN" : "en_US",
    },
  };
}

// Structured data: the page's own schema and, below the home page, a breadcrumb.
export function pageGraph({
  lang,
  path,
  title,
  noindex = false,
  schema,
}: Pick<PageInfo, "lang" | "path" | "title" | "noindex"> & { schema?: unknown }) {
  const url = origin + localized(lang, path);
  return [
    schema,
    path !== "/" &&
      !noindex && {
        "@context": "https://schema.org",
        "@type": "BreadcrumbList",
        itemListElement: [
          {
            "@type": "ListItem",
            position: 1,
            name: "Argus",
            item: origin + localized(lang),
          },
          { "@type": "ListItem", position: 2, name: title, item: url },
        ],
      },
  ].filter(Boolean);
}
