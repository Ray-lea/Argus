import data from "../../content/site.json";
import { downloadOrigin, origin, repo } from "../../scripts/release.mjs";
import { languages, localized, notesLanguage, languageInfo, type Language } from "./languages";
import { withLanguages } from "./site-i18n";

export { downloadOrigin, origin, repo };

export type { Language } from "./languages";
export { localized } from "./languages";
export type Policy = "privacy" | "signing" | "security";
export const policies: Policy[] = ["privacy", "signing", "security"];
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
  softwareLanguages: string[];
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
  // Each language that has release notes; the others fall back to changelogLanguage.
  changelog: Partial<Record<Language, ReleaseNote[]>>;
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
if (data.format !== 7)
  throw Error("content/site.json is from an older version of the website scripts. Restart start-website.bat.");
const snapshot = data as unknown as Snapshot;
// The language a page's release notes are in: its own changelog, or the
// source language's when the release has no edition in it.
export const changelogLanguage = (lang: Language): Language =>
  snapshot.changelog[lang]?.length ? lang : notesLanguage(lang);
export const site = {
  ...snapshot,
  changelog: Object.fromEntries(languages.map(({ id }) => [id, snapshot.changelog[changelogLanguage(id)]])) as Record<Language, ReleaseNote[]>,
  translations: Object.fromEntries(languages.map(({ id }) => [id, snapshot.translations[id] ?? snapshot.translations[notesLanguage(id)]])) as Snapshot["translations"],
};
export const [stable, development] = site.editions;
// The first native release documents the interface demonstrated on the home page.
const [releaseMajor, releaseMinor] = site.release.version.split(".").map(Number);
export const nativeStable = releaseMajor > 0 || (releaseMajor === 0 && releaseMinor >= 4);
// The development base is nested inside the stable one; the last match is the closest.
export const editionOf = (path: string) =>
  site.editions.findLast((edition) => path.startsWith(edition.base)) ?? stable;
export const text = withLanguages({
  zh: {
    home: "首页",
    docs: "文档",
    download: "下载",
    changelog: "更新日志",
    started: "下载已开始",
    install: "下载 Windows 版",
    skip: "跳到正文",
    privacy: "隐私政策",
    signing: "代码签名策略",
    security: "安全报告",
    contact: "联系维护者",
    questions: "常见问题",
    source: "源代码",
    language: "语言",
    search: "搜索文档",
    theme: "主题",
    themeOptions: { auto: "跟随系统", light: "浅色", dark: "深色" },
  },
  en: {
    home: "Home",
    docs: "Docs",
    download: "Download",
    changelog: "Changelog",
    started: "Download started",
    install: "Download for Windows",
    skip: "Skip to content",
    privacy: "Privacy",
    signing: "Code signing policy",
    security: "Security",
    contact: "Contact",
    questions: "Questions",
    source: "Source code",
    language: "Language",
    search: "Search docs",
    theme: "Theme",
    themeOptions: { auto: "System", light: "Light", dark: "Dark" },
  },
});
// What Argus is: the home page's head tags and lead, the structured data and llms.txt.
export const about = withLanguages({
  zh: {
    title: "Argus · 快速、现代、信息丰富的 Windows 任务管理器",
    description:
      "Argus 是开源的 Windows 任务管理器：原生 GPU 绘制界面，曲线随刷新率流动，可查端口、文件占用、每进程磁盘读写、磁盘健康和最近 7 天的历史。",
    lead: "Argus 用 Rust 直接读取 Windows 原生接口，界面由 GPU 绘制。进程、性能、服务、启动项、网络连接与文件占用，集中在一个流畅的窗口里。",
  },
  en: {
    title: "Argus · A fast, modern, information-rich Windows task manager",
    description:
      "Argus is an open-source task manager for Windows: a native GPU-drawn interface, curves at the display's refresh rate, ports, file locks, per-process disk I/O, disk health and seven days of history.",
    lead: "Argus reads Windows directly in Rust and draws its window on the GPU. Processes, performance, services, startup apps, network connections and file locks share one fluid window.",
  },
});
export const jsonLd = (data: unknown) =>
  JSON.stringify(data).replaceAll("<", "\\u003c");
export const sourceFor = (lang: Language) => languageInfo(lang).tag;
export const mebibytes = (bytes: number) => (bytes / 1048576).toFixed(1);
// The withdrawn releases are left out; the last entry is the first release.
export const releases = (lang: Language) => site.changelog[lang].filter((note) => !note.withdrawn);
// The same entity on every page that describes it; the home page adds its features.
export function softwareSchema(lang: Language, features?: string[]) {
  const { release } = site;
  return {
    "@context": "https://schema.org",
    "@type": "SoftwareApplication",
    "@id": origin + "/#software",
    name: "Argus",
    url: origin + localized(lang),
    description: about[lang].description,
    image: origin + "/assets/argus.svg",
    screenshot: origin + "/assets/preview-" + notesLanguage(lang) + "-dark.png",
    applicationCategory: "UtilitiesApplication",
    operatingSystem: site.translations[lang].operatingSystem,
    inLanguage: site.softwareLanguages,
    softwareVersion: release.version,
    datePublished: releases(lang).at(-1)?.date,
    dateModified: release.published.slice(0, 10),
    downloadUrl: release.files.installer.url,
    installUrl: origin + localized(lang, "/download/"),
    fileSize: mebibytes(release.files.installer.size) + " MiB",
    releaseNotes: origin + localized(lang, "/changelog/"),
    softwareHelp: { "@type": "CreativeWork", url: origin + stable.base },
    featureList: features,
    license: repo + "/blob/" + release.tag + "/LICENSE",
    isAccessibleForFree: true,
    sameAs: [repo],
    offers: { "@type": "Offer", price: "0", priceCurrency: "USD" },
  };
}
