import { escape, sidebar } from "./documents.mjs";
import { origin, repo } from "./release.mjs";

const paths = {
  down: "M12 3v12m-5-5 5 5 5-5M4 16v4h16v-4",
  arrow: "M5 12h14m-5-5 5 5-5 5",
  external: "M7 17 17 7M7 7h10v10",
  layers: "m3 7 9-5 9 5-9 5-9-5Zm0 5 9 5 9-5m-18 5 9 5 9-5",
  pulse: "M2 13h5l3-8 4 15 3-9 2 2h3",
  clock: "M21 12a9 9 0 1 1-9-9 9 9 0 0 1 9 9Zm-9-5v5l3 2",
  box: "m3 8 9-5 9 5v9l-9 5-9-5V8Zm0 0 9 5 9-5m-9 5v9",
  portable: "M7 4h10v16H7zM10 7h4m-3 10h2",
  search: "M17 17l4 4m-3-11a7 7 0 1 1-14 0 7 7 0 0 1 14 0Z",
};
const icon = (name) =>
  /* HTML */ `<svg
    viewBox="0 0 24 24"
    fill="none"
    stroke="currentColor"
    stroke-width="1.6"
    stroke-linecap="round"
    stroke-linejoin="round"
    aria-hidden="true"
  >
    <path d="${paths[name]}" />
  </svg>`;
const labels = {
  zh: {
    home: "概览",
    docs: "文档",
    download: "下载",
    skip: "跳到正文",
    privacy: "隐私政策",
    security: "报告安全问题",
    contact: "联系维护者",
    source: "源代码",
    requirements: "Windows 10 22H2 或更高版本 · x64",
    installer: "下载安装版",
    portable: "免安装版",
    release: "版本记录",
    guide: "使用指南",
  },
  en: {
    home: "Overview",
    docs: "Docs",
    download: "Download",
    skip: "Skip to content",
    privacy: "Privacy",
    security: "Report a vulnerability",
    contact: "Contact",
    source: "Source code",
    requirements: "Windows 10 22H2 or later · x64",
    installer: "Download installer",
    portable: "Portable edition",
    release: "Release notes",
    guide: "User guide",
  },
};
const brand =
  '<a class="brand" href="/"><img src="/assets/argus.svg" alt="" width="31" height="31"><span>Argus</span></a>';
const prefix = (lang) => (lang === "en" ? "/en" : "");
const active = (yes) => (yes ? ' aria-current="page"' : "");
export function layout({
  lang = "zh",
  title,
  path,
  kind = "",
  description,
  other,
  body,
  contact = false,
  siteImage = "/assets/preview-zh-dark.png",
}) {
  const t = labels[lang],
    p = prefix(lang),
    alternate = other ?? (lang === "zh" ? "/en/" : "/");
  const canonical = origin + path;
  return /* HTML */ `<!doctype html>
    <html lang="${lang === "zh" ? "zh-CN" : "en"}">
      <head>
        <meta charset="utf-8" />
        <meta name="viewport" content="width=device-width,initial-scale=1" />
        <meta name="color-scheme" content="dark light" />
        <title>${escape(title)} · Argus</title>
        <meta name="description" content="${escape(description ?? title)}" />
        <link rel="canonical" href="${canonical}" />
        ${other
          ? `<link rel="alternate" hreflang="${lang === "zh" ? "en" : "zh-CN"}" href="${origin + other}">`
          : ""}
        <meta property="og:title" content="${escape(title)} · Argus" />
        <meta property="og:type" content="website" />
        <meta property="og:image" content="${origin + siteImage}" />
        <link rel="icon" href="/assets/argus.svg" type="image/svg+xml" />
        <link rel="stylesheet" href="/assets/site.css" />
      </head>
      <body>
        <a class="skip" href="#main">${t.skip}</a>
        <header class="mast">
          <div class="wrap mast-inner">
            ${brand.replace('href="/"', `href="${p}/"`)}
            <nav aria-label="${lang === "zh" ? "主导航" : "Main navigation"}">
              <a href="${p}/" ${active(kind === "home")}>${t.home}</a
              ><a href="/docs/" ${active(kind === "docs")}>${t.docs}</a
              ><a href="${p}/download/" ${active(kind === "download")}
                >${t.download}</a
              >
            </nav>
            <div class="mast-end">
              <a
                class="lang"
                href="${alternate}"
                lang="${lang === "zh" ? "en" : "zh-CN"}"
                >${lang === "zh" ? "EN" : "中文"}</a
              ><a class="git" href="${repo}">GitHub ${icon("external")}</a>
            </div>
          </div>
        </header>
        <main id="main">${body}</main>
        <footer>
          <div class="wrap footer-inner">
            <div>
              ${brand.replace('href="/"', `href="${p}/"`)}
              <p>
                © Argus contributors ·
                <a href="${repo}/blob/main/LICENSE">AGPL-3.0</a>
              </p>
            </div>
            <div class="foot-links">
              <a href="${p}/privacy/">${t.privacy}</a
              ><a href="${p}/signing/">Code signing policy</a
              ><a href="${p}/security/">${t.security}</a
              ><a
                href="${contact
                  ? "mailto:contact@argustask.com"
                  : repo + "/issues"}"
                >${t.contact}</a
              >
            </div>
          </div>
        </footer>
      </body>
    </html>`;
}

function card(symbol, title, text, href, link) {
  return /* HTML */ `<article class="feature">
    <i>${icon(symbol)}</i>
    <h3>${title}</h3>
    <p>${text}</p>
    <a href="${href}">${link} ${icon("arrow")}</a>
  </article>`;
}
export function home(lang, r, contact) {
  const en = lang === "en",
    t = labels[lang],
    p = prefix(lang);
  const intro = en
    ? "Find the process using a resource, inspect hardware activity, and look back over the last seven days."
    : "按应用查看进程，跟踪 CPU、内存和 GPU 的实时负载，回看过去七天的资源占用。";
  const shot = `/assets/preview-${lang}-dark.png`;
  return layout({
    lang,
    title: en ? "Windows Task Manager" : "Windows 任务管理器",
    description: intro,
    path: `${p}/`,
    other: en ? "/" : "/en/",
    kind: "home",
    contact,
    siteImage: shot,
    body: /* HTML */ `<section class="hero">
        <div class="hero-grid wrap">
          <div class="hero-copy">
            <div class="eyebrow"><b></b> ARGUS TASK MANAGER</div>
            <h1>
              Windows<br /><span>${en ? "Task Manager" : "任务管理器"}</span>
            </h1>
            <p class="lead">${intro}</p>
            <div class="actions">
              <a class="button primary" href="${r.files.installer.url}"
                >${icon("down")}${t.installer}</a
              ><a class="button outline" href="${p}/download/"
                >${en ? "All downloads" : "全部下载"} ${icon("arrow")}</a
              >
            </div>
            <p class="meta"><span>v${r.version}</span>${t.requirements}</p>
          </div>
          <figure class="product-shot">
            <div class="shot-caption">
              <span><b></b> ${en ? "PROCESS VIEW" : "进程视图"}</span
              ><span>v${r.version} / Windows</span>
            </div>
            <a
              href="${shot}"
              aria-label="${en
                ? "Open full-size screenshot"
                : "查看原始尺寸截图"}"
              ><picture
                ><source
                  media="(prefers-color-scheme:light)"
                  srcset="/assets/preview-${lang}-light.png" />
                <img
                  src="${shot}"
                  alt="${en
                    ? "Argus process list with CPU, memory, disk, network and GPU graphs"
                    : "Argus 进程列表和 CPU、内存、磁盘、网络、GPU 概况图"}"
                  width="1240"
                  height="800"
                  fetchpriority="high" /></picture
            ></a>
          </figure>
        </div>
      </section>
      <section class="wrap feature-section">
        <div class="section-intro">
          <div class="eyebrow">${en ? "WORKSPACE" : "工作区"}</div>
          <h2>${en ? "Inside Argus" : "进程、设备与历史"}</h2>
        </div>
        <div class="features">
          ${card(
            "layers",
            en ? "Processes" : "进程管理",
            en
              ? "Group related processes, inspect their details, and find the program holding a file."
              : "按应用或进程树浏览，查看资源占用和进程详情，定位占用文件的程序。",
            "/docs/guide/",
            t.guide,
          )}${card(
            "pulse",
            en ? "Hardware" : "硬件监控",
            en
              ? "Inspect per-core CPU activity, memory, disks, network adapters and GPU engines."
              : "查看逻辑处理器、内存、物理磁盘、网卡与 GPU 引擎的性能读数。",
            "/docs/guide/hardware-data/",
            en ? "Hardware reference" : "硬件数据说明",
          )}${card(
            "clock",
            en ? "History" : "历史回溯",
            en
              ? "Select a point on the resource timeline to see which programs were busiest."
              : "选中资源曲线上的一个时刻，查看当时占用最高的进程。",
            "/docs/guide/#历史",
            en ? "History guide" : "历史使用指南",
          )}
        </div>
      </section>
      <div class="wrap project-line">
        <div>
          <strong>${en ? "Open source" : "开源项目"}</strong
          ><span>AGPL-3.0-or-later</span>
        </div>
        <a href="${repo}"
          >${en ? "Browse the repository" : "浏览 GitHub 仓库"}
          ${icon("external")}</a
        >
      </div>`,
  });
}

export function downloads(lang, r, contact, requirements) {
  const en = lang === "en",
    p = prefix(lang),
    t = labels[lang];
  const files = Object.entries(r.files)
    .map(([kind, file]) => {
      const setup = kind === "installer";
      return /* HTML */ `<article class="package">
        <div class="package-icon">${icon(setup ? "box" : "portable")}</div>
        <div class="eyebrow">WINDOWS / X64</div>
        <h2>${setup ? (en ? "Installer" : "安装版") : t.portable}</h2>
        <p>
          ${setup
            ? en
              ? "Install Argus with Start menu shortcuts and in-app update installation."
              : "安装到本机，创建开始菜单快捷方式，并在应用内安装更新。"
            : en
              ? "Download a single executable and run it directly."
              : "下载单个可执行文件后直接运行。"}
        </p>
        <a class="button ${setup ? "primary" : "outline"}" href="${file.url}"
          >${icon("down")}${setup
            ? t.installer
            : en
              ? "Download portable"
              : "下载免安装版"}</a
        ><a class="origin-link" href="${file.upstream}"
          >GitHub ${en ? "download" : "下载"} ${icon("external")}</a
        >
        <div class="file-meta">
          <code>${file.name}</code
          ><span>${(file.size / 1048576).toFixed(2)} MiB</span>
        </div>
        <details>
          <summary>SHA-256</summary>
          <code class="digest">${file.sha256}</code>
        </details>
      </article>`;
    })
    .join("");
  return layout({
    lang,
    path: `${p}/download/`,
    other: en ? "/download/" : "/en/download/",
    title: t.download,
    kind: "download",
    contact,
    body: /* HTML */ `<section class="wrap subpage">
      <div class="eyebrow">
        <b></b> ${en ? "STABLE RELEASE" : "稳定版"} / v${r.version}
      </div>
      <h1>${en ? "Download Argus" : "下载 Argus"}</h1>
      <p class="lead">${t.requirements} · ${r.published.slice(0, 10)}</p>
      <div class="packages">${files}</div>
      <div class="resource-links">
        <a href="https://dl.argustask.com/${r.tag}/SHA256SUMS.txt"
          >SHA256SUMS.txt</a
        ><a href="${repo}/releases/tag/${r.tag}">${t.release}</a
        ><a href="/docs/guide/hardware-data/#系统兼容范围"
          >${en ? "System requirements (中文)" : "系统兼容范围"}</a
        ><a href="${p}/signing/">Code signing policy</a>
      </div>
      <div class="prose requirements">${requirements}</div>
    </section>`,
  });
}

export function docPage(page, pages, r, contact) {
  const menu = sidebar(pages, page.path),
    toc = page.headings.filter((h) => h.level === 2);
  return layout({
    title: page.title,
    path: page.path,
    kind: "docs",
    contact,
    description: page.text.slice(0, 125),
    body: /* HTML */ `<div class="wrap docs-shell">
      <aside class="doc-nav">
        <div class="eyebrow">DOCUMENTATION</div>
        <strong>Argus 文档</strong
        ><span class="version-pill">稳定版 v${r.version}</span
        ><a class="search-link" href="/search/">${icon("search")} 搜索文档</a
        >${menu}<a class="dev-ref" href="${repo}/tree/main/docs"
          >开发分支文档 ↗</a
        >
      </aside>
      <div class="doc-center">
        <details class="mobile-nav">
          <summary>文档目录 · v${r.version}</summary>
          ${menu}<a href="/search/">搜索文档</a>
        </details>
        <div class="breadcrumbs">
          <a href="/docs/">文档</a><span>/</span><span>v${r.version}</span>
        </div>
        <article class="prose">${page.html}</article>
        <a class="source-link" href="${repo}/blob/${r.tag}/${page.file}"
          >在 GitHub 查看此页 ${icon("external")}</a
        >
      </div>
      <aside class="toc">
        <strong>本页内容</strong>${toc
          .map((h) => `<a href="#${escape(h.id)}">${escape(h.text)}</a>`)
          .join("")}
      </aside>
    </div>`,
  });
}

export function article(lang, route, title, html, contact) {
  const p = prefix(lang);
  return layout({
    lang,
    title,
    path: `${p}/${route}/`,
    other: `${lang === "en" ? "" : "/en"}/${route}/`,
    contact,
    body: /* HTML */ `<section class="wrap paper">
      <div class="eyebrow">ARGUS / PROJECT</div>
      <article class="prose">${html}</article>
    </section>`,
  });
}
export function searchPage(contact) {
  return layout({
    path: "/search/",
    title: "搜索文档",
    kind: "docs",
    contact,
    body: /* HTML */ `<section class="wrap paper">
      <div class="eyebrow">DOCUMENTATION</div>
      <h1>搜索文档</h1>
      <form class="search-form" action="/search/">
        <label for="query">关键词</label
        ><input
          id="query"
          name="q"
          type="search"
          placeholder="例如：传感器、网络、快捷键"
        /><button class="button primary">搜索</button>
      </form>
      <p id="status" role="status"></p>
      <ul id="results" class="search-results"></ul>
      <noscript><a href="/docs/">浏览完整文档目录</a></noscript>
      <script src="/assets/search.js" defer></script>
    </section>`,
  });
}
