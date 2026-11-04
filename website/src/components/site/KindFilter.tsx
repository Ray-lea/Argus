"use client";
import { useLayoutEffect, useRef, useState } from "react";

// Shows every release's entries, or only one kind of them (new features,
// fixes...): the other groups, the releases without that kind and their
// entries in the version index are hidden. A pill slides to the chosen
// option, and the list rises into place again from the top.
export function KindFilter({
  label,
  options,
}: {
  label: string;
  options: { kind: string; title: string; count: number }[];
}) {
  const [kind, setKind] = useState("");
  const group = useRef<HTMLDivElement>(null);
  const [pill, setPill] = useState<{ x: number; w: number } | null>(null);
  useLayoutEffect(() => {
    const el = group.current;
    if (!el) return;
    const measure = () => {
      const chosen = el.querySelector<HTMLElement>('button[aria-checked="true"]');
      if (chosen) setPill({ x: chosen.offsetLeft, w: chosen.offsetWidth });
    };
    measure();
    const observer = new ResizeObserver(measure);
    observer.observe(el);
    return () => observer.disconnect();
  }, [kind]);
  const choose = (next: string) => {
    if (next === kind) return;
    setKind(next);
    const body = group.current?.closest(".cl-body");
    if (!body) return;
    const show = (el: HTMLElement, visible: boolean) => {
      el.hidden = !visible;
    };
    for (const release of body.querySelectorAll<HTMLElement>(".cl-release, .cl-nav li"))
      show(release, !next || (release.dataset.kinds ?? "").split(" ").includes(next));
    for (const el of body.querySelectorAll<HTMLElement>(".cl-group")) show(el, !next || el.classList.contains("is-" + next));
    for (const el of body.querySelectorAll<HTMLElement>(".cl-intro, .cl-notice")) show(el, !next);
    dispatchEvent(new Event("changelog-filter"));
    // Back to the top of the list, if it has scrolled away, to read it from the start.
    const list = body.querySelector<HTMLElement>(".cl-list");
    const still = matchMedia("(prefers-reduced-motion: reduce)").matches;
    if (list && list.getBoundingClientRect().top < 0) group.current?.scrollIntoView({ behavior: still ? "auto" : "smooth", block: "start" });
    if (list && !still)
      list.animate(
        { opacity: [0, 1], transform: ["translateY(12px)", "none"] },
        { duration: 360, easing: "cubic-bezier(0.2, 0.8, 0.2, 1)" },
      );
  };
  return (
    <div className="cl-filter" role="radiogroup" aria-label={label} ref={group}>
      {pill && <span className="cl-filter-pill" style={{ width: pill.w, transform: `translateX(${pill.x}px)` }} aria-hidden="true" />}
      {options.map((option) => (
        <button
          key={option.kind}
          type="button"
          role="radio"
          aria-checked={option.kind === kind}
          data-kind={option.kind || undefined}
          onClick={() => choose(option.kind)}
        >
          {option.title}
          <small className="figures">{option.count}</small>
        </button>
      ))}
    </div>
  );
}
