// Translate complete text blocks while preserving links, code and emphasis.
import { parseFragment, serialize, serializeOuter } from "parse5";
const blocks = new Set(["p", "li", "th", "td", "h1", "h2", "h3", "h4", "h5", "h6"]);
const inline = new Set(["a", "code", "strong", "em", "b", "i", "span", "br"]);
const escape = text => text.replaceAll("&", "&amp;").replaceAll("<", "&lt;").replaceAll(">", "&gt;");

export function labelTables(html) {
  const root = parseFragment(html);
  const plain = node => node.nodeName === "#text" ? node.value : (node.childNodes ?? []).map(plain).join("");
  const visit = node => {
    if (node.tagName === "table") {
      const head = node.childNodes.find(child => child.tagName === "thead");
      const row = head?.childNodes.find(child => child.tagName === "tr");
      const labels = row?.childNodes.filter(child => child.tagName === "th").map(child => plain(child).trim()) ?? [];
      for (const body of node.childNodes.filter(child => child.tagName === "tbody"))
        for (const item of body.childNodes.filter(child => child.tagName === "tr"))
          item.childNodes.filter(child => child.tagName === "td").forEach((cell,index) => {
            if (labels[index]) cell.attrs.push({ name: "data-label", value: labels[index] });
          });
    }
    node.childNodes?.forEach(visit);
  };
  visit(root);
  return serialize(root);
}

export function translateHtml(html, messages, { collect = new Set(), strict = true } = {}) {
  const root = parseFragment(html);
  const text = source => {
    if (!source.trim() || !/[A-Za-z]/.test(source.replace(/\{(?:link|code|inline)\d+\}/g, ""))) return source;
    collect.add(source);
    const result = messages[source];
    if (result === undefined) {
      if (strict) throw Error("Missing HTML translation: " + source);
      return source;
    }
    const holes = value => [...value.matchAll(/\{(?:link|code|inline)\d+\}/g)].map(match => match[0]).sort().join();
    if (holes(source) !== holes(result)) throw Error("HTML placeholders changed: " + source);
    return result;
  };
  const content = node => {
    const saved = {}, counts = { link: 0, code: 0, inline: 0 };
    const source = node.childNodes.map(child => {
      if (child.nodeName === "#text") return child.value;
      if (child.tagName === "br") return "\n";
      const kind = child.tagName === "a" ? "link" : child.tagName === "code" ? "code" : "inline";
      const key = kind + counts[kind]++;
      if (kind !== "code") content(child);
      saved[key] = serializeOuter(child);
      return "{" + key + "}";
    }).join("");
    const translated = escape(text(source)).replace(/\{((?:link|code|inline)\d+)\}/g, (_, key) => saved[key]);
    node.childNodes = parseFragment(translated).childNodes;
    for (const child of node.childNodes) child.parentNode = node;
  };
  const visit = node => {
    if (blocks.has(node.tagName) && node.childNodes.every(child => child.nodeName === "#text" || inline.has(child.tagName))) content(node);
    else node.childNodes?.forEach(visit);
  };
  visit(root);
  return serialize(root);
}
