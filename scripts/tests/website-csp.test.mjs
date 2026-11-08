import assert from "node:assert/strict";
import { test } from "node:test";
import { createHash } from "node:crypto";
import { mkdtemp, readFile, writeFile, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import { basename, dirname, join, resolve } from "node:path";
import { policyFor, secureHtml } from "../../website/scripts/csp.mjs";

const hash = (source) => "'sha256-" + createHash("sha256").update(source).digest("base64") + "'";
const hashes = (html) => policyFor(html).match(/'sha256-[^']+'/g) ?? [];

test("CSP hashes follow HTML parsing for closing tags and quoted attributes", () => {
  const source = "window.probe = '<tag>&amp;';";
  for (const end of ["</script>", "</script >", "</SCRIPT\t>", "</script\n>"]) {
    assert.deepEqual(hashes(`<script data-label="a > b">${source}${end}`), [hash(source)]);
  }
  assert.deepEqual(hashes("<script>one();\r\ntwo();</script>"), [hash("one();\ntwo();")]);
});

test("CSP includes executable inline scripts and speculation rules, not inert content", () => {
  const html = `<!-- <script>comment();</script> -->
    <template><script>template();</script></template>
    <textarea><script>text();</script></textarea>
    <script src="/app.js">external();</script>
    <script src="">emptySrc();</script>
    <script type="application/ld+json">{"name":"data"}</script>
    <script type="text/plain">plain();</script>
    <SCRIPT TYPE="module" data-label=" src=elsewhere">moduleCode();</SCRIPT >
    <script type="application/javascript">classic();</script>
    <script type="speculationrules">{"prerender":[]}</script>
    <script>classic();</script>`;
  assert.deepEqual(hashes(html), [hash("moduleCode();"), hash("classic();"), hash('{"prerender":[]}')]);
  assert.ok(policyFor(html).includes("script-src 'self'"));
  assert.ok(!policyFor(html).includes("script-src 'self' 'unsafe-inline'"));
});

test("CSP is inserted into the real head without changing script bytes", async () => {
  const root = resolve(await mkdtemp(join(tmpdir(), "argus-csp-test-")));
  try {
    const prefix = '<!doctype html><!-- <head> --><html><head data-label="a > b">';
    const suffix = '<script>boot();</script ></head><body>Content</body></html>';
    await writeFile(join(root, "index.html"), prefix + suffix);
    await secureHtml(root);
    const html = await readFile(join(root, "index.html"), "utf8");
    assert.ok(html.startsWith(prefix + '<meta http-equiv="content-security-policy"'));
    assert.ok(html.includes(hash("boot();")));
    assert.ok(html.endsWith(suffix));
  } finally {
    assert.equal(dirname(root), resolve(tmpdir()));
    assert.ok(basename(root).startsWith("argus-csp-test-"));
    await rm(root, { recursive: true, force: true });
  }
});
