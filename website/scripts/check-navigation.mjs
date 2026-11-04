// Exercise the public pages in a fresh headless browser profile and keep screenshots for review.
// Run it against a built preview (start-website.bat preview).
import assert from "node:assert/strict";
import { mkdir, writeFile } from "node:fs/promises";
import { join, resolve } from "node:path";
import { Browser } from "./browser.mjs";
import { workDir } from "./paths.mjs";

const base = new URL(process.argv[2] ?? "http://127.0.0.1:4321").origin;
const output = resolve(process.argv[3] ?? join(workDir, "review"));
await mkdir(output, { recursive: true });
const browser = await Browser.launch();
const results = [];
const violations =
  "window.argusCsp=[];document.addEventListener('securitypolicyviolation',e=>window.argusCsp.push([e.violatedDirective,e.blockedURI]));";
const state = (page) =>
  page.eval(() => ({
    theme: document.documentElement.dataset.theme,
    selected: document.querySelector("[data-site-theme]")?.dataset.siteTheme,
    stored: localStorage.getItem("argus-theme"),
    overflow: document.documentElement.scrollWidth - innerWidth,
  }));
// The theme menu opens on the next render, then takes the choice.
const choose = (page, value) =>
  page.eval(async (v) => {
    document.querySelector("[data-site-theme]").click();
    await new Promise((resolve) => setTimeout(resolve, 50));
    document.querySelector(`[data-theme-choice="${v}"]`).click();
  }, value);
async function clean(page, where) {
  assert.deepEqual(await page.eval(() => window.argusCsp), [], where + ": CSP violation");
  assert.deepEqual(page.consoleErrors, [], where + ": console errors");
}

try {
  for (const [width, height] of [
    [1440, 900],
    [768, 1024],
    [390, 844],
  ]) {
    const mobile = width < 500;
    const shot = (name) => join(output, name + "-" + width + ".jpg");

    // Without scripts the title and the window's process list are already there.
    const still = await browser.newPage({ width, height, mobile, reducedMotion: true });
    await still.send("Emulation.setScriptExecutionDisabled", { value: true });
    await still.goto(base + "/");
    const plain = await still.eval(() => ({
      title: document.querySelector("h1")?.textContent.trim(),
      rows: document.querySelectorAll(".a-tbody .a-tr").length,
    }));
    assert(plain.title && plain.rows > 10, "The first screen renders without scripts");
    await still.close();

    const page = await browser.newPage({ width, height, mobile, reducedMotion: true, scheme: "light" });
    await page.send("Page.addScriptToEvaluateOnNewDocument", { source: violations });

    // The theme follows the system until a choice is made, then keeps it.
    await page.goto(base + "/");
    await choose(page, "auto");
    assert.equal((await state(page)).theme, "light");
    await page.setMedia({ scheme: "dark", reducedMotion: true });
    await page.waitFor(() => document.documentElement.dataset.theme === "dark");
    await choose(page, "light");
    assert.equal((await state(page)).theme, "light");
    await choose(page, "dark");
    const home = await state(page);
    assert(home.theme === "dark" && home.stored === "dark" && home.overflow <= 0, "Home theme and width");
    await page.screenshot(shot("home"));

    // The window hydrates, sorts by a column and switches pages.
    await page.waitFor(
      () => {
        const root = document.querySelector(".argus-app");
        return !!root && Object.keys(root).some((key) => key.startsWith("__reactFiber"));
      },
    );
    const app = await page.eval(async () => {
      const root = document.querySelector(".argus-app");
      const wait = (ms) => new Promise((r) => setTimeout(r, ms));
      const button = (selector, text) =>
        [...root.querySelectorAll(selector)].find((b) => b.textContent.trim().startsWith(text));
      root.querySelector(".a-thead .a-col-cpu").click();
      await wait(150);
      const sorted = root.querySelectorAll(".a-group").length === 0;
      button(".a-nav button", "性能").click();
      await wait(250);
      const performance = !!root.querySelector(".a-perf");
      button(".a-nav button", "进程").click();
      await wait(150);
      return { sorted, performance };
    });
    assert(app.sorted && app.performance, "The window sorts and switches pages");
    await clean(page, "/");
    try {
      await page.screenshot(shot("home-full"), true);
    } catch {
      // Very tall captures can time out on slow machines; the first screen is kept.
    }

    // Documentation: the stored theme carries over, the sidebar opens a page, search finds text.
    await page.goto(base + "/docs/");
    assert.equal((await state(page)).theme, "dark");
    await page.screenshot(shot("docs"));
    // On narrow screens the sidebar is a drawer behind the menu button.
    const openSidebar = () =>
      page.eval(() => {
        const menu = [...document.querySelectorAll('button[aria-label="打开侧栏"]')].find((b) => b.getClientRects().length);
        if (menu && !document.querySelector("#nd-sidebar-mobile[data-open], #nd-sidebar-mobile [data-open]")) menu.click();
      });
    const sidebarLink = (match) =>
      `[...document.querySelectorAll("#nd-sidebar a, #nd-sidebar-mobile a")].find((a) => a.getClientRects().length && (${match}))`;
    await openSidebar();
    await page.waitFor(new Function("return !!" + sidebarLink("true")));
    // Pages change in place: the marker below survives only if the document was not reloaded.
    await page.eval(() => (window.argusSamePage = true));
    const target = await page.eval(
      new Function(
        "const link = " +
          sidebarLink('a.pathname.startsWith("/docs/") && a.pathname !== location.pathname') +
          "; link.click(); return link.pathname;",
      ),
    );
    await page.waitFor(
      (path) => location.pathname === path && !!document.querySelector("#nd-page h1"),
      target,
    );
    const current = await page.eval(() => ({
      marked: [...document.querySelectorAll('#nd-sidebar a[data-active="true"], #nd-sidebar-mobile a[data-active="true"]')].find((a) => a.pathname.startsWith("/docs/"))?.pathname,
      inPlace: window.argusSamePage === true,
      title: document.title,
    }));
    assert.equal(current.marked, target, "The sidebar marks the open page");
    assert(current.inPlace, "Documentation pages change without reloading");
    assert(!current.title.startsWith("Argus 文档"), "The title follows the new page");
    assert((await state(page)).overflow <= 0, "Article width");
    await page.screenshot(shot("article"));

    // The edition selector at the top of the sidebar opens the same document in
    // the development edition and back.
    const swap = async (to) => {
      await openSidebar();
      await page.eval(() =>
        [...document.querySelectorAll("#nd-sidebar button, #nd-sidebar-mobile button")]
          .find((b) => b.getClientRects().length && /稳定版|开发版/.test(b.textContent))
          .click(),
      );
      await page.waitFor(
        (id) => [...document.querySelectorAll('[role="menu"] a')].some((a) => (id === "dev") === a.pathname.startsWith("/docs/dev/")),
        to,
      );
      return page.eval((id) => {
        const link = [...document.querySelectorAll('[role="menu"] a')].find(
          (a) => (id === "dev") === a.pathname.startsWith("/docs/dev/"),
        );
        link.click();
        return link.pathname;
      }, to);
    };
    const switched = await swap("dev");
    await page.waitFor((path) => location.pathname === path, switched);
    assert(switched === "/docs/dev/" + target.slice(6), "Edition switch keeps the document");
    await page.waitFor((path) => location.pathname === path, await swap("stable"));
    assert.equal(await page.eval(() => location.pathname), target);
    assert(await page.eval(() => window.argusSamePage === true), "Edition switch without reloading");
    await page.key("k", { code: "KeyK", keyCode: 75, modifiers: 2 });
    await page.waitFor(() => !!document.querySelector('[role="dialog"] input'));
    await page.type("特权服务");
    await page.waitFor(() => document.querySelectorAll('[role="dialog"] [role="option"]').length > 0, undefined, 20000);
    const hits = await page.eval(() => document.querySelectorAll('[role="dialog"] [role="option"]').length);
    await page.screenshot(shot("search"));
    // Enter opens the highlighted result in place once the list has settled.
    await page.waitFor(() => !!document.querySelector('[role="dialog"] [role="option"][aria-selected="true"]'));
    await new Promise((resolve) => setTimeout(resolve, 400));
    await page.key("Enter", { code: "Enter", keyCode: 13 });
    const before = await page.eval(() => location.href);
    await page.waitFor((href) => location.href !== href && !document.querySelector('[role="dialog"] input'), before);
    assert(await page.eval(() => window.argusSamePage === true), "Search opens results without reloading");
    await clean(page, target);

    // The English home runs the same window, in English.
    await page.goto(base + "/en/");
    await page.waitFor(() => document.querySelector(".stage .a-nav")?.textContent.includes("Processes"));
    await clean(page, "/en/");
    results.push({ width, theme: home.theme, app, article: target, searchResults: hits });
    await page.close();
  }
} finally {
  await writeFile(join(output, "results.json"), JSON.stringify(results, null, 2));
  await browser.close();
}
console.log(
  "Checked themes, the window, documentation navigation and search at " +
    results.map((r) => r.width).join(", ") +
    " px. Screenshots: " +
    output,
);
