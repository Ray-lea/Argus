import type { MetadataRoute } from "next";
import { origin, policies } from "@/lib/site";
import { pathOf, source } from "@/lib/source";

export const dynamic = "force-static";

export default function sitemap(): MetadataRoute.Sitemap {
  const localized = ["/", "/download/", "/changelog/", ...policies.map((policy) => "/" + policy + "/")].flatMap((path) =>
    ["", "/en"].map((prefix) => ({
      url: origin + prefix + path,
      alternates: { languages: { "zh-CN": origin + path, en: origin + "/en" + path } },
    })),
  );
  return [...localized, ...source.getPages().map((page) => ({ url: origin + pathOf(page) }))];
}
