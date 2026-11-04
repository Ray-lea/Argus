"use client";
import { useEffect, useState, type ReactNode } from "react";

// A button that downloads a release file. Under the pointer its arrow dips
// toward the tray and a light passes over it; once clicked, the arrow drops
// into the tray, a tick takes its place and the label says the download has
// started, then the button settles back. Both labels share one cell, so the
// button keeps its width. The browser downloads the file as for any link.
export function DownloadButton({
  href,
  primary = false,
  className,
  started,
  quiet = false,
  children,
  extra,
}: {
  href: string;
  primary?: boolean;
  className?: string;
  /** The label once clicked. */
  started: string;
  /** Keep the label (a small button): only the icon shows the download starting. */
  quiet?: boolean;
  children: ReactNode;
  /** Shown after the label, such as the file's size. */
  extra?: ReactNode;
}) {
  // "done" holds the tick and the new label; "back" brings the arrow down into place again.
  const [state, setState] = useState<"idle" | "done" | "back">("idle");
  useEffect(() => {
    if (state === "idle") return;
    const id = setTimeout(() => setState(state === "done" ? "back" : "idle"), state === "done" ? 2600 : 600);
    return () => clearTimeout(id);
  }, [state]);
  return (
    <a
      className={"btn dl-button" + (primary ? " is-primary" : "") + (className ? " " + className : "")}
      href={href}
      data-state={state === "idle" ? undefined : state}
      onClick={(event) => {
        if (event.ctrlKey || event.metaKey || event.shiftKey || event.altKey) return;
        setState("done");
      }}
    >
      {primary && <span className="dl-sheen" aria-hidden="true" />}
      <svg
        className="dl-glyph"
        width="18"
        height="18"
        viewBox="0 0 24 24"
        fill="none"
        stroke="currentColor"
        strokeWidth="1.7"
        strokeLinecap="round"
        strokeLinejoin="round"
        aria-hidden="true"
      >
        <path className="dl-glyph-arrow" d="M12 3v12m-5-5 5 5 5-5" />
        <path className="dl-glyph-tray" d="M4 16v5h16v-5" />
        <path className="dl-glyph-tick" d="m7.5 9.5 3.2 3.2 6-6.4" pathLength={1} />
      </svg>
      <span className="dl-label">
        <span className="dl-label-idle">{children}</span>
        {!quiet && (
          <span className="dl-label-done" aria-hidden="true">
            {started}
          </span>
        )}
      </span>
      {extra}
      <span className="sr-only" aria-live="polite">
        {state === "done" ? started : ""}
      </span>
    </a>
  );
}
