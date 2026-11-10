"use client";
import type { Language } from "@/lib/languages";
import { translate, type Messages } from "@/lib/translate";
import { languageInfo } from "@/lib/languages";
import { chineseDocsLabels as zh } from "@/lib/document-labels";
import { LocaleProvider } from "./site/LocaleProvider";
import { RootProvider } from "fumadocs-ui/provider/next";
import { useEffect, type ReactNode } from "react";
import SearchDialog from "./search";
import { DocsLink } from "./site/IntentLink";
import { StableTitle } from "./stable-title";

// Fumadocs' interface strings, keyed as it looks them up: the English text
// followed by notes naming the component (fumadocs-ui/dist/.translations/keys.js).


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

export function Provider({ lang, messages, children }: { lang: Language; messages: Messages; children: ReactNode }) {
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
      i18n={lang === "zh" ? { locale: "zh", translations: zh } : { locale: languageInfo(lang).tag, translations: Object.fromEntries(Object.keys(zh).map(key => [key, translate(messages, key.split("(")[0])])) }}
    >
      <LocaleProvider lang={lang} messages={messages}>
        <StableTitle />
        <QuietThemeSwitch />
        {children}
      </LocaleProvider>
    </RootProvider>
  );
}
