import type { MetadataRoute } from "next";
import { canonicalPath } from "@/lib/docs";
import { origin, policies, site } from "@/lib/site";
import { pathOf, source } from "@/lib/source";

export const dynamic = "force-static";

// Only canonical addresses: development pages that the stable edition also has are left out.
export default function sitemap(): MetadataRoute.Sitemap {
  const released = site.release.published.slice(0, 10);
  const pages: [path: string, lastModified?: string][] = [
    ...["/", "/download/", "/changelog/"].map((path): [string, string] => [path, released]),
    ...policies.map((policy): [string] => ["/" + policy + "/"]),
  ];
  const localized = pages.flatMap(([path, lastModified]) =>
    ["", "/en"].map((prefix) => ({
      url: origin + prefix + path,
      lastModified,
      alternates: { languages: { "zh-CN": origin + path, en: origin + "/en" + path } },
    })),
  );
  const docs = source
    .getPages()
    .filter((page) => canonicalPath(page) === pathOf(page))
    .map((page) => ({ url: origin + pathOf(page), lastModified: page.data.updated || undefined }));
  return [...localized, ...docs];
}
