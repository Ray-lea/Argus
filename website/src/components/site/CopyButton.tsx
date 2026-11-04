"use client";
import { useEffect, useState } from "react";
import { Icon } from "./Icon";

// Copies a checksum or a command; the icon turns into a tick for a moment.
export function CopyButton({ text, label, done }: { text: string; label: string; done: string }) {
  const [copied, setCopied] = useState(false);
  useEffect(() => {
    if (!copied) return;
    const id = setTimeout(() => setCopied(false), 1600);
    return () => clearTimeout(id);
  }, [copied]);
  return (
    <button
      type="button"
      className={"copy-button" + (copied ? " is-done" : "")}
      aria-label={copied ? done : label}
      title={copied ? done : label}
      onClick={() => navigator.clipboard?.writeText(text).then(() => setCopied(true), () => {})}
    >
      <Icon name={copied ? "check" : "copy"} size={15} />
    </button>
  );
}
