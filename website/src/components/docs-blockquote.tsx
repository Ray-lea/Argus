import { Callout } from "fumadocs-ui/components/callout";
import { Children, cloneElement, isValidElement, type ComponentProps, type ReactNode } from "react";

// GitHub's alert syntax ("> [!WARNING]" on the first line of a blockquote) as
// Fumadocs callouts, with the colors GitHub gives each kind and a Chinese
// title. The kinds and when to use them: docs/development/docs-style.md#告警块.
const kinds = {
  NOTE: { type: "info", title: "说明" },
  TIP: { type: "idea", title: "提示" },
  IMPORTANT: { type: "info", title: "重要" },
  WARNING: { type: "warning", title: "警告" },
  CAUTION: { type: "error", title: "危险" },
} as const;
const marker = /^\[!(NOTE|TIP|IMPORTANT|WARNING|CAUTION)\][ \t]*\r?\n?/;

const blank = (node: ReactNode) => typeof node === "string" && !node.trim();

/** A documentation blockquote: an alert of its kind, or a plain callout. */
export function DocsBlockquote({ children }: ComponentProps<"blockquote">) {
  const blocks = Children.toArray(children).filter((child) => !blank(child));
  const [first, ...others] = blocks;
  if (isValidElement<{ children?: ReactNode }>(first)) {
    const [lead, ...inline] = Children.toArray(first.props.children);
    const match = typeof lead === "string" ? marker.exec(lead) : null;
    if (typeof lead === "string" && match) {
      const kind = kinds[match[1] as keyof typeof kinds];
      const text = [lead.slice(match[0].length), ...inline].filter((part) => !blank(part));
      // "[!NOTE]" alone in the first paragraph when a blank line follows it.
      const body = text.length ? [cloneElement(first, undefined, ...text), ...others] : others;
      return (
        <Callout type={kind.type} title={kind.title}>
          {body}
        </Callout>
      );
    }
  }
  return <Callout>{blocks}</Callout>;
}
