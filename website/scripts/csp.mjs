// Next.js writes inline scripts (the theme, the page data) into every page.
// Each exported page gets a Content-Security-Policy that allows exactly the
// inline scripts it contains, by hash.
import { createHash } from "node:crypto";
import { glob, readFile, writeFile } from "node:fs/promises";
import { join } from "node:path";

const executable = (attributes) => {
  if (/\ssrc\s*=/i.test(attributes)) return false;
  const type = /\stype\s*=\s*["']?([^"'\s>]+)/i.exec(attributes)?.[1]?.toLowerCase();
  return !type || ["module", "text/javascript", "application/javascript"].includes(type);
};

export function policyFor(html) {
  const hashes = new Set();
  for (const [, attributes, body] of html.matchAll(/<script\b([^>]*)>([\s\S]*?)<\/script>/gi))
    if (executable(attributes))
      hashes.add("'sha256-" + createHash("sha256").update(body).digest("base64") + "'");
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
    const head = /<head[^>]*>/i.exec(html);
    if (!head) throw Error(file + ": no <head>");
    const at = head.index + head[0].length;
    const meta = '<meta http-equiv="content-security-policy" content="' + policyFor(html) + '"/>';
    await writeFile(path, html.slice(0, at) + meta + html.slice(at));
  }
}
