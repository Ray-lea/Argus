"use client";
import NextLink from "next/link";
import { useRouter } from "next/navigation";
import { useEffect } from "react";
import type { AnchorHTMLAttributes, ComponentProps, FocusEvent, MouseEvent, TouchEvent } from "react";

/*
 * A link to the page already open scrolls there itself: to its section, every
 * time (the router does nothing for the address already shown), or back to
 * the top without the section in the address. Sections below the home page's
 * first screen are laid out only as they near the view, so their heights
 * change on the way: the landing is checked when the scroll stops, and the
 * scroll carries on to the section if it moved.
 */
function glide(event: MouseEvent<HTMLAnchorElement>, href: string) {
  if (event.defaultPrevented || event.button || event.metaKey || event.ctrlKey || event.shiftKey || event.altKey) return;
  const url = new URL(href, location.href);
  if (url.origin !== location.origin || url.pathname !== location.pathname || url.search !== location.search) return;
  const target = url.hash ? document.getElementById(decodeURIComponent(url.hash.slice(1))) : null;
  if (url.hash && !target) return;
  event.preventDefault();
  const behavior = matchMedia("(prefers-reduced-motion: reduce)").matches ? "auto" : "smooth";
  if (!target) {
    history.replaceState(history.state, "", url.pathname + url.search);
    scrollTo({ top: 0, behavior });
    return;
  }
  if (location.hash !== url.hash) history.pushState(history.state, "", url.pathname + url.search + url.hash);
  landOn(target, behavior);
}

/**
 * Scrolls to a section, then keeps it there while the sections around it
 * finish their layout: whenever the scroll has rested for a moment with the
 * section pushed off its place, it scrolls to it again. Ends once the section
 * has stayed in place for a while, after a few tries, or when the visitor
 * scrolls, presses a key or touches the page.
 */
export function landOn(target: Element, behavior: ScrollBehavior) {
  const padding = parseFloat(getComputedStyle(document.documentElement).scrollPaddingTop) || 0;
  const start = performance.now();
  let moved = start,
    steady = 0,
    tries = 0;
  const scrolled = () => (moved = performance.now());
  const stop = () => {
    clearInterval(timer);
    removeEventListener("scroll", scrolled);
    for (const type of ["wheel", "touchstart", "keydown"]) removeEventListener(type, stop);
  };
  const timer = window.setInterval(() => {
    const now = performance.now();
    if (now - start > 4000) return stop();
    if (now - moved < 150) return;
    if (Math.abs(target.getBoundingClientRect().top - padding) <= 2) {
      if ((steady += 120) >= 600) stop();
      return;
    }
    steady = 0;
    if (++tries > 4) return stop();
    target.scrollIntoView({ behavior, block: "start" });
    moved = now;
  }, 120);
  addEventListener("scroll", scrolled, { passive: true });
  for (const type of ["wheel", "touchstart", "keydown"]) addEventListener(type, stop, { passive: true });
  target.scrollIntoView({ behavior, block: "start" });
}

// Arriving from another page at a section (/#faq), the landing is checked the same way.
export function HashLanding() {
  useEffect(() => {
    const languageLanding = document.documentElement.getAttribute("data-argus-language-landing");
    if (languageLanding) {
      document.documentElement.removeAttribute("data-argus-language-landing");
      if (languageLanding === location.pathname) {
        try {
          const place = JSON.parse(sessionStorage.getItem("argus-place") || "null");
          sessionStorage.removeItem("argus-place");
          if (place?.to === location.pathname && Date.now() - place.at < 15000) {
            if (typeof place.hash === "string") history.replaceState(history.state, "", location.pathname + location.search + place.hash);
            const root = document.documentElement;
            const padding = parseFloat(getComputedStyle(root).scrollPaddingTop) || 0;
            const target = place.id && document.getElementById(place.id);
            scrollTo({ top: target ? target.getBoundingClientRect().top + scrollY - padding + place.within * target.offsetHeight : place.ratio * (root.scrollHeight - innerHeight), behavior: "instant" });
            return;
          }
        } catch {
          // Normal hash landing remains available if storage was cleared.
        }
      }
    }
    const target = location.hash ? document.getElementById(decodeURIComponent(location.hash.slice(1))) : null;
    if (target) landOn(target, "auto");
  }, []);
  return null;
}

// A page is fetched ahead when a visitor points at, focuses or touches its
// link, not whenever the link scrolls into view: the documentation's sidebar
// alone would otherwise fetch every page it lists as soon as it shows.
export function IntentLink({
  prefetch,
  onMouseEnter,
  onFocus,
  onTouchStart,
  onClick,
  inPage = false,
  ...props
}: ComponentProps<typeof NextLink> & { inPage?: boolean }) {
  const router = useRouter();
  const intent = () => {
    if (prefetch !== false && typeof props.href === "string") router.prefetch(props.href);
  };
  return (
    <NextLink
      {...props}
      prefetch={false}
      onClick={(event: MouseEvent<HTMLAnchorElement>) => {
        onClick?.(event);
        if (inPage && typeof props.href === "string") glide(event, props.href);
      }}
      onMouseEnter={(event: MouseEvent<HTMLAnchorElement>) => {
        onMouseEnter?.(event);
        intent();
      }}
      onFocus={(event: FocusEvent<HTMLAnchorElement>) => {
        onFocus?.(event);
        intent();
      }}
      onTouchStart={(event: TouchEvent<HTMLAnchorElement>) => {
        onTouchStart?.(event);
        intent();
      }}
    />
  );
}

// The same for links inside Fumadocs, which hands over a plain anchor's props.
export function DocsLink({ href = "#", ...props }: AnchorHTMLAttributes<HTMLAnchorElement> & { prefetch?: boolean }) {
  return <IntentLink href={href} {...props} />;
}
