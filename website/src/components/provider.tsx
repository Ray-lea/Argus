"use client";
import { RootProvider } from "fumadocs-ui/provider/next";
import { useEffect, type ReactNode } from "react";
import SearchDialog from "./search";
import { DocsLink } from "./site/IntentLink";
import { StableTitle } from "./stable-title";

// Fumadocs' interface strings, keyed as it looks them up: the English text
// followed by notes naming the component (fumadocs-ui/dist/.translations/keys.js).
const zh: Record<string, string> = {
  "Back to Home(404 not found page)": "返回首页",
  "Close Search(search dialog)(aria-label)": "关闭搜索",
  "Close Sidebar(aria-label)": "关闭侧栏",
  "Close Sidebar(sidebar)(aria-label)": "关闭侧栏",
  "Close(image zoom)(aria-label)": "关闭",
  "Collapse Sidebar(sidebar)(aria-label)": "收起侧栏",
  "Copied Anchor Link(heading anchor)(aria-label)": "已复制标题链接",
  "Copied Markdown(page actions)": "已复制 Markdown",
  "Copied Text(code block)(aria-label)": "已复制",
  "Copy Anchor Link(heading anchor)(aria-label)": "复制标题链接",
  "Copy Markdown(page actions)": "复制 Markdown",
  "Copy Text(code block)(aria-label)": "复制代码",
  "Dark(theme switcher)(aria-label)": "深色",
  "Edit on GitHub(edit page)": "在 GitHub 上编辑",
  "Hide Sidebar(sidebar)": "隐藏侧栏",
  "Last updated on(page footer)": "最后更新于",
  "Light(theme switcher)(aria-label)": "浅色",
  "Next Page(pagination)": "下一页",
  "No Headings(table of contents)": "暂无标题",
  "No results found(search dialog)": "没有匹配的内容",
  "On this page(table of contents)": "本页内容",
  "Open Search(search trigger)(aria-label)": "搜索文档",
  "Open Sidebar(aria-label)": "打开侧栏",
  "Open Sidebar(sidebar)(aria-label)": "打开侧栏",
  "Open in GitHub(page actions)": "在 GitHub 上查看",
  "Open(page actions)": "打开",
  "Options(aria-label)": "选项",
  "Page Not Found(404 not found page)": "页面未找到",
  "Previous Page(pagination)": "上一页",
  "Search failed(search dialog)": "搜索失败",
  "Search(search dialog)": "搜索文档",
  "Search(search trigger)": "搜索文档",
  "Show Sidebar(sidebar)": "显示侧栏",
  "System(theme switcher)(aria-label)": "跟随系统",
  "Table of Contents(inline table of contents)": "目录",
  "The page you are looking for might have been removed, had its name changed, or is temporarily unavailable.(404 not found page)":
    "页面可能已移动或改名。",
  "Toggle Menu(home layout header)(aria-label)": "菜单",
  "Toggle Theme(theme switcher)(aria-label)": "切换主题",
  "View as Markdown(page actions)": "查看 Markdown",
};

// While the page takes another theme, nothing transitions: colours change at
// once, as they did with next-themes' disableTransitionOnChange.
function QuietThemeSwitch() {
  useEffect(() => {
    const root = document.documentElement;
    let last = root.getAttribute("data-theme");
    const observer = new MutationObserver(() => {
      const now = root.getAttribute("data-theme");
      if (now === last) return;
      last = now;
      const style = document.createElement("style");
      style.textContent = "*,*::before,*::after{transition:none!important}";
      document.head.append(style);
      requestAnimationFrame(() => requestAnimationFrame(() => style.remove()));
    });
    observer.observe(root, { attributes: true, attributeFilter: ["data-theme"] });
    return () => observer.disconnect();
  }, []);
  return null;
}

export function Provider({ lang, children }: { lang: "zh" | "en"; children: ReactNode }) {
  return (
    <RootProvider
      // The site's own key and values: "light", "dark" or "system" (stored as absent).
      theme={{
        attribute: ["class", "data-theme"],
        storageKey: "argus-theme",
        defaultTheme: "system",
        enableSystem: true,
        // next-themes would also re-apply the stored theme at every page load
        // behind a stylesheet that forces two full style recalculations;
        // QuietThemeSwitch does the same only when the theme really changes.
        disableTransitionOnChange: false,
        hotKey: false,
      }}
      search={{ SearchDialog }}
      components={{ Link: DocsLink }}
      i18n={lang === "zh" ? { locale: "zh", translations: zh } : { locale: "en" }}
    >
      <StableTitle />
      <QuietThemeSwitch />
      {children}
    </RootProvider>
  );
}
