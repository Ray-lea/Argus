// Agents that send "Accept: text/markdown" get Markdown at the page's own
// address. Workers Static Assets cannot negotiate content, so the build writes
// index.md beside every index.html and a URL Rewrite Rule on the zone serves it
// (see docs/development/website.md). Documents reuse the Markdown that "View as
// Markdown" serves; the other pages convert their <main> element.
import { glob, readFile, writeFile } from "node:fs/promises";
import { dirname, join } from "node:path";
import { parse } from "parse5";
import { origin } from "./release.mjs";

// Next.js's own error pages; the site's 404 is 404.html.
const skippedPages = new Set(["404/", "_not-found/"]);
// Interface parts with nothing to read: controls, graphics and embedded media.
const skippedTags = new Set([
  "button", "canvas", "dialog", "form", "iframe", "input", "nav", "noscript", "object",
  "script", "select", "style", "svg", "template", "textarea", "video", "audio",
]);
const headings = { h1: 1, h2: 2, h3: 3, h4: 4, h5: 5, h6: 6 };
const blockTags = new Set([
  "address", "article", "aside", "blockquote", "caption", "dd", "details", "div", "dl", "dt",
  "figcaption", "figure", "footer", "header", "hr", "li", "main", "ol", "p", "pre", "section",
  "summary", "table", "tbody", "td", "tfoot", "th", "thead", "tr", "ul", ...Object.keys(headings),
]);

const attr = (node, name) => node.attrs?.find((a) => a.name === name)?.value;
function* elements(node) {
  if (node.tagName) yield node;
  for (const child of node.content?.childNodes ?? node.childNodes ?? []) yield* elements(child);
}
const find = (node, test) => {
  for (const element of elements(node)) if (test(element)) return element;
};
const textOf = (node) =>
  node.nodeName === "#text" ? node.value : (node.childNodes ?? []).map(textOf).join("");

// Hidden from every reader, or marked data-markdown="skip" (an interactive demo).
const skipped = (node) =>
  skippedTags.has(node.tagName) ||
  attr(node, "aria-hidden") === "true" ||
  attr(node, "hidden") !== undefined ||
  attr(node, "data-markdown") === "skip";

const escape = (text) => text.replace(/([\\`*<])/g, "\\$1");
const collapse = (text) => text.replace(/[ \t\r\n\f]+/g, " ");
const wordEnd = /[\p{Script=Latin}\p{Script=Cyrillic}\p{N}.,;:!?)\]`*·]$/u;
const wordStart = /^[\p{Script=Latin}\p{Script=Cyrillic}\p{N}[`*(]/u;
// CJK and full-width punctuation carries its own spacing.
const wide = /[\u3000-\u303f\uff00-\uffef]/;

// Code whose lines are block elements without newlines between them.
function codeText(pre) {
  const code = pre.childNodes.find((child) => child.tagName === "code") ?? pre;
  const text = textOf(code);
  const lines = code.childNodes.filter((child) => child.tagName);
  return !text.includes("\n") &&
    lines.length > 1 &&
    code.childNodes.every((child) => child.tagName || child.nodeName !== "#text" || !child.value.trim())
    ? lines.map(textOf).join("\n")
    : text;
}

function converter(pageUrl, lang) {
  const absolute = (href) => {
    if (/^(?:mailto|tel):/i.test(href)) return href;
    try {
      return new URL(href, pageUrl).href;
    } catch {
      return href;
    }
  };
  // Chinese and Japanese pages separate a label from its text with a full-width colon.
  const labelSeparator = /^(?:zh|ja)\b/.test(lang) ? "：" : ": ";

  // Siblings that the page sets apart with CSS ("<b>v0.5.0</b><span>Released…</span>",
  // "<kbd>Esc</kbd>opens") keep a space between them. Punctuation stays attached, and
  // text beside an element gets a space only between Latin or Cyrillic words, not in CJK.
  function joined(parts) {
    let out = "",
      previous = false;
    for (const [text, element] of parts) {
      if (!text) continue;
      const space =
        element && previous
          ? !/\s$/.test(out) &&
            !/^[\s.,;:!?)\]\u3000-\u303f\uff00-\uffef]/.test(text) &&
            !wide.test(out.at(-1))
          : (element || previous) && wordEnd.test(out) && wordStart.test(text);
      if (out && space) out += " ";
      out += text;
      previous = element;
    }
    return out;
  }
  const part = (node) => [inline(node), Boolean(node.tagName)];

  function inline(node) {
    if (node.nodeName === "#text") return escape(collapse(node.value));
    if (!node.tagName || skipped(node)) return "";
    const inner = () => joined((node.childNodes ?? []).map(part));
    switch (node.tagName) {
      case "br":
        return "\n";
      case "a": {
        const text = inner().trim() || attr(node, "aria-label") || "";
        const href = attr(node, "href");
        if (!text) return "";
        return href ? `[${text}](${absolute(href)})` : text;
      }
      case "b":
      case "strong":
        return wrap(inner(), "**");
      case "em":
      case "i":
        return wrap(inner(), "*");
      case "code":
      case "kbd": {
        const code = collapse(textOf(node));
        if (!code.trim()) return "";
        const fence = code.includes("`") ? "``" : "`";
        return fence + (fence === "``" ? " " + code + " " : code) + fence;
      }
      case "img": {
        const alt = attr(node, "alt");
        return alt ? `![${escape(alt)}](${absolute(attr(node, "src") ?? "")})` : "";
      }
      default:
        // Block elements inside a heading or a table cell keep their text on one line.
        return blockTags.has(node.tagName) ? " " + inner() + " " : inner();
    }
  }
  // Emphasis markers must touch the text they wrap.
  function wrap(text, mark) {
    const match = /^(\s*)(.*?)(\s*)$/s.exec(text);
    return match[2] ? match[1] + mark + match[2] + mark + match[3] : text;
  }
  const line = (text) =>
    text
      .split("\n")
      .map((part) => part.replace(/ {2,}/g, " ").trim())
      .filter(Boolean)
      .join("  \n");

  // The Markdown blocks of the node's children: runs of inline content form paragraphs.
  function blocks(node) {
    const out = [];
    let run = [];
    const flush = () => {
      const text = line(joined(run));
      if (text) out.push(text);
      run = [];
    };
    for (const child of node.content?.childNodes ?? node.childNodes ?? []) {
      if (child.tagName && !skipped(child) && blockTags.has(child.tagName)) {
        flush();
        out.push(...block(child));
      } else run.push(part(child));
    }
    flush();
    return out;
  }

  function block(node) {
    const tag = node.tagName;
    if (headings[tag]) {
      const text = line(inline(node).replace(/\n/g, " "));
      return text ? ["#".repeat(headings[tag]) + " " + text] : [];
    }
    switch (tag) {
      case "hr":
        return ["* * *"];
      case "caption":
        // The table's heading already names it.
        return [];
      case "pre": {
        const code = codeText(node).replace(/\n$/, "");
        const fence = code.includes("```") ? "````" : "```";
        return code.trim() ? [fence + "\n" + code + "\n" + fence] : [];
      }
      case "blockquote":
        return [blocks(node).join("\n\n").replace(/^/gm, "> ")];
      case "ul":
      case "ol":
        return list(node, tag === "ol");
      case "table":
        return table(node);
      case "details": {
        const summary = (node.childNodes ?? []).find((child) => child.tagName === "summary");
        const title = summary ? line(inline(summary).replace(/\n/g, " ")) : "";
        const rest = { ...node, childNodes: node.childNodes.filter((child) => child !== summary) };
        return [...(title ? ["**" + title + "**"] : []), ...blocks(rest)];
      }
      case "dt":
        return [line(inline(node))].filter(Boolean).map((text) => "**" + text + "**");
      default:
        return blocks(node);
    }
  }

  function list(node, ordered) {
    const items = (node.childNodes ?? []).filter((child) => child.tagName === "li" && !skipped(child));
    const out = items.map((item, i) => {
      labelled(item);
      const marker = ordered ? `${i + 1}. ` : "- ";
      const body = blocks(item).join("\n");
      return marker + body.replace(/\n/g, "\n" + " ".repeat(marker.length));
    });
    return out.length ? [out.filter((item) => item.trim().length > 2).join("\n")] : [];
  }

  // "<li><b>Label</b>text</li>" shows the label as a title; in Markdown it needs a separator.
  function labelled(item) {
    const children = (item.childNodes ?? []).filter((child) => child.nodeName !== "#text" || child.value.trim());
    const [label, next] = children;
    if (!label || !["b", "strong"].includes(label.tagName) || !next) return;
    const following = textOf(next);
    if (/^[\s:：,，.。;；]/.test(following) || /[:：]\s*$/.test(textOf(label))) return;
    item.childNodes.splice(item.childNodes.indexOf(label) + 1, 0, {
      nodeName: "#text",
      value: labelSeparator,
    });
  }

  function table(node) {
    const rows = [...elements(node)]
      .filter((row) => row.tagName === "tr" && !skipped(row))
      .map((row) =>
        row.childNodes
          .filter((cell) => ["td", "th"].includes(cell.tagName) && !skipped(cell))
          .map((cell) => line(inline(cell).replace(/\n/g, " ")).replace(/\|/g, "\\|")),
      )
      .filter((cells) => cells.length);
    if (!rows.length) return [];
    const width = Math.max(...rows.map((cells) => cells.length));
    const row = (cells) => "| " + [...cells, ...Array(width - cells.length).fill("")].join(" | ") + " |";
    return [[row(rows[0]), row(Array(width).fill("---")), ...rows.slice(1).map(row)].join("\n")];
  }

  return (node) => blocks(node).join("\n\n");
}

// Front matter as Markdown for Agents writes it: the page's title, description and address.
function frontMatter(document, url) {
  const head = find(document, (node) => node.tagName === "head");
  const meta = (name) =>
    attr(find(head, (node) => node.tagName === "meta" && attr(node, "name") === name) ?? {}, "content");
  const title = find(head, (node) => node.tagName === "title");
  const fields = { title: title && textOf(title).trim(), description: meta("description"), url };
  return (
    "---\n" +
    Object.entries(fields)
      .filter(([, value]) => value)
      .map(([key, value]) => key + ": " + JSON.stringify(value))
      .join("\n") +
    "\n---\n\n"
  );
}

export function pageMarkdown(html, path, documentMarkdown) {
  const document = parse(html);
  const url = origin + "/" + path;
  const lang = attr(find(document, (node) => node.tagName === "html"), "lang") ?? "en";
  let body = documentMarkdown;
  if (body === undefined) {
    const main = find(document, (node) => node.tagName === "main");
    if (!main) throw Error(path + ": no <main>");
    body = converter(url, lang)(main);
  }
  return frontMatter(document, url) + body.trim() + "\n";
}

// Writes index.md beside each page and returns the number written.
export async function writeMarkdown(dir) {
  let count = 0;
  for await (const file of glob("**/index.html", { cwd: dir })) {
    const path = file.replace(/\\/g, "/").replace(/index\.html$/, "");
    if (skippedPages.has(path)) continue;
    const html = await readFile(join(dir, file), "utf8");
    const document = path.startsWith("docs/")
      ? await readFile(join(dir, "llms.mdx", path, "content.md"), "utf8")
      : undefined;
    await writeFile(join(dir, dirname(file), "index.md"), pageMarkdown(html, path, document));
    count++;
  }
  return count;
}
