"use client";
import { useEffect, useLayoutEffect, useRef, useState, type MouseEvent } from "react";

// The changelog's version index: it follows the release being read, the last
// one whose top has passed the upper third of the window (the first one above
// them all). A dot slides along the line to it and the line fills up to there.
// A version chosen here is scrolled to smoothly, the index going straight to
// it rather than through the releases on the way, and its heading lights up.
export function VersionNav({
  label,
  versions,
}: {
  label: string;
  versions: { id: string; version: string; date: string; note?: string; kinds: string }[];
}) {
  const [current, setCurrent] = useState(versions[0]?.id);
  const list = useRef<HTMLOListElement>(null);
  const [mark, setMark] = useState<{ y: number; line: number } | null>(null);
  const going = useRef<string | null>(null);
  useEffect(() => {
    let frame = 0;
    const update = () => {
      frame = 0;
      if (going.current) return;
      const line = innerHeight / 3;
      let reading: string | undefined;
      for (const v of versions) {
        const el = document.getElementById(v.id);
        if (!el || el.hidden) continue;
        reading ??= v.id;
        if (el.getBoundingClientRect().top <= line) reading = v.id;
      }
      if (reading) setCurrent(reading);
    };
    const schedule = () => {
      if (!frame) frame = requestAnimationFrame(update);
    };
    update();
    addEventListener("scroll", schedule, { passive: true });
    addEventListener("resize", schedule);
    addEventListener("changelog-filter", schedule);
    return () => {
      cancelAnimationFrame(frame);
      removeEventListener("scroll", schedule);
      removeEventListener("resize", schedule);
      removeEventListener("changelog-filter", schedule);
    };
  }, [versions]);
  // The dot and the filled line follow the current entry; entries hidden by
  // the filter move the others, so the index is measured again as it resizes.
  useLayoutEffect(() => {
    const ol = list.current;
    if (!ol) return;
    const measure = () => {
      const link = ol.querySelector<HTMLElement>(`a[href="#${current}"]`);
      if (!link || !link.offsetParent) return setMark(null);
      const y = link.offsetTop + 14;
      setMark((m) => (m && m.y === y && m.line === ol.offsetHeight ? m : { y, line: ol.offsetHeight }));
    };
    measure();
    const observer = new ResizeObserver(measure);
    observer.observe(ol);
    return () => observer.disconnect();
  }, [current]);
  const go = (event: MouseEvent<HTMLAnchorElement>, id: string) => {
    const target = document.getElementById(id);
    if (!target || event.button || event.ctrlKey || event.metaKey || event.shiftKey || event.altKey) return;
    event.preventDefault();
    history.pushState(history.state, "", "#" + id);
    setCurrent(id);
    going.current = id;
    const still = matchMedia("(prefers-reduced-motion: reduce)").matches;
    target.scrollIntoView({ behavior: still ? "auto" : "smooth", block: "start" });
    target.classList.remove("is-arrived");
    void target.offsetWidth;
    target.classList.add("is-arrived");
    // The index follows the reading again once the scroll has come to rest.
    let timer = 0;
    const done = () => {
      clearTimeout(timer);
      removeEventListener("scrollend", done);
      if (going.current === id) going.current = null;
    };
    addEventListener("scrollend", done);
    timer = window.setTimeout(done, 1500);
  };
  const at = versions.findIndex((v) => v.id === current);
  return (
    <nav className="cl-nav" aria-label={label} data-ready={mark ? "" : undefined}>
      {mark && (
        <>
          <span
            className="cl-nav-fill"
            style={{ transform: `scaleY(${Math.min(1, Math.max(0, (mark.y + 4.5 - 12) / Math.max(1, mark.line - 24)))})` }}
            aria-hidden="true"
          />
          <span className="cl-nav-dot" style={{ transform: `translateY(${mark.y}px)` }} aria-hidden="true" />
        </>
      )}
      <ol ref={list}>
        {versions.map((v, i) => (
          <li key={v.id} data-kinds={v.kinds}>
            <a
              href={"#" + v.id}
              aria-current={v.id === current ? "true" : undefined}
              data-read={i < at ? "" : undefined}
              onClick={(event) => go(event, v.id)}
            >
              <b className="figures">v{v.version}</b>
              <span>{v.date}</span>
              {v.note && <small>{v.note}</small>}
            </a>
          </li>
        ))}
      </ol>
    </nav>
  );
}
