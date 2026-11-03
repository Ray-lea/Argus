import { readFileSync } from "node:fs";
import { join } from "node:path";
import { siteInput } from "../../scripts/paths.mjs";
import { origin, repo } from "../../scripts/release.mjs";

export { origin, repo };
export type Language = "zh" | "en";
export type Policy = "privacy" | "signing" | "security";
export const policies: Policy[] = ["privacy", "signing", "security"];
export const localized = (lang: Language, path = "/") =>
  (lang === "en" ? "/en" : "") + path;
interface Download {
  name: string;
  size: number;
  sha256: string;
  url: string;
  upstream: string;
}
interface Edition {
  ref: string;
  framework: string;
  label: string;
  base: string;
  routes: Record<string, string>;
}
interface Snapshot {
  release: {
    version: string;
    tag: string;
    published: string;
    beta: string;
    files: { installer: Download; portable: Download };
  };
  translations: Record<
    Language,
    {
      operatingSystem: string;
      requirements: string;
      signing: string;
      privacy: string;
    }
  >;
  editions: Edition[];
  contact: boolean;
}
export const site: Snapshot = JSON.parse(
  readFileSync(join(siteInput(), "site.json"), "utf8"),
);
export function editionName(edition: Edition, lang: Language) {
  if (lang === "zh") return edition.label;
  return edition.base === "/docs/"
    ? "Stable " + site.release.tag
    : "Development";
}
export const text = {
  zh: {
    home: "首页",
    docs: "文档",
    download: "下载",
    install: "下载 Windows 版",
    skip: "跳到正文",
    privacy: "隐私政策",
    signing: "代码签名策略",
    security: "安全报告",
    contact: "联系维护者",
    questions: "常见问题",
    source: "源代码",
    language: "English",
    stable: "稳定版",
    portable: "免安装版",
  },
  en: {
    home: "Home",
    docs: "Docs",
    download: "Download",
    install: "Download for Windows",
    skip: "Skip to content",
    privacy: "Privacy",
    signing: "Code signing policy",
    security: "Security",
    contact: "Contact",
    questions: "Questions",
    source: "Source code",
    language: "简体中文",
    stable: "Stable release",
    portable: "Portable edition",
  },
} as const;
export const jsonLd = (data: unknown) =>
  JSON.stringify(data).replaceAll("<", "\\u003c");
export const sourceFor = (lang: Language) => (lang === "en" ? "en" : "zh-CN");
export function softwareSchema(lang: Language, description: string) {
  const { release } = site;
  return {
    "@context": "https://schema.org",
    "@type": "SoftwareApplication",
    "@id": origin + "/#software",
    name: "Argus",
    url: origin + localized(lang),
    description,
    applicationCategory: "UtilitiesApplication",
    operatingSystem: site.translations[lang].operatingSystem,
    softwareVersion: release.version,
    dateModified: release.published.slice(0, 10),
    downloadUrl: release.files.installer.url,
    releaseNotes: repo + "/releases/tag/" + release.tag,
    license: repo + "/blob/" + release.tag + "/LICENSE",
    offers: { "@type": "Offer", price: "0", priceCurrency: "USD" },
  };
}
