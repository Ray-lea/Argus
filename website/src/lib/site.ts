import data from "../../content/site.json";
import { downloadOrigin, origin, repo } from "../../scripts/release.mjs";

export { downloadOrigin, origin, repo };

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
export interface DocLink {
  label: string;
  file: string;
  path: string;
  note: string;
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
  changelog: Record<Language, ReleaseNote[]>;
  // Commits per day up to the documented development commit.
  activity: { total: number; first: string; days: [string, number][] };
  // GitHub's figures, absent when they could not be fetched.
  repository: { stars: number; forks: number; releases: number; downloads: number; checked: string } | null;
}
// One release of the changelog: plain text for summaries, HTML (links kept) for the full notes.
export interface ReleaseNote {
  version: string;
  date: string;
  // GitHub's comparison with the previous release; empty for the first.
  compare: string;
  notice: string;
  withdrawn: boolean;
  intro: string;
  introHtml: string;
  groups: {
    title: string;
    kind: "feature" | "fix" | "performance" | "breaking" | "upgrade" | "known" | "other";
    items: { label: string; text: string; html: string }[];
  }[];
}
export interface Edition {
  id: "stable" | "dev";
  label: string;
  note: string;
  ref: string;
  base: string;
  folder: string;
  routes: Record<string, string>;
  nav: { label: string; items: DocLink[] }[];
}
// Written by scripts/content.mjs; raise siteFormat there when this shape changes.
if (data.format !== 6)
  throw Error("content/site.json is from an older version of the website scripts. Restart start-website.bat.");
export const site = data as unknown as Snapshot;
export const [stable, development] = site.editions;
// The development base is nested inside the stable one; the last match is the closest.
export const editionOf = (path: string) =>
  site.editions.findLast((edition) => path.startsWith(edition.base)) ?? stable;
export const text = {
  zh: {
    home: "首页",
    docs: "文档",
    download: "下载",
    changelog: "更新日志",
    install: "下载 Windows 版",
    skip: "跳到正文",
    privacy: "隐私政策",
    signing: "代码签名策略",
    security: "安全报告",
    contact: "联系维护者",
    questions: "常见问题",
    source: "源代码",
    language: "English",
    search: "搜索文档",
    theme: "主题",
    themeOptions: { auto: "跟随系统", light: "浅色", dark: "深色" },
  },
  en: {
    home: "Home",
    docs: "Docs",
    download: "Download",
    changelog: "Changelog",
    install: "Download for Windows",
    skip: "Skip to content",
    privacy: "Privacy",
    signing: "Code signing policy",
    security: "Security",
    contact: "Contact",
    questions: "Questions",
    source: "Source code",
    language: "简体中文",
    search: "Search docs",
    theme: "Theme",
    themeOptions: { auto: "System", light: "Light", dark: "Dark" },
  },
} as const;
export const jsonLd = (data: unknown) =>
  JSON.stringify(data).replaceAll("<", "\\u003c");
export const sourceFor = (lang: Language) => (lang === "en" ? "en" : "zh-CN");
export const mebibytes = (bytes: number) => (bytes / 1048576).toFixed(1);
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
