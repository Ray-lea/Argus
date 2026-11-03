// Exercise the public controls in a fresh headless browser profile. No account or credentials required.
import assert from "node:assert/strict";
import { mkdir, writeFile } from "node:fs/promises";
import { join, resolve } from "node:path";
import {
  Browser,
  sleep,
} from "../../.agents/skills/design-taste-frontend/scripts/lib/cdp.mjs";
import { workDir } from "./paths.mjs";

const base = new URL(process.argv[2] ?? "http://127.0.0.1:4321").origin;
const output = resolve(process.argv[3] ?? join(workDir, "navigation-review"));
await mkdir(output, { recursive: true });
const browser = await Browser.launch();
const results = [];
async function waitFor(page, fn) {
  for (let i = 0; i < 80; i++) {
    try {
      if (await page.eval(fn)) return;
    } catch {
      /* A new document can replace the previous context. */
    }
    await sleep(150);
  }
  throw Error("Timed out waiting for " + fn);
}
async function press(page, key, code) {
  for (const type of ["keyDown", "keyUp"])
    await page.send("Input.dispatchKeyEvent", {
      type,
      key,
      code: key,
      windowsVirtualKeyCode: code,
      ...(key === "Enter" && type === "keyDown" ? { text: "\r" } : {}),
    });
}
async function setSystem(page, scheme) {
  await page.send("Emulation.setEmulatedMedia", {
    features: [
      { name: "prefers-color-scheme", value: scheme },
      { name: "prefers-reduced-motion", value: "reduce" },
    ],
  });
}
async function choose(page, value) {
  await page.eval((v) => {
    const control = document.querySelector("header select[data-site-theme]");
    control.value = v;
    control.dispatchEvent(new Event("change", { bubbles: true }));
  }, value);
}
async function pageState(page) {
  return page.eval(() => {
    const control = document.querySelector("header select[data-site-theme]");
    const r = control.getBoundingClientRect(),
      top = document.elementFromPoint(r.x + r.width / 2, r.y + r.height / 2);
    return {
      theme: document.documentElement.dataset.theme,
      selected: control.value,
      unset: !localStorage.getItem("starlight-theme"),
      paper: getComputedStyle(document.documentElement)
        .getPropertyValue("--paper")
        .trim(),
      visible: r.width > 0 && r.height > 0 && control.contains(top),
      overflow: document.documentElement.scrollWidth - innerWidth,
    };
  });
}
try {
  for (const [width, height] of [
    [1440, 900],
    [390, 844],
  ]) {
    const page = await browser.newPage({
      width,
      height,
      mobile: width < 500,
      reducedMotion: true,
    });
    await page.send("Page.addScriptToEvaluateOnNewDocument", {
      source:
        "window.argusCsp=[];document.addEventListener('securitypolicyviolation', e => window.argusCsp.push([e.violatedDirective,e.blockedURI]));",
    });
    await setSystem(page, "light");
    await page.goto(base + "/");
    await choose(page, "auto");
    const light = await pageState(page);
    assert(light.theme === "light" && light.visible && light.overflow === 0);
    await page.eval(() =>
      document.querySelector("header select[data-site-theme]").focus(),
    );
    await press(page, "End", 35);
    await waitFor(
      page,
      () => document.documentElement.dataset.theme === "dark",
    );
    const dark = await pageState(page);
    assert.equal(dark.selected, "dark");
    assert.notEqual(dark.paper, light.paper);
    await page.eval(() =>
      document.querySelector("header details summary").focus(),
    );
    await press(page, "Enter", 13);
    await waitFor(page, () => document.querySelector("header details").open);
    const editions = await page.eval(() =>
      [...document.querySelectorAll("header details[open] a")].map((a) => [
        a.pathname,
        a.textContent.trim(),
      ]),
    );
    assert.deepEqual(
      editions.map((item) => item[0]),
      ["/docs/", "/docs/dev/"],
    );
    assert(
      editions[0][1].includes("稳定版") && editions[1][1].includes("开发版"),
    );
    await page.screenshot({ path: join(output, "versions-" + width + ".jpg") });
    await press(page, "Escape", 27);
    await waitFor(page, () => !document.querySelector("header details").open);
    await page.eval(() =>
      document.querySelector("header details summary").click(),
    );
    await page.eval(() =>
      document.querySelector('header details a[href="/docs/dev/"]').click(),
    );
    await waitFor(
      page,
      () =>
        location.pathname === "/docs/dev/" &&
        document.readyState === "complete",
    );
    const documentState = await pageState(page);
    assert(
      documentState.theme === "dark" &&
        documentState.selected === "dark" &&
        documentState.visible &&
        documentState.overflow === 0,
    );
    await page.goto(base + "/docs/dev/architecture/tech-stack/");
    await page.eval(() =>
      document.querySelector("header details summary").click(),
    );
    const pair = await page.eval(
      () => document.querySelector("header details a").pathname,
    );
    assert.equal(pair, "/docs/architecture/tech-stack/");
    await press(page, "Escape", 27);
    await choose(page, "light");
    await page.goto(base + "/en/download/");
    assert.equal((await pageState(page)).theme, "light");
    await setSystem(page, "dark");
    assert.equal((await pageState(page)).theme, "light");
    await choose(page, "auto");
    await waitFor(
      page,
      () => document.documentElement.dataset.theme === "dark",
    );
    await setSystem(page, "light");
    await waitFor(
      page,
      () => document.documentElement.dataset.theme === "light",
    );
    assert((await pageState(page)).unset);
    await page.goto(base + "/");
    await page.eval(() =>
      document.querySelector(".replay-frame").scrollIntoView(),
    );
    await waitFor(
      page,
      () =>
        !document
          .querySelector(".replay-frame")
          .closest("astro-island")
          .hasAttribute("ssr"),
    );
    await page.eval(() =>
      document.querySelector(".replay-frame button:last-child").click(),
    );
    await waitFor(page, () =>
      document.querySelector(".replay-frame").classList.contains("metric-gpu"),
    );
    const before = await page.eval(
      () => document.querySelector(".replay-frame input").value,
    );
    await page.eval(() =>
      document.querySelector(".replay-frame input").focus(),
    );
    await press(page, "ArrowRight", 39);
    assert.notEqual(
      await page.eval(
        () => document.querySelector(".replay-frame input").value,
      ),
      before,
    );
    assert.deepEqual(await page.eval(() => window.argusCsp), []);
    assert.deepEqual(page.consoleErrors, []);
    results.push({
      width,
      editions,
      pair,
      keyboard: true,
      persistent: true,
      system: true,
      replay: true,
    });
    await page.close();
  }
} finally {
  await writeFile(
    join(output, "results.json"),
    JSON.stringify(results, null, 2),
  );
  await browser.close();
}
console.log(
  "Verified website themes, edition menus and sample playback at " +
    results.map((r) => r.width).join(", ") +
    " px.",
);
