"use client";
import { useEffect, useId, useRef, useState, type MouseEvent, type CSSProperties } from "react";
import { languages, languageInfo, localized, type Language } from "@/lib/languages";
import { Icon } from "./Icon";

/*
 * Switching language keeps the reader where they were: the section on screen
 * and how far into it are kept for the other language's page, which scrolls
 * there before it is first drawn (the script in the root layout). The two
 * pages then cross over in a view transition of their own.
 */
export const placeKey = "argus-place";
function remember(event: MouseEvent<HTMLAnchorElement>) {
  if (event.defaultPrevented || event.button || event.metaKey || event.ctrlKey || event.shiftKey || event.altKey) return false;
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
        hash: new URL(event.currentTarget.href).hash,
        id: scrollY > 0 ? (section?.id ?? null) : null,
        within,
        ratio: end > 0 ? scrollY / end : 0,
        at: Date.now(),
      }),
    );
    return true;
  } catch {
    // Without storage the other page opens at its top.
    return false;
  }
}

// One disclosure at every viewport, with native names and keyboard navigation.
export function LanguageSwitch({ lang, path, label }: { lang: Language; path: string; label: string }) {
  const [open, setOpen] = useState(false);
  const [leaving, setLeaving] = useState(false);
  const root = useRef<HTMLDivElement>(null);
  const trigger = useRef<HTMLButtonElement>(null);
  const id = useId();
  const info = languageInfo(lang);
  const close = (restore = false) => {
    setOpen(false);
    setLeaving(!matchMedia("(prefers-reduced-motion: reduce)").matches);
    if (restore) trigger.current?.focus({ preventScroll: true });
  };
  useEffect(() => {
    if (!leaving) return;
    const timer = setTimeout(() => setLeaving(false), 150);
    return () => clearTimeout(timer);
  }, [leaving]);
  useEffect(() => {
    if (!open) return;
    root.current?.querySelector<HTMLElement>('[aria-checked="true"]')?.focus({ preventScroll: true });
    const outside = (event: PointerEvent) => {
      if (event.target instanceof Node && !root.current?.contains(event.target)) close();
    };
    document.addEventListener("pointerdown", outside);
    return () => document.removeEventListener("pointerdown", outside);
  }, [open]);
  return (
    <div className="language-picker" ref={root}
      onBlurCapture={event => { if (open && !event.currentTarget.contains(event.relatedTarget)) close(); }}
      onKeyDown={event => {
        if (!open) {
          if (["ArrowDown", "ArrowUp"].includes(event.key)) { event.preventDefault(); setLeaving(false); setOpen(true); }
          return;
        }
        const items = [...(root.current?.querySelectorAll<HTMLAnchorElement>('[role="menuitemradio"]') ?? [])];
        const at = items.indexOf(document.activeElement as HTMLAnchorElement);
        const next = event.key === "ArrowDown" ? (at + 1) % items.length : event.key === "ArrowUp" ? (at + items.length - 1) % items.length : event.key === "Home" ? 0 : event.key === "End" ? items.length - 1 : -1;
        if (event.key === "Escape") { event.preventDefault(); event.stopPropagation(); close(true); }
        else if (event.key === " " && event.target instanceof HTMLAnchorElement) { event.preventDefault(); event.target.click(); }
        else if (next >= 0) {
          event.preventDefault();
          const item = items[next];
          item?.focus({ preventScroll: true });
          const panel = item?.parentElement;
          if (panel && item) {
            if (item.offsetTop < panel.scrollTop) panel.scrollTop = item.offsetTop;
            else if (item.offsetTop + item.offsetHeight > panel.scrollTop + panel.clientHeight)
              panel.scrollTop = item.offsetTop + item.offsetHeight - panel.clientHeight;
          }
        }
      }}>
      <button ref={trigger} type="button" className="tool lang-trigger" aria-label={label + " · " + info.name}
        title={label + " · " + info.name} aria-haspopup="menu" aria-controls={id} aria-expanded={open}
        onClick={() => { if (open) close(); else { setLeaving(false); setOpen(true); } }}>
        <Icon name="language" size={16} />
        <span className="lang-current" lang={info.tag}>{info.name}</span>
        <span className="lang-chevron" aria-hidden="true"><Icon name="chevron" size={14} /></span>
      </button>
      {(open || leaving) && <div id={id} role="menu" aria-label={label} inert={!open}
        className={"language-menu" + (open ? " is-open" : " is-closing")}>
        {languages.map((item, index) => <a key={item.id} className="language-option" role="menuitemradio"
          aria-checked={item.id === lang} lang={item.tag} hrefLang={item.tag} href={localized(item.id, path)}
          data-language={item.id} tabIndex={open && item.id === lang ? 0 : -1} style={{ "--i": index } as CSSProperties}
          onClick={event => {
            if (item.id === lang) { event.preventDefault(); close(true); return; }
            event.currentTarget.href = localized(item.id, path) + location.search + location.hash;
            // Add the hash after restoring the reading position, so the browser's
            // fragment navigation cannot replace it with the section's start.
            if (remember(event)) event.currentTarget.hash = "";
          }}>
          <span>{item.name}</span>
          <span className="language-check">{item.id === lang && <Icon name="check" size={15} />}</span>
        </a>)}
      </div>}
    </div>
  );
}
