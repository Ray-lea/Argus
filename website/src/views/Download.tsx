import type { CSSProperties } from "react";
import { longDate } from "@/components/home/Releases";
import { CopyButton } from "@/components/site/CopyButton";
import { ReleaseBody } from "@/views/Changelog";
import { Icon, type IconName } from "@/components/site/Icon";
import { SiteFrame } from "@/components/site/SiteFrame";
import { SiteLink } from "@/components/site/SiteLink";
import { pageMetadata } from "@/lib/page";
import { downloadOrigin, localized, mebibytes, repo, site, softwareSchema, text, type Language } from "@/lib/site";

const title = (lang: Language) => (lang === "en" ? "Download Argus for Windows" : "下载 Argus Windows 版");
const description = (lang: Language) =>
  lang === "en"
    ? "Download the Argus Windows x64 installer or portable executable, read the system requirements and verify the release checksums."
    : "下载 Argus 的 Windows x64 安装版或免安装版，查看系统要求、发布说明和 SHA-256 校验值。";

export const downloadMetadata = (lang: Language) =>
  pageMetadata({ lang, path: "/download/", title: title(lang), description: description(lang) });

const copy = {
  zh: {
    h1: "下载 Argus",
    lead: "免费开源的 Windows 任务管理器。安装版与免安装版共用设置和历史数据。",
    released: "发布",
    system: "Windows 10 / 11 · x64",
    recommended: "推荐",
    copy: "复制 SHA-256",
    copied: "已复制",
    after: ["安装之后", "从下载到随时呼出，只需几步。"],
    steps: [
      ["运行安装程序", "安装到 Program Files 并创建开始菜单快捷方式；免安装版直接运行即可。"],
      ["完成首次引导", "依次设置语言、外观、常驻与快捷键、更新方式，以及可选的特权服务；任何一步都可跳过。"],
      ["随时呼出", "按 Ctrl+Alt+Esc 显示或隐藏窗口；在“设置 → 权限”开启替换后，Ctrl+Shift+Esc 也会打开 Argus。"],
    ],
    notes: "更新内容",
    all: "完整更新日志",
    sums: "SHA256SUMS.txt",
    checks: "校验与签名：",
    requirements: "系统要求",
    history: ["历史版本", "早期版本可在 GitHub 发布页下载。"],
    withdrawn: "已撤回",
    source: "GitHub 源代码",
  },
  en: {
    h1: "Download Argus",
    lead: "A free, open-source task manager for Windows. The installer and the portable edition share settings and history.",
    released: "Released",
    system: "Windows 10 / 11 · x64",
    recommended: "Recommended",
    copy: "Copy SHA-256",
    copied: "Copied",
    after: ["After downloading", "A few steps from the download to a window that is always one key away."],
    steps: [
      ["Run the installer", "It installs to Program Files and adds a Start menu shortcut. The portable edition simply runs."],
      ["Finish the first-run guide", "Language, appearance, tray and shortcut, updates and the optional privileged service; every step can be skipped."],
      ["Open it any time", "Ctrl+Alt+Esc shows or hides the window. With the replacement on in Settings → Permissions, Ctrl+Shift+Esc opens Argus too."],
    ],
    notes: "What's new",
    all: "Full changelog",
    sums: "SHA256SUMS.txt",
    checks: "Checksums and signing:",
    requirements: "System requirements",
    history: ["Earlier releases", "Earlier versions are available on GitHub."],
    withdrawn: "Withdrawn",
    source: "Source code on GitHub",
  },
};

export function Download({ lang }: { lang: Language }) {
  const en = lang === "en",
    t = copy[lang],
    r = site.release;
  const notes = site.changelog[lang];
  const current = notes[0]?.version === r.version ? notes[0] : null;
  const packages: { file: typeof r.files.installer; title: string; icon: IconName; points: string[]; action: string }[] = [
    {
      file: r.files.installer,
      title: en ? "Installer" : "安装版",
      icon: "box",
      points: en
        ? ["Installs for all users in Program Files", "Start menu shortcut, uninstall from Windows Settings", "Updates in the app, downloaded in the background"]
        : ["为所有用户安装到 Program Files", "创建开始菜单快捷方式，可在 Windows 设置中卸载", "支持应用内更新，新版本在后台下载"],
      action: en ? "Download installer" : "下载安装版",
    },
    {
      file: r.files.portable,
      title: en ? "Portable" : "免安装版",
      icon: "file",
      points: en
        ? ["One executable, runs without installing", "Shares settings and history with the installer", "Tells you about new versions; update it by hand"]
        : ["单个可执行文件，无需安装即可运行", "与安装版共用设置和历史数据", "检查新版本，从发布页手动更新"],
      action: en ? "Download portable" : "下载免安装版",
    },
  ];
  return (
    <SiteFrame lang={lang} path="/download/" title={title(lang)} schema={softwareSchema(lang, description(lang))}>
      <div className="site-wrap download">
        <header className="dl-hero">
          <div className="cta-mark" aria-hidden="true">
            <img src="/assets/argus.svg" width="64" height="64" alt="" />
          </div>
          <h1 data-rise="">{t.h1}</h1>
          <p className="dl-lead" data-rise="2">
            {t.lead}
          </p>
          <ul className="dl-meta" data-rise="2">
            <li>
              <a href={repo + "/releases/tag/" + r.tag}>
                <Icon name="tag" size={15} />
                <span className="figures">{r.tag}</span>
              </a>
            </li>
            <li>
              <Icon name="clock" size={15} />
              <time dateTime={current?.date ?? r.published}>
                {en ? t.released + " " : ""}
                {longDate(current?.date ?? r.published.slice(0, 10), lang)}
                {en ? "" : t.released}
              </time>
            </li>
            <li>
              <Icon name="monitor" size={15} />
              {t.system}
            </li>
            <li>
              <Icon name="scale" size={15} />
              AGPL-3.0
            </li>
          </ul>
        </header>

        <div className="packages">
          {packages.map((pkg, i) => (
            <article key={pkg.file.name} className={i === 0 ? "package is-primary" : "package"} data-spotlight="" data-rise={i ? "2" : ""}>
              <div className="package-head">
                <span className="package-icon">
                  <Icon name={pkg.icon} size={24} />
                </span>
                <h2>{pkg.title}</h2>
                {i === 0 && <span className="package-badge">{t.recommended}</span>}
              </div>
              <ul className="package-points">
                {pkg.points.map((point) => (
                  <li key={point}>
                    <Icon name="check" size={16} />
                    {point}
                  </li>
                ))}
              </ul>
              <a className={i === 0 ? "btn is-primary package-get" : "btn package-get"} href={pkg.file.url}>
                <Icon name="download" size={18} />
                {pkg.action}
                <span className="package-size figures">{mebibytes(pkg.file.size)} MiB</span>
              </a>
              <div className="package-file">
                <span className="figures">{pkg.file.name}</span>
                <details>
                  <summary>SHA-256</summary>
                  <span className="package-hash">
                    <code>{pkg.file.sha256}</code>
                    <CopyButton text={pkg.file.sha256} label={t.copy} done={t.copied} />
                  </span>
                </details>
              </div>
            </article>
          ))}
        </div>

        <section className="dl-steps" aria-labelledby="after-title">
          <header>
            <h2 id="after-title" data-rise="">
              {t.after[0]}
            </h2>
            <p data-rise="2">{t.after[1]}</p>
          </header>
          <ol>
            {t.steps.map(([name, detail], i) => (
              <li key={name} style={{ "--i": i } as CSSProperties} data-rise={String(i + 1)}>
                <span className="dl-step-number figures">{i + 1}</span>
                <h3>{name}</h3>
                <p>{detail}</p>
              </li>
            ))}
          </ol>
        </section>

        <div className="dl-more">
          {current && (
            <section className="panel dl-notes" data-rise="">
              <header className="dl-notes-head">
                <h2>
                  {r.tag} {t.notes}
                </h2>
                <SiteLink lang={lang} className="text-link" href={localized(lang, "/changelog/")}>
                  {t.all} <Icon name="arrow" size={14} />
                </SiteLink>
              </header>
              <div className="dl-release">
                <ReleaseBody note={current} />
              </div>
              <h3 className="dl-history-title">{t.history[0]}</h3>
              <ul className="dl-history">
                {notes.slice(1).map((note) => (
                  <li key={note.version}>
                    <SiteLink lang={lang} className="figures" href={localized(lang, "/changelog/") + "#v" + note.version}>
                      v{note.version}
                    </SiteLink>
                    <time dateTime={note.date}>{longDate(note.date, lang)}</time>
                    {note.withdrawn && <span className="release-badge is-muted">{t.withdrawn}</span>}
                  </li>
                ))}
              </ul>
              <p className="dl-history-note">{t.history[1]}</p>
            </section>
          )}
          <div className="dl-side">
            <section className="panel site-prose" data-rise="2">
              <h2>{t.requirements}</h2>
              <div dangerouslySetInnerHTML={{ __html: site.translations[lang].requirements }} />
            </section>
            <p className="dl-fine">
              {t.checks}
              <a href={downloadOrigin + "/" + r.tag + "/SHA256SUMS.txt"}>{t.sums}</a>
              <SiteLink lang={lang} href={localized(lang, "/signing/")}>
                {text[lang].signing}
              </SiteLink>
              <a href={repo}>{t.source}</a>
            </p>
          </div>
        </div>
      </div>
    </SiteFrame>
  );
}
