// A small Chrome DevTools Protocol client for the website checks: a headless
// Chrome or Edge in a throwaway profile, one session per page.
import { spawn } from "node:child_process";
import { existsSync } from "node:fs";
import { mkdtemp, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { setTimeout as sleep } from "node:timers/promises";

export { sleep };

function findBrowser() {
  const candidates = [
    process.env.CHROME_PATH,
    process.env.PROGRAMFILES && join(process.env.PROGRAMFILES, "Google/Chrome/Application/chrome.exe"),
    process.env["PROGRAMFILES(X86)"] && join(process.env["PROGRAMFILES(X86)"], "Microsoft/Edge/Application/msedge.exe"),
    process.env.PROGRAMFILES && join(process.env.PROGRAMFILES, "Microsoft/Edge/Application/msedge.exe"),
    "/usr/bin/google-chrome",
    "/usr/bin/chromium",
    "/usr/bin/chromium-browser",
    "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome",
  ];
  const found = candidates.find((path) => path && existsSync(path));
  if (!found) throw Error("No Chrome or Edge found; set CHROME_PATH");
  return found;
}

export class Browser {
  static async launch() {
    const profile = await mkdtemp(join(tmpdir(), "argus-site-check-"));
    const child = spawn(
      findBrowser(),
      [
        "--headless=new",
        "--remote-debugging-port=0",
        "--user-data-dir=" + profile,
        "--no-first-run",
        "--no-default-browser-check",
        "--disable-extensions",
        "--hide-scrollbars",
        "about:blank",
      ],
      { stdio: ["ignore", "ignore", "pipe"] },
    );
    const endpoint = await new Promise((resolve, reject) => {
      let text = "";
      const timer = setTimeout(() => reject(Error("Browser did not start")), 20000);
      child.stderr.on("data", (chunk) => {
        text += chunk;
        const match = /DevTools listening on (ws:\/\/\S+)/.exec(text);
        if (match) {
          clearTimeout(timer);
          resolve(match[1]);
        }
      });
      child.once("exit", () => reject(Error("Browser exited: " + text.slice(-400))));
    });
    const socket = new WebSocket(endpoint);
    await new Promise((resolve, reject) => {
      socket.addEventListener("open", resolve, { once: true });
      socket.addEventListener("error", reject, { once: true });
    });
    return new Browser(child, socket, profile);
  }
  constructor(child, socket, profile) {
    this.child = child;
    this.socket = socket;
    this.profile = profile;
    this.id = 0;
    this.pending = new Map();
    this.listeners = new Set();
    socket.addEventListener("message", (event) => {
      const message = JSON.parse(event.data);
      if (message.id !== undefined) {
        const waiter = this.pending.get(message.id);
        if (!waiter) return;
        this.pending.delete(message.id);
        if (message.error) waiter.reject(Error(waiter.method + ": " + message.error.message));
        else waiter.resolve(message.result);
      } else for (const listener of this.listeners) listener(message);
    });
  }
  send(method, params = {}, sessionId) {
    const id = ++this.id;
    this.socket.send(JSON.stringify({ id, method, params, sessionId }));
    return new Promise((resolve, reject) => {
      const timer = setTimeout(() => {
        this.pending.delete(id);
        reject(Error("Timed out: " + method));
      }, 30000);
      this.pending.set(id, {
        method,
        resolve: (value) => {
          clearTimeout(timer);
          resolve(value);
        },
        reject: (error) => {
          clearTimeout(timer);
          reject(error);
        },
      });
    });
  }
  async newPage({ width = 1440, height = 900, mobile = false, reducedMotion = false, scheme = "dark" } = {}) {
    const { targetId } = await this.send("Target.createTarget", { url: "about:blank" });
    const { sessionId } = await this.send("Target.attachToTarget", { targetId, flatten: true });
    const page = new Page(this, targetId, sessionId);
    await page.send("Page.enable");
    await page.send("Runtime.enable");
    await page.send("Emulation.setDeviceMetricsOverride", {
      width,
      height,
      deviceScaleFactor: 1,
      mobile,
    });
    await page.setMedia({ scheme, reducedMotion });
    return page;
  }
  async close() {
    try {
      await this.send("Browser.close");
    } catch {
      this.child.kill();
    }
    await new Promise((resolve) => (this.child.exitCode === null ? this.child.once("exit", resolve) : resolve()));
    await rm(this.profile, { recursive: true, force: true }).catch(() => {});
  }
}

export class Page {
  constructor(browser, targetId, sessionId) {
    this.browser = browser;
    this.targetId = targetId;
    this.sessionId = sessionId;
    this.consoleErrors = [];
    this.loaded = null;
    this.listener = (message) => {
      if (message.sessionId !== sessionId) return;
      if (message.method === "Page.loadEventFired") this.loaded?.();
      if (message.method === "Runtime.exceptionThrown")
        this.consoleErrors.push(message.params.exceptionDetails.exception?.description ?? message.params.exceptionDetails.text);
      if (message.method === "Runtime.consoleAPICalled" && message.params.type === "error")
        this.consoleErrors.push(message.params.args.map((a) => a.value ?? a.description).join(" "));
    };
    browser.listeners.add(this.listener);
  }
  send(method, params = {}) {
    return this.browser.send(method, params, this.sessionId);
  }
  setMedia({ scheme = "dark", reducedMotion = false } = {}) {
    return this.send("Emulation.setEmulatedMedia", {
      features: [
        { name: "prefers-color-scheme", value: scheme },
        { name: "prefers-reduced-motion", value: reducedMotion ? "reduce" : "no-preference" },
      ],
    });
  }
  async goto(url, settle = 600) {
    const loaded = new Promise((resolve) => (this.loaded = resolve));
    await this.send("Page.navigate", { url });
    await Promise.race([loaded, sleep(20000)]);
    await sleep(settle);
  }
  async eval(fn, arg) {
    const expression =
      typeof fn === "function" ? "(" + fn + ")(" + JSON.stringify(arg) + ")" : fn;
    const { result, exceptionDetails } = await this.send("Runtime.evaluate", {
      expression,
      awaitPromise: true,
      returnByValue: true,
    });
    if (exceptionDetails)
      throw Error(exceptionDetails.exception?.description ?? exceptionDetails.text);
    return result.value;
  }
  async waitFor(fn, arg, timeout = 12000) {
    const end = Date.now() + timeout;
    for (;;) {
      try {
        if (await this.eval(fn, arg)) return;
      } catch {
        // A navigation can replace the context between polls.
      }
      if (Date.now() > end) throw Error("Timed out waiting for " + fn);
      await sleep(120);
    }
  }
  // Modifiers follow the protocol's bit mask: Alt 1, Ctrl 2, Meta 4, Shift 8.
  async key(key, { code = key, keyCode = 0, modifiers = 0 } = {}) {
    for (const type of ["rawKeyDown", "keyUp"])
      await this.send("Input.dispatchKeyEvent", {
        type,
        key,
        code,
        windowsVirtualKeyCode: keyCode,
        modifiers,
      });
  }
  type(text) {
    return this.send("Input.insertText", { text });
  }
  async screenshot(path, fullPage = false) {
    const { data } = await this.send("Page.captureScreenshot", {
      format: "jpeg",
      quality: 78,
      captureBeyondViewport: fullPage,
      ...(fullPage
        ? {
            clip: await this.eval(() => ({
              x: 0,
              y: 0,
              width: innerWidth,
              height: Math.min(document.documentElement.scrollHeight, 16000),
              scale: 1,
            })),
          }
        : {}),
    });
    await writeFile(path, Buffer.from(data, "base64"));
  }
  async close() {
    this.browser.listeners.delete(this.listener);
    await this.browser.send("Target.closeTarget", { targetId: this.targetId });
  }
}
