import assert from "node:assert/strict";
import { createRequire } from "node:module";
import test from "node:test";
import { pageMarkdown } from "../../website/scripts/markdown.mjs";

const require = createRequire(new URL("../../website/package.json", import.meta.url));
const MarkdownIt = require("markdown-it");
const renderer = new MarkdownIt({ html: true });

test("exported table cells preserve pipes, literal backslashes and adjacent columns", () => {
  for (const slashes of ["", "\\", "\\\\", "\\\\\\"]) {
    const value = `left${slashes}|right`;
    const cells = [
      [value, value],
      [`<strong>${value}</strong>`, `<strong>${value}</strong>`],
      [`<code>${value}</code>`, `<code>${value}</code>`],
      [`<kbd>${value}</kbd>`, `<code>${value}</code>`],
      [`<a href="/a|b" aria-label="${value}"></a>`, `<a href="https://argustask.com/a%7Cb">${value}</a>`],
      [`<img src="/a|b" alt="${value}">`, `<img src="https://argustask.com/a%7Cb" alt="${value}">`],
    ];
    const rows = cells.map(([source]) => `<tr><td>${source}</td><td>end</td></tr>`).join("");
    const markdown = pageMarkdown(`<main><table><tr><th>Value</th><th>Next</th></tr>${rows}</table></main>`, "");
    // GFM readers that pair backslashes treat a pipe after an even run as a column break.
    assert.doesNotMatch(markdown, /(?<!\\)(?:\\\\)+\|/);
    const html = renderer.render(markdown);
    assert.deepEqual([...html.matchAll(/<td>(.*?)<\/td>/g)].map((match) => match[1]),
      cells.flatMap(([, expected]) => [expected, "end"]));
  }
});

test("code punctuation stays literal in a cell and regular paragraphs keep Markdown code spans", () => {
  const source = "<code>a\\|**b** &amp; `c` &lt;d&gt;</code>";
  const table = `<table><tr><th>Value</th></tr><tr><td>${source}</td></tr></table>`;
  const markdown = pageMarkdown(`<main>${table}<p>${source}</p></main>`, "");
  const html = renderer.render(markdown);
  assert.ok(html.includes(`<td>${source}</td>`));
  assert.ok(html.includes(`<p>${source}</p>`));
  assert.ok(markdown.includes("`` a\\|**b** & `c` <d> ``"));
});
