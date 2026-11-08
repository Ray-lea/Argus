// Next.js writes inline scripts (the theme, the page data) into every page.
// Each exported page gets a Content-Security-Policy that allows exactly the
// inline scripts it contains, by hash.
import { createHash } from "node:crypto";
import { glob, readFile, writeFile } from "node:fs/promises";
import { join } from "node:path";
import { parse } from "parse5";

function* elements(node) {
  if (node.tagName) yield node;
  for (const child of node.childNodes ?? []) yield* elements(child);
}

export function policyFor(html) {
  return policyForDocument(parse(html));
}

function policyForDocument(document) {
  const hashes = new Set();
  for (const node of elements(document)) {
    if (node.tagName !== "script" || node.attrs.some((attr) => attr.name === "src")) continue;
    const type = node.attrs.find((attr) => attr.name === "type")?.value.trim().toLowerCase();
    // Speculation rules are data, but their inline content also needs a CSP hash.
    if (type && !["module", "text/javascript", "application/javascript", "speculationrules"].includes(type)) continue;
    const body = node.childNodes.map((child) => child.value ?? "").join("");
    hashes.add("'sha256-" + createHash("sha256").update(body).digest("base64") + "'");
  }
  return [
    "default-src 'none'",
    ["script-src 'self'", ...hashes].join(" "),
    // React and the theme switch set inline styles; styles cannot run code.
    "style-src 'self' 'unsafe-inline'",
    "img-src 'self' data:",
    "font-src 'self' data:",
    "connect-src 'self'",
    "manifest-src 'self'",
    "form-action 'self'",
    "base-uri 'none'",
  ].join("; ");
}

export async function secureHtml(dir) {
  for await (const file of glob("**/*.html", { cwd: dir })) {
    const path = join(dir, file);
    const html = await readFile(path, "utf8");
    const document = parse(html, { sourceCodeLocationInfo: true });
    const head = [...elements(document)].find((node) => node.tagName === "head");
    const at = head?.sourceCodeLocation?.startTag?.endOffset;
    if (at === undefined) throw Error(file + ": no <head>");
    const meta = '<meta http-equiv="content-security-policy" content="' + policyForDocument(document) + '"/>';
    await writeFile(path, html.slice(0, at) + meta + html.slice(at));
  }
}
