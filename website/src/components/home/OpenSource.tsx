import { copyFor, localize } from "@/lib/site-i18n";
import type { CSSProperties } from "react";
import { Icon, type IconName } from "@/components/site/Icon";
import { SiteLink } from "@/components/site/SiteLink";
import { localized, repo, site, type Language } from "@/lib/site";
import { dateLocale } from "@/lib/languages";

// The project in figures and in the open: GitHub's counts (left out when the
// build could not fetch them), the published releases, the commit history and
// how to build, report and talk about Argus.
export const compact = (n: number) => (n < 1000 ? String(n) : (n / 1000).toFixed(n < 10000 ? 1 : 0).replace(/\.0$/, "") + "k");
const released = (lang: Language) => site.changelog[lang].filter((note) => !note.withdrawn);
const shortDate = (date: string, lang: Language) => {
  const [y, m, d] = date.split("-").map(Number);
  return new Date(Date.UTC(y, m - 1, d)).toLocaleDateString(dateLocale(lang), { month: "short", day: "numeric", timeZone: "UTC" });
};

export function ProjectStats({ lang }: { lang: Language }) {
  const number = (n: number) => n.toLocaleString(dateLocale(lang));
  const en = lang !== "zh",
    gh = site.repository,
    versions = released(lang),
    { activity } = site;
  const first = versions.at(-1)?.date ?? "";
  const stats: [string, string, IconName, string][] = gh
    ? [
        [compact(gh.stars), localize(lang, en ? "GitHub stars" : "GitHub 星标"), "star", (localize(lang, en ? "As of " : "截至 ")) + shortDate(gh.checked.slice(0, 10), lang)],
        [number(gh.downloads), localize(lang, en ? "Downloads" : "下载次数"), "download", localize(lang, en ? "From GitHub Releases" : "GitHub Releases 累计")],
      ]
    : [];
  const items: [string, string, IconName, string][] = localize(lang, [
    ...stats,
    [String(versions.length), localize(lang, en ? "Releases" : "正式版本"), "tag", (localize(lang, en ? "Since " : "首个版本发布于 ")) + shortDate(first, lang)],
    [number(activity.total), localize(lang, en ? "Commits" : "提交次数"), "commit", (localize(lang, en ? "Since " : "开发始于 ")) + shortDate(activity.first, lang)],
  ]);
  return (
    <ul className="stat-band" data-rise="">
      {items.map(([value, label, icon, note]) => (
        <li key={label}>
          <span className="stat-label">
            <Icon name={icon} size={15} />
            {label}
          </span>
          <b className="figures">{value}</b>
          <small>{note}</small>
        </li>
      ))}
    </ul>
  );
}

export function OpenSource({ lang, guide }: { lang: Language; guide: string }) {
  const number = (n: number) => n.toLocaleString(dateLocale(lang));
  const en = lang !== "zh",
    gh = site.repository,
    { days, total } = site.activity;
  const top = Math.max(1, ...days.map(([, n]) => n));
  const recent = days.reduce((sum, [, n]) => sum + n, 0);
  const tags = ["Rust", "GPUI", "Direct3D 11", "NT API", "Windows 11"];
  const community: [string, string, string][] = localize(lang, [
    ...(localize(lang, en ? [] : ([["QQ 群 453803497", "反馈问题、提出建议或交流使用心得", "https://qm.qq.com/q/97YqJAQRNK"]] as [string, string, string][]))),
    [localize(lang, en ? "GitHub Issues" : "GitHub 问题"), localize(lang, en ? "Report a bug or suggest a feature" : "报告缺陷或提出功能建议"), repo + "/issues/new/choose"],
    [localize(lang, en ? "Security reports" : "安全报告"), localize(lang, en ? "Report a vulnerability privately" : "私下报告安全漏洞"), localized(lang, "/security/")],
  ]);
  return (
    <div className="oss-grid">
      <article className="cell cell-repo" data-spotlight="" data-rise="">
        <div className="repo-head">
          <span className="repo-mark">
            <Icon name="github" size={22} />
          </span>
          <div>
            <a href={repo}>Ray-lea / Argus</a>
            <small>{localize(lang, en ? "A fast, modern, information-rich Windows task manager" : "快速、现代、信息丰富的 Windows 任务管理器")}</small>
          </div>
        </div>
        <ul className="repo-stats figures">
          {gh && (
            <li>
              <Icon name="star" size={15} />
              {number(gh.stars)} {localize(lang, en ? "stars" : "星标")}
            </li>
          )}
          {gh && (
            <li>
              <Icon name="fork" size={15} />
              {number(gh.forks)} {localize(lang, en ? "forks" : "复刻")}
            </li>
          )}
          <li>
            <Icon name="scale" size={15} />
            AGPL-3.0
          </li>
        </ul>
        <ul className="repo-tags">
          {tags.map((tag) => (
            <li key={tag}>{tag}</li>
          ))}
        </ul>
        <div className="repo-actions">
          <a className="btn is-primary" href={repo}>
            <Icon name="star" size={16} />
            {localize(lang, en ? "Star on GitHub" : "在 GitHub 上加星标")}
          </a>
          <a className="btn" href={repo + "/fork"}>
            <Icon name="fork" size={16} />
            {localize(lang, en ? "Fork" : "复刻")}
          </a>
        </div>
      </article>
      <article className="cell cell-activity" data-spotlight="" data-rise="2">
        <h3>{localize(lang, en ? "Built in the open" : "持续开发")}</h3>
        <p>
          {localize(lang, recent === total
            ? en
              ? `${number(total)} commits since development began on ${shortDate(days[0][0], lang)}.`
              : `自 ${shortDate(days[0][0], lang)}开始开发以来提交 ${number(total)} 次。`
            : en
              ? `${number(recent)} commits in the last ${days.length} days, ${number(total)} in all.`
              : `最近 ${days.length} 天提交 ${number(recent)} 次，累计 ${number(total)} 次。`)}
        </p>
        <div className="commit-bars" role="img" aria-label={localize(lang, en ? "Commits per day" : "每日提交次数")}>
          {days.map(([day, n]) => (
            <i key={day} style={{ "--h": n / top } as CSSProperties} title={`${day} · ${n}`} data-empty={n ? undefined : ""} />
          ))}
        </div>
        <div className="commit-axis figures">
          <span>{shortDate(days[0][0], lang)}</span>
          <span>{shortDate(days.at(-1)![0], lang)}</span>
        </div>
      </article>
      <article className="cell cell-build" data-spotlight="" data-rise="">
        <h3>{localize(lang, en ? "Build from source" : "从源码构建")}</h3>
        <p>
          {localize(lang, en
            ? "Needs the Visual Studio C++ Build Tools, stable MSVC Rust, Node.js and pnpm."
            : "需要 Visual Studio C++ 生成工具、stable MSVC Rust、Node.js 与 pnpm。")}
        </p>
        <pre className="terminal">
          <code>
            <span className="term-line">git clone {repo}.git</span>
            <span className="term-line">cd Argus</span>
            <span className="term-line">rustup override set stable-x86_64-pc-windows-msvc</span>
            <span className="term-line">pnpm install</span>
            <span className="term-line">start.bat build</span>
            <span className="term-out">{localize(lang, en ? "# Writes target\\release\\argus.exe" : "# 生成 target\\release\\argus.exe")}</span>
          </code>
        </pre>
        <SiteLink lang={lang} className="text-link" href={guide}>
          {localize(lang, en ? "Development guide (Chinese)" : "开发指南")} <Icon name="arrow" size={14} />
        </SiteLink>
      </article>
      <article className="cell cell-community" data-spotlight="" data-rise="2">
        <h3>{localize(lang, en ? "Feedback and support" : "交流与反馈")}</h3>
        <ul className="community">
          {community.map(([title, note, href]) => (
            <li key={title}>
              <SiteLink lang={lang} href={href}>
                <span>
                  <b>{title}</b>
                  <small>{note}</small>
                </span>
                <Icon name={href.startsWith("/") ? "arrow" : "external"} size={15} />
              </SiteLink>
            </li>
          ))}
        </ul>
      </article>
    </div>
  );
}
