"use client";
import type { MouseEvent, ReactNode } from "react";
import type { Language } from "@/lib/site";

/*
 * Switching language keeps the reader where they were: the section on screen
 * and how far into it are kept for the other language's page, which scrolls
 * there before it is first drawn (the script in the root layout). The two
 * pages then cross over in a view transition of their own.
 */
export const placeKey = "argus-place";
function remember(event: MouseEvent<HTMLAnchorElement>) {
  if (event.defaultPrevented || event.button || event.metaKey || event.ctrlKey || event.shiftKey || event.altKey) return;
  const padding = parseFloat(getComputedStyle(document.documentElement).scrollPaddingTop) || 0;
  let section: HTMLElement | null = null;
  for (const el of document.querySelectorAll<HTMLElement>("main section[id], main article[id]"))
    if (el.getBoundingClientRect().top <= padding + 1) section = el;
  const within = section ? (padding - section.getBoundingClientRect().top) / Math.max(1, section.offsetHeight) : 0;
  const end = document.documentElement.scrollHeight - innerHeight;
  try {
    sessionStorage.setItem(
      placeKey,
      JSON.stringify({
        to: new URL(event.currentTarget.href).pathname,
        id: scrollY > 0 ? (section?.id ?? null) : null,
        within,
        ratio: end > 0 ? scrollY / end : 0,
        at: Date.now(),
      }),
    );
  } catch {
    // Without storage the other page opens at its top.
  }
}

// The two languages side by side; a thumb rests under the one shown and, in
// browsers with view transitions between pages, glides to the other as the
// page changes.
export function LanguageSwitch({ lang, href, label }: { lang: Language; href: string; label: string }) {
  return (
    <a className="tool lang-switch" href={href} hrefLang={lang === "en" ? "zh-CN" : "en"} aria-label={label} title={label} data-lang={lang} onClick={remember}>
      <span className="lang-thumb" aria-hidden="true" />
      <span lang="zh-CN">中</span>
      <span lang="en">EN</span>
    </a>
  );
}

// The same for the narrow-screen menu's link.
export function LanguageLink({ href, lang, children }: { href: string; lang: string; children: ReactNode }) {
  return (
    <a href={href} lang={lang} hrefLang={lang} onClick={remember}>
      {children}
    </a>
  );
}
