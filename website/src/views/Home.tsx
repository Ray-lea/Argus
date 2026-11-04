import type { CSSProperties, ReactNode } from "react";
import AccentPicker from "@/components/home/AccentPicker";
import CurveCompare from "@/components/home/CurveCompare";
import DeskWindow from "@/components/home/DeskWindow";
import FileDrop from "@/components/home/FileDrop";
import HeroTour from "@/components/home/HeroTour";
import LatencyBoard from "@/components/home/LatencyBoard";
import { OpenSource, ProjectStats } from "@/components/home/OpenSource";
import Releases, { longDate } from "@/components/home/Releases";
import ShowPage from "@/components/home/ShowPage";
import Taskbar from "@/components/home/Taskbar";
import { Icon } from "@/components/site/Icon";
import { SiteFrame } from "@/components/site/SiteFrame";
import { SiteLink } from "@/components/site/SiteLink";
import { differences, docs, fastest, latency, questions } from "@/lib/home-content";
import { pageMetadata } from "@/lib/page";
import { localized, mebibytes, origin, site, stable, type Language } from "@/lib/site";

const r = site.release,
  installer = r.files.installer;
// The changelog dates releases by the maintainer's day, GitHub by UTC.
const releasedOn = (lang: Language) => {
  const note = site.changelog[lang][0];
  return note?.version === r.version ? note.date : r.published.slice(0, 10);
};
const releases = (lang: Language) => site.changelog[lang].filter((note) => !note.withdrawn);
const copy = {
  en: {
    title: "Argus · A fast, modern, information-rich Windows task manager",
    description:
      "Argus is an open-source task manager for Windows: a native GPU-drawn interface, curves at the display's refresh rate, ports, file locks, per-process disk I/O and seven days of history.",
    h1: ["A fast, modern,", "information-rich task manager"],
    pill: ["Released ", "What's new"],
    replace: ["Replaces Task Manager", "opens Argus"],
    lead: "Argus reads Windows directly in Rust and draws its window on the GPU. Processes, performance, services, startup apps, network connections and file locks share one fluid window.",
    download: "Download for Windows",
    other: "Portable and checksums",
    facts: [r.tag + " · installer " + mebibytes(installer.size) + " MiB", "Windows 10 / 11 · x64", "Open source · AGPL-3.0"],
    intro: ["Made for finding what is wrong.", " Pages appear within tens of milliseconds and curves move with the display; ports, file locks, per-process disk I/O and seven days of history need no other tool."],
    fast: ["Fast", "From a click to the page on screen, Argus needs a fraction of Task Manager's time.", " Back from CPU to Processes: " + fastest.argus + " ms against " + fastest.native + " ms."],
    note: "Median time from the mouse click until the window shows the target content.",
    raw: "Raw data",
    modern: ["Modern", "A native window drawn by the GPU, in step with every frame of the display.", " Look, accent and theme follow Windows."],
    curves: ["Curves that flow", "The same recorded load drawn two ways: Argus moves the curve at the display's refresh rate, Task Manager jumps once per update."],
    keys: ["Always one key away", "The global shortcut can be changed or turned off. Hidden in the tray, the window stops drawing while sampling, history and rules continue."],
    native: ["Native from end to end", "Process and hardware data come from NT and Win32 interfaces; the window is drawn with Direct3D 11 and composed by DirectComposition."],
    pipe: [["Collect", "NT · Win32"], ["Draw", "Direct3D 11"], ["Compose", "DirectComposition"], ["Display", "Refresh rate"]],
    rich: ["Information-rich", "What Task Manager leaves to other tools, or does not show.", " Only the rows where the two differ."],
    head: ["When you need", "Argus", "Task Manager"],
    changelog: [
      "Always improving",
      releases("en").length + " releases since the first on " + longDate(releases("en").at(-1)?.date ?? "", "en") + ".",
      " Every change is recorded in the changelog.",
    ],
    oss: ["Open source", "Argus is developed in the open on GitHub under AGPL-3.0.", " Code, issues and releases are public; feedback and contributions are welcome."],
    faq: "Questions",
    more: "Read more (Chinese)",
    rotate: "Turn your phone sideways to see the full desktop",
    cta: ["Get Argus", "Free and open source. The installer downloads updates in the background and verifies each signature before installing."],
    install: "Download installer",
  },
  zh: {
    title: "Argus · 快速、现代、信息丰富的 Windows 任务管理器",
    description:
      "Argus 是开源的 Windows 任务管理器：原生 GPU 绘制界面，曲线随刷新率流动，可查端口、文件占用、每进程磁盘读写和最近 7 天的历史。",
    h1: ["快速、现代、信息丰富的", "Windows 任务管理器"],
    pill: ["发布于 ", "查看更新内容"],
    replace: ["可替换系统任务管理器", "即打开 Argus"],
    lead: "Argus 用 Rust 直接读取 Windows 原生接口，界面由 GPU 绘制。进程、性能、服务、启动项、网络连接与文件占用，集中在一个流畅的窗口里。",
    download: "下载 Windows 版",
    other: "免安装版与校验值",
    facts: [r.tag + " · 安装版 " + mebibytes(installer.size) + " MiB", "Windows 10 / 11 · x64", "开源 · AGPL-3.0"],
    intro: ["为排查而生的任务管理器。", "从单击到画面出现只需几十毫秒，曲线随屏幕刷新率流动；端口、文件占用、每进程磁盘读写和最近 7 天的历史，都不必再打开别的工具。"],
    fast: ["快速", "从单击到目标画面出现，Argus 只需要系统任务管理器的一小部分时间。", "从 CPU 性能回到进程页：" + fastest.argus + " 毫秒对 " + fastest.native + " 毫秒。"],
    note: "中位耗时：从鼠标单击开始，到窗口画面出现目标内容为止。",
    raw: "原始数据",
    modern: ["现代", "原生界面由 GPU 绘制，跟上显示器的每一帧。", "外观、强调色与深浅主题跟随 Windows。"],
    curves: ["曲线连续流动", "同一段实测负载的两种画法：Argus 按显示器的刷新率平移曲线，任务管理器每次刷新跳一格。"],
    keys: ["随时呼出，随时隐藏", "全局快捷键可更改或停用。隐藏到托盘后窗口停止绘制，采样、历史记录与规则继续运行。"],
    native: ["从采集到绘制都是原生", "进程与硬件数据直接来自 NT 与 Win32 接口，窗口由 Direct3D 11 绘制、DirectComposition 合成。"],
    pipe: [["采集", "NT · Win32"], ["绘制", "Direct3D 11"], ["合成", "DirectComposition"], ["显示", "随刷新率"]],
    rich: ["信息丰富", "任务管理器需要另开工具或无法查看的信息。", "下表只列出两者不同的地方。"],
    head: ["排查需求", "Argus", "任务管理器"],
    changelog: [
      "持续更新",
      "自 " + longDate(releases("zh").at(-1)?.date ?? "", "zh") + " 发布首个版本以来，已发布 " + releases("zh").length + " 个正式版本。",
      "每个版本的变更均记录在更新日志中。",
    ],
    oss: ["开源", "Argus 以 AGPL-3.0 许可在 GitHub 开源。", "代码、问题与发布记录全部公开，欢迎反馈与贡献。"],
    faq: "常见问题",
    more: "详细说明",
    rotate: "横屏查看完整的桌面预览",
    cta: ["下载 Argus", "免费开源。安装版在后台下载更新，安装前校验更新签名。"],
    install: "下载安装版",
  },
};

export const homeMetadata = (lang: Language) =>
  pageMetadata({ lang, path: "/", title: copy[lang].title, description: copy[lang].description });

// The window comes from the route, so each language loads only its own: the
// English one carries its dictionary (EnglishReplica.tsx).
export function Home({ lang, demo }: { lang: Language; demo: ReactNode }) {
  const en = lang === "en",
    t = copy[lang];
  return (
    <SiteFrame
      lang={lang}
      path="/"
      title={t.title}
      schema={{
        "@context": "https://schema.org",
        "@type": "WebSite",
        "@id": origin + "/#website",
        name: "Argus",
        url: origin + "/",
        inLanguage: ["zh-CN", "en"],
      }}
    >
      <section className="hero" aria-labelledby="hero-title">
        <div className="site-wrap hero-copy">
          <a className="hero-pill" href="#changelog">
            <b className="figures">{r.tag}</b>
            <span>
              {t.pill[0]}
              {longDate(releasedOn(lang), lang)}
            </span>
            <span className="hero-pill-more">
              {t.pill[1]}
              <Icon name="arrow" size={14} />
            </span>
          </a>
          <h1 id="hero-title">
            <span>{t.h1[0]}</span>
            <span>{t.h1[1]}</span>
          </h1>
          <p className="hero-replace">
            <span className="hero-replace-what">
              <Icon name="replace" size={18} />
              {t.replace[0]}
            </span>
            <span className="hero-replace-keys">
              <kbd>Ctrl</kbd>
              <kbd>Shift</kbd>
              <kbd>Esc</kbd>
              {t.replace[1]}
            </span>
          </p>
          <p className="lead">{t.lead}</p>
          <div className="button-row">
            <a className="btn is-primary" href={installer.url}>
              <Icon name="download" size={18} />
              {t.download}
            </a>
            <SiteLink lang={lang} className="btn" href={localized(lang, "/download/")}>
              {t.other}
            </SiteLink>
          </div>
          <ul className="facts">
            {t.facts.map((fact) => (
              <li key={fact}>{fact}</li>
            ))}
          </ul>
        </div>
        {/* A 1920 × 1080 desktop drawn at the width of the page; the window can be
            resized and maximized, and the desktop shown full screen. */}
        <div className="stage" data-motion="">
          <div className="desk">
            <div className="wallpaper" aria-hidden="true">
              <i className="petal p1"></i>
              <i className="petal p2"></i>
              <i className="petal p3"></i>
              <i className="petal p4"></i>
            </div>
            <DeskWindow width={1600} height={900}>
              {demo}
            </DeskWindow>
            <Taskbar lang={lang} />
          </div>
          <p className="rotate-hint">
            <Icon name="rotate" size={18} />
            {t.rotate}
          </p>
          <HeroTour lang={lang} />
        </div>
      </section>

      <section className="site-wrap stats" aria-label={en ? "Project figures" : "项目数据"}>
        <ProjectStats lang={lang} />
      </section>

      <section className="site-wrap intro">
        <p data-rise="">
          {t.intro[0]}
          <span>{t.intro[1]}</span>
        </p>
      </section>

      <section className="site-wrap section" id="fast" aria-labelledby="fast-title">
        <header className="section-head">
          <h2 id="fast-title" data-rise="">
            {t.fast[0]}
          </h2>
          <p data-rise="2">
            {t.fast[1]}
            <span>{t.fast[2]}</span>
          </p>
        </header>
        <div data-rise="3">
          <LatencyBoard rows={latency(lang)} lang={lang} />
        </div>
        <p className="note">
          {t.note}
          <a className="text-link" href="/benchmarks/windows-interaction.json">
            {t.raw}
          </a>
        </p>
      </section>

      <section className="site-wrap section" id="modern" aria-labelledby="modern-title">
        <header className="section-head">
          <h2 id="modern-title" data-rise="">
            {t.modern[0]}
          </h2>
          <p data-rise="2">
            {t.modern[1]}
            <span>{t.modern[2]}</span>
          </p>
        </header>
        <div className={en ? "feature-grid is-short" : "feature-grid"}>
          <article className="cell cell-curves" data-spotlight="" data-rise="">
            <h3>{t.curves[0]}</h3>
            <p>{t.curves[1]}</p>
            <CurveCompare lang={lang} />
          </article>
          {!en && (
            <article className="cell cell-accent" data-spotlight="" data-rise="2">
              <h3>跟随系统强调色</h3>
              <p>Windows 11 上使用云母材质。选择一种强调色，上方窗口随之改变。</p>
              <AccentPicker />
            </article>
          )}
          <article className="cell cell-keys" data-spotlight="" data-rise="2" data-motion="">
            <div className="keys" aria-hidden="true">
              <kbd>
                <span>Ctrl</span>
              </kbd>
              <kbd>
                <span>Alt</span>
              </kbd>
              <kbd>
                <span>Esc</span>
              </kbd>
              <span className="mini-window">
                <img src="/assets/argus.svg" width="18" height="18" alt="" />
              </span>
            </div>
            <h3>{t.keys[0]}</h3>
            <p>{t.keys[1]}</p>
          </article>
          <article className="cell cell-native" data-spotlight="" data-rise="3" data-motion="">
            <h3>{t.native[0]}</h3>
            <p>{t.native[1]}</p>
            <ol className="pipeline" aria-hidden="true">
              {t.pipe.map(([name, detail], i) => (
                <li key={name} style={{ "--i": i } as CSSProperties}>
                  {i > 0 && (
                    <i className="pipe-line">
                      <i />
                    </i>
                  )}
                  <span className="pipe-node">
                    <b>{name}</b>
                    <small>{detail}</small>
                  </span>
                </li>
              ))}
            </ol>
          </article>
        </div>
      </section>

      <section className="site-wrap section" id="rich" aria-labelledby="rich-title">
        <header className="section-head">
          <h2 id="rich-title" data-rise="">
            {t.rich[0]}
          </h2>
          <p data-rise="2">
            {t.rich[1]}
            <span>{t.rich[2]}</span>
          </p>
        </header>
        <div className={en ? "rich is-full" : "rich"}>
          <table className="diff-table" data-rise="">
            <caption className="sr-only">{t.rich[0]}</caption>
            <thead>
              <tr>
                {t.head.map((h) => (
                  <th scope="col" key={h}>
                    {h}
                  </th>
                ))}
              </tr>
            </thead>
            <tbody>
              {differences(lang).map(([need, argus, native, page], i) => (
                <tr key={need} style={{ "--i": i } as CSSProperties}>
                  <th scope="row">{need}</th>
                  <td className="yes">
                    {argus}
                    {page && <ShowPage page={page}>{en ? "See it in the demo" : "在演示中查看"}</ShowPage>}
                  </td>
                  <td className="no">{native}</td>
                </tr>
              ))}
            </tbody>
          </table>
          {!en && (
            <aside className="cell drop" data-spotlight="" data-motion="">
              <h3>把文件拖到这里</h3>
              <p>只读取文件名，在浏览器内演示查询结果，不会上传。</p>
              <FileDrop />
            </aside>
          )}
        </div>
      </section>

      <section className="site-wrap section" id="changelog" aria-labelledby="changelog-title">
        <header className="section-head">
          <h2 id="changelog-title" data-rise="">
            {t.changelog[0]}
          </h2>
          <p data-rise="2">
            {t.changelog[1]}
            <span>{t.changelog[2]}</span>
          </p>
        </header>
        <Releases notes={site.changelog[lang]} lang={lang} />
      </section>

      <section className="site-wrap section" id="open-source" aria-labelledby="oss-title">
        <header className="section-head">
          <h2 id="oss-title" data-rise="">
            {t.oss[0]}
          </h2>
          <p data-rise="2">
            {t.oss[1]}
            <span>{t.oss[2]}</span>
          </p>
        </header>
        <OpenSource lang={lang} guide={docs("docs/development/README.md")} />
      </section>

      <section className="site-wrap section faq" id="faq" aria-labelledby="faq-title">
        <header className="section-head">
          <h2 id="faq-title" data-rise="">
            {t.faq}
          </h2>
        </header>
        <div className="faq-list" data-rise="2">
          {questions(lang).map(([id, question, answer, link], i) => (
            <details id={id} key={id} name="faq" open={i === 0}>
              <summary>
                {question}
                <Icon name="plus" size={18} />
              </summary>
              <div className="faq-answer">
                <p>{answer}</p>
                <SiteLink lang={lang} className="text-link" href={link}>
                  {t.more} <Icon name="arrow" size={14} />
                </SiteLink>
              </div>
            </details>
          ))}
        </div>
      </section>

      <section className="site-wrap cta" aria-labelledby="cta-title">
        <div className="cta-inner" data-spotlight="" data-rise="" data-motion="">
          <div className="cta-mark" aria-hidden="true">
            <img src="/assets/argus.svg" width="64" height="64" alt="" />
          </div>
          <h2 id="cta-title">{t.cta[0]}</h2>
          <p>{t.cta[1]}</p>
          <div className="button-row">
            <a className="btn is-primary" href={installer.url}>
              <Icon name="download" size={18} />
              {t.install}
            </a>
            <SiteLink lang={lang} className="btn" href={localized(lang, "/download/")}>
              {t.other}
            </SiteLink>
          </div>
          <p className="fine">
            {r.tag} · {site.translations[lang].operatingSystem} ·
            <SiteLink lang={lang} href={stable.base}>
              {en ? "Documentation (Chinese)" : "使用文档"}
            </SiteLink>
          </p>
        </div>
      </section>
    </SiteFrame>
  );
}
