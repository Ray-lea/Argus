"use client";
import { useEffect, useId, useRef, useState, type MouseEvent, type CSSProperties } from "react";
import { choiceKeys, languages, languageInfo, localized, offeredLanguage, offers, rememberLanguage, type Language } from "@/lib/languages";
import { Icon } from "./Icon";

/*
 * Switching language keeps the reader where they were: the section on screen
 * and how far into it are kept for the other language's page, which scrolls
 * there before it is first drawn (the script in the root layout). The two
 * pages then cross over in a view transition of their own.
 */
export const placeKey = "argus-place";
// A plain click turns this page into the other; with a modifier the other opens beside it.
const plain = (event: MouseEvent) =>
  !(event.defaultPrevented || event.button || event.metaKey || event.ctrlKey || event.shiftKey || event.altKey);
function remember(event: MouseEvent<HTMLAnchorElement>) {
  if (!plain(event)) return false;
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

// What a typed letter is matched against: a language's own name, and its
// code for the names a Latin keyboard cannot start (zh, ja, ru).
const typeable = languages.map((item) => [item.name, item.id, item.tag].map((text) => text.toLocaleLowerCase()));

// The offer of another language is made once while the site stays loaded:
// moving on to its other pages does not bring it back.
let offered = false;

// One disclosure at every viewport, with native names and keyboard navigation.
// One row is lit at a time: the keyboard's, or the one under the pointer,
// which takes the keyboard's place. A chosen language shows in the button at
// once, while its page loads.
export function LanguageSwitch({ lang, path, label }: { lang: Language; path: string; label: string }) {
  const [open, setOpen] = useState(false);
  const [leaving, setLeaving] = useState(false);
  const [going, setGoing] = useState<Language | null>(null);
  const root = useRef<HTMLDivElement>(null);
  const trigger = useRef<HTMLButtonElement>(null);
  const typed = useRef({ text: "", at: 0 });
  const id = useId();
  const info = languageInfo(going ?? lang);
  // The page in the visitor's own language, offered under the header.
  const [offer, setOffer] = useState<{ lang: Language; open: boolean } | null>(null);
  const withdraw = () => setOffer((current) => current && { ...current, open: false });
  // The menu stays in the page while it fades (a chosen link must outlive its
  // click); without motion it is simply hidden for that moment.
  const close = (restore = false, keep = false) => {
    setOpen(false);
    setLeaving(keep || !matchMedia("(prefers-reduced-motion: reduce)").matches);
    if (restore) trigger.current?.focus({ preventScroll: true });
  };
  useEffect(() => {
    if (!leaving) return;
    const timer = setTimeout(() => setLeaving(false), 150);
    return () => clearTimeout(timer);
  }, [leaving]);
  useEffect(() => {
    if (offered) return;
    const wanted = (() => {
      try {
        return offeredLanguage(lang, localStorage.getItem(choiceKeys.chosen), localStorage.getItem(choiceKeys.dismissed), navigator.languages ?? []);
      } catch {
        // Without storage a closed offer would come back on every page.
        return null;
      }
    })();
    if (!wanted) return;
    // Once the page has arrived, so that the offer does not arrive with it.
    const timer = setTimeout(() => {
      offered = true;
      setOffer({ lang: wanted, open: true });
    }, 900);
    return () => clearTimeout(timer);
  }, [lang]);
  useEffect(() => {
    if (offer?.open !== false) return;
    const timer = setTimeout(() => setOffer(null), 150);
    return () => clearTimeout(timer);
  }, [offer]);
  // A language chosen is kept for the home page's address (layout.tsx); its
  // page takes along the query, the hash and the reading position.
  const go = (event: MouseEvent<HTMLAnchorElement>, to: Language) => {
    if (plain(event)) rememberLanguage(to);
    event.currentTarget.href = localized(to, path) + location.search + location.hash;
    // Add the hash after restoring the reading position, so the browser's
    // fragment navigation cannot replace it with the section's start.
    if (!remember(event)) return false;
    event.currentTarget.hash = "";
    // This page turns into the other one: the button says so at once.
    setGoing(to);
    return true;
  };
  const dismiss = (restore: boolean) => {
    if (!offer) return;
    try {
      localStorage.setItem(choiceKeys.dismissed, offer.lang);
    } catch {
      // It is not offered again while the site stays loaded.
    }
    withdraw();
    if (restore) trigger.current?.focus({ preventScroll: true });
  };
  // The other page did not come (offline), or this one came back from the
  // history cache: the button names the page's own language again.
  useEffect(() => {
    if (!going) return;
    const reset = () => setGoing(null);
    const timer = setTimeout(reset, 8000);
    addEventListener("pageshow", reset);
    return () => {
      clearTimeout(timer);
      removeEventListener("pageshow", reset);
    };
  }, [going]);
  // The keyboard's row wears a ring; the pointer's does not, whichever came last.
  const keyed = (on: boolean) => root.current?.toggleAttribute("data-keys", on);
  const light = (item: HTMLElement | undefined) => {
    if (!item) return;
    keyed(true);
    item.focus({ preventScroll: true });
    const panel = item.parentElement;
    if (!panel) return;
    if (item.offsetTop < panel.scrollTop) panel.scrollTop = item.offsetTop;
    else if (item.offsetTop + item.offsetHeight > panel.scrollTop + panel.clientHeight)
      panel.scrollTop = item.offsetTop + item.offsetHeight - panel.clientHeight;
  };
  useEffect(() => {
    if (!open) return;
    // The menu takes the offer's place.
    withdraw();
    root.current?.querySelector<HTMLElement>('[aria-checked="true"]')?.focus({ preventScroll: true });
    const outside = (event: PointerEvent) => {
      if (event.target instanceof Node && !root.current?.contains(event.target)) close();
    };
    document.addEventListener("pointerdown", outside);
    return () => document.removeEventListener("pointerdown", outside);
  }, [open]);
  return (
    <>
    <div className="language-picker" ref={root}
      onBlurCapture={event => { if (open && !event.currentTarget.contains(event.relatedTarget)) close(); }}
      onKeyDown={event => {
        if (!open) {
          if (["ArrowDown", "ArrowUp"].includes(event.key)) { event.preventDefault(); keyed(true); setLeaving(false); setOpen(true); }
          return;
        }
        const items = [...(root.current?.querySelectorAll<HTMLAnchorElement>('[role="menuitemradio"]') ?? [])];
        const at = items.indexOf(document.activeElement as HTMLAnchorElement);
        const next = event.key === "ArrowDown" ? (at + 1) % items.length : event.key === "ArrowUp" ? (at + items.length - 1) % items.length : event.key === "Home" ? 0 : event.key === "End" ? items.length - 1 : -1;
        if (event.key === "Escape") { event.preventDefault(); event.stopPropagation(); close(true); }
        else if (event.key === " " && event.target instanceof HTMLAnchorElement) { event.preventDefault(); event.target.click(); }
        else if (next >= 0) { event.preventDefault(); light(items[next]); }
        else if (event.key.length === 1 && !event.ctrlKey && !event.metaKey && !event.altKey) {
          // A letter goes to the language it starts; typed again, to the next one.
          const now = Date.now();
          const text = (now - typed.current.at > 700 ? "" : typed.current.text) + event.key.toLocaleLowerCase();
          typed.current = { text, at: now };
          const again = [...text].every((letter) => letter === text[0]);
          const needle = again ? text[0] : text;
          for (let step = 0; step < items.length; step++) {
            const index = ((again ? at + 1 : Math.max(0, at)) + step) % items.length;
            if (!typeable[index].some((name) => name.startsWith(needle))) continue;
            event.preventDefault();
            light(items[index]);
            break;
          }
        }
      }}>
      <button ref={trigger} type="button" className="tool lang-trigger" aria-label={label + " · " + info.name}
        title={label + " · " + info.name} aria-haspopup="menu" aria-controls={id} aria-expanded={open}
        aria-busy={going ? true : undefined}
        onClick={event => { if (open) close(); else { keyed(event.detail === 0); setLeaving(false); setOpen(true); } }}>
        <Icon name="language" size={16} />
        <span className="lang-current" lang={info.tag}>{info.name}</span>
        <span className="lang-chevron" aria-hidden="true"><Icon name="chevron" size={14} /></span>
      </button>
      {(open || leaving) && <div id={id} role="menu" aria-label={label} inert={!open}
        className={"language-menu" + (open ? " is-open" : " is-closing")}>
        {languages.map((item, index) => <a key={item.id} className="language-option" role="menuitemradio"
          aria-checked={item.id === lang} lang={item.tag} hrefLang={item.tag} href={localized(item.id, path)}
          data-language={item.id} tabIndex={open && item.id === lang ? 0 : -1} style={{ "--i": index } as CSSProperties}
          onPointerMove={event => { keyed(false); if (document.activeElement !== event.currentTarget) event.currentTarget.focus({ preventScroll: true }); }}
          onClick={event => {
            if (item.id === lang) {
              if (plain(event)) rememberLanguage(lang);
              event.preventDefault();
              close(true);
              return;
            }
            if (go(event, item.id)) close(false, true);
          }}>
          <span>{item.name}</span>
          <span className="language-check">{item.id === lang && <Icon name="check" size={14} />}</span>
        </a>)}
      </div>}
    </div>
    {offer && <aside className={"language-offer" + (offer.open ? "" : " is-closing")} lang={languageInfo(offer.lang).tag}
      aria-labelledby={id + "-offer"} inert={!offer.open}
      onKeyDown={event => { if (event.key === "Escape") { event.preventDefault(); dismiss(true); } }}>
      <Icon name="language" size={16} />
      <p id={id + "-offer"}>{offers[offer.lang].text}</p>
      <button type="button" className="language-offer-close" aria-label={offers[offer.lang].close} title={offers[offer.lang].close}
        onClick={event => dismiss(event.detail === 0)}>
        <Icon name="close" size={14} />
      </button>
      <a className="btn is-primary is-small language-offer-go" hrefLang={languageInfo(offer.lang).tag} href={localized(offer.lang, path)}
        onClick={event => { if (go(event, offer.lang)) withdraw(); }}>
        {offers[offer.lang].action}
      </a>
    </aside>}
    </>
  );
}
