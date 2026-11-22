// Checks that depend on real animation frames run outside the reduced-motion navigation suite.
import assert from "node:assert/strict";

export async function checkDemoMotion(browser, base) {
  const page = await browser.newPage({ width: 1440, height: 900, reducedMotion: false });
  try {
    await page.goto(base + "/");
    await page.waitFor(() => {
      const app = document.querySelector(".argus-app");
      return app && Object.keys(app).some((key) => key.startsWith("__reactFiber"));
    });
    await page.eval(() => document.querySelector(".stage").scrollIntoView({ block: "center", behavior: "instant" }));
    const point = await page.eval(() => {
      const b = document.querySelector(".a-brand").getBoundingClientRect();
      return { x: b.x + b.width / 2, y: b.y + b.height / 2 };
    });
    await page.send("Input.dispatchMouseEvent", { type: "mouseMoved", ...point });
    const entry = await page.eval(async () => {
      document.querySelector('[data-tour="nav-performance"]').click();
      const positions = [];
      const start = performance.now();
      while (performance.now() - start < 700) {
        await new Promise(requestAnimationFrame);
        const body = document.querySelector(".a-perf-body");
        if (body) positions.push(body.getBoundingClientRect().top - body.parentElement.getBoundingClientRect().top);
      }
      return positions;
    });
    assert(entry.length && entry.every((y) => Math.abs(y) < 1), "Entering Performance adds no device slide");
    await page.eval(() => document.querySelector('[data-tour="nav-history"]').click());
    await page.waitFor(
      () => !!document.querySelector(".h-mini-nav canvas") && !!document.querySelector(".a-history-plot canvas"),
    );
    const positions = await page.eval(async () => {
      const chart = new Set(),
        navigator = new Set();
      const start = performance.now();
      while (performance.now() - start < 720) {
        await new Promise(requestAnimationFrame);
        for (const [selector, values] of [
          [".a-history-plot", chart],
          [".h-mini-nav", navigator],
        ]) {
          const surface = document.querySelector(selector + " canvas:last-child");
          values.add(getComputedStyle(surface).transform);
        }
      }
      return [chart.size, navigator.size];
    });
    assert(
      positions.every((count) => count > 3),
      "The history plot and navigator move between samples",
    );
    await page.eval(() => {
      const plot = document.querySelector(".a-history-plot");
      const b = plot.getBoundingClientRect();
      plot.dispatchEvent(
        new WheelEvent("wheel", { bubbles: true, cancelable: true, deltaY: -80, clientX: b.x + b.width / 2 }),
      );
    });
    await page.waitFor(() => document.querySelector(".h-follow")?.getAttribute("aria-pressed") === "false");
    await page.eval(() => {
      document.querySelector(".wallpaper").dispatchEvent(new PointerEvent("pointerdown", { bubbles: true }));
    });
    await page.waitFor(() => getComputedStyle(document.querySelector(".a-window"), "::before").opacity === "0");
    await page.eval(() => document.querySelector(".a-brand").click());
    await page.waitFor(() => getComputedStyle(document.querySelector(".a-window"), "::before").opacity === "1");
    assert.deepEqual(page.consoleErrors, [], "Animation checks complete without rendering errors");
  } finally {
    await page.close();
  }
}
