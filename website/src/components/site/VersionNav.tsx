"use client";
import { useEffect, useState } from "react";

// The changelog's version index: it follows the release being read, the last
// one whose top has passed the upper third of the window (the first one above
// them all).
export function VersionNav({
  label,
  versions,
}: {
  label: string;
  versions: { id: string; version: string; date: string; note?: string }[];
}) {
  const [current, setCurrent] = useState(versions[0]?.id);
  useEffect(() => {
    let frame = 0;
    const update = () => {
      frame = 0;
      const line = innerHeight / 3;
      let reading = versions[0]?.id;
      for (const v of versions) {
        const el = document.getElementById(v.id);
        if (el && el.getBoundingClientRect().top <= line) reading = v.id;
      }
      setCurrent(reading);
    };
    const schedule = () => {
      if (!frame) frame = requestAnimationFrame(update);
    };
    update();
    addEventListener("scroll", schedule, { passive: true });
    addEventListener("resize", schedule);
    return () => {
      cancelAnimationFrame(frame);
      removeEventListener("scroll", schedule);
      removeEventListener("resize", schedule);
    };
  }, [versions]);
  return (
    <nav className="cl-nav" aria-label={label}>
      <ol>
        {versions.map((v) => (
          <li key={v.id}>
            <a href={"#" + v.id} aria-current={v.id === current ? "true" : undefined}>
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
