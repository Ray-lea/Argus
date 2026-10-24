// Measure only an owned, isolated window; the probe rejects foreground activation.
import { spawn, spawnSync } from "node:child_process";
import { mkdirSync, readFileSync, writeFileSync, copyFileSync, existsSync } from "node:fs";
import { resolve, dirname } from "node:path";
import { createInterface } from "node:readline";
import { createHash } from "node:crypto";
import { availableParallelism } from "node:os";
import assert from "node:assert/strict";
import { summarizeFrames } from "./perf-frames.mjs";

const root = resolve(import.meta.dirname, "..");
process.chdir(root);
const [label, input] = process.argv.slice(2);
const tauri = process.argv.includes("--tauri");
assert(/^[a-z0-9-]+$/.test(label ?? ""), "test label required");
const flag = (name, fallback) => {
  const index = process.argv.indexOf(name);
  return index < 0 ? fallback : process.argv[index + 1];
};
const output = resolve("target/native-validation", label);
mkdirSync(output, { recursive: true });
const executable = resolve(output, "argus.exe");
const sourceExecutable = resolve(input ?? "target/release/argus.exe");
copyFileSync(sourceExecutable, executable);
if (process.env.ARGUS_PROTOTYPE_STACK_FILE && existsSync(resolve(dirname(sourceExecutable), "argus.pdb")))
  copyFileSync(resolve(dirname(sourceExecutable), "argus.pdb"), resolve(output, "argus.pdb"));
const identifier = flag("--identifier", `dev.argus.taskmanager.test.${label}`);
assert(identifier.startsWith("dev.argus.taskmanager.test."), "isolated test identifier required");
const config = resolve(process.env.APPDATA, identifier);
mkdirSync(config, { recursive: true });
writeFileSync(
  resolve(config, "settings.json"),
  JSON.stringify({
    language: "zh-CN",
    theme: "dark",
    onboarding: 1,
    closeToTray: false,
    history: false,
    hotkeyEnabled: false,
    updateCheck: false,
  }),
);
const probe = resolve(flag("--probe", "target/rust-analyzer/debug/argus-window-probe.exe"));
const seconds = Number(flag("--seconds", 20));
const warmup = Number(flag("--warmup", 25));
const captureCycles = Number(flag("--capture-cycles", 0));
assert(Number.isInteger(captureCycles) && captureCycles >= 0 && captureCycles <= 100, "invalid capture cycle count");
const phases = flag(
  "--phases",
  "processes:fluid,processes:off,performance:fluid,logical:fluid,settings:off,hidden:off",
).split(",");
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const result = {
  measurementVersion: 3,
  label,
  identifier,
  renderer: tauri ? "tauri" : "gpui",
  sha256: createHash("sha256").update(readFileSync(executable)).digest("hex"),
  logicalProcessors: availableParallelism(),
  seconds,
  captureCycles,
  instrumentation: {
    frameTrace: !!process.env.ARGUS_GPUI_FRAME_TRACE,
    viewTrace: !!process.env.ARGUS_GPUI_VIEW_TRACE,
    prefixCompositionDisabled: !!process.env.ARGUS_GPUI_DISABLE_PREFIX_COMPOSITION,
  },
  events: [],
  summaries: [],
  invalidStructuredLines: 0,
};
const port = Number(flag("--port", 9489));
const child = spawn(
  probe,
  [
    "run",
    executable,
    "--hidden",
    ...(tauri ? [] : ["--test", "--identifier", identifier, "--stop-after", "550"]),
  ],
  {
    windowsHide: true,
    stdio: ["pipe", "pipe", "pipe"],
    env: {
      ...process.env,
      ...(tauri
        ? {
            WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS: `--remote-debugging-port=${port} --disable-features=CalculateNativeWinOcclusion`,
            WEBVIEW2_USER_DATA_FOLDER: resolve(output, "webview-profile"),
          }
        : {}),
    },
  },
);
let shown,
  exited = false,
  stderr = "";
child.on("error", (error) => {
  stderr += error.message;
  exited = true;
});
child.on("exit", (code) => {
  result.exitCode = code;
  exited = true;
});
child.stderr.on("data", (bytes) => {
  stderr += bytes;
});
createInterface({ input: child.stdout }).on("line", (line) => {
  try {
    const event = JSON.parse(line);
    result.events.push(event);
    if (event.event === "shown") shown = event;
  } catch {
    if (line.trimStart().startsWith("{")) result.invalidStructuredLines++;
    stderr += line;
  }
});
const send = (command) => child.stdin.write(`${command}\n`);
const app = (value) => send(`app ${JSON.stringify(value)}`);
async function stopWatching() {
  const previous = result.events.filter((event) => event.event === "unwatched").length;
  send("unwatch");
  for (let i = 0; i < 200 && !exited; i++) {
    if (result.events.filter((event) => event.event === "unwatched").length > previous) return true;
    await sleep(50);
  }
  return false;
}
let cdp;
async function connectWebview() {
  let endpoint;
  for (let i = 0; i < 300 && !exited; i++) {
    try {
      endpoint = (await (await fetch(`http://127.0.0.1:${port}/json/list`)).json()).find(
        (tab) => tab.type === "page",
      )?.webSocketDebuggerUrl;
    } catch {}
    if (endpoint) break;
    await sleep(200);
  }
  assert(endpoint, "owned WebView2 debugger unavailable");
  const socket = new WebSocket(endpoint),
    pending = new Map();
  let sequence = 0;
  await new Promise((resolve, reject) => {
    socket.onopen = resolve;
    socket.onerror = reject;
  });
  socket.onmessage = ({ data }) => {
    const event = JSON.parse(data);
    const waiter = pending.get(event.id);
    if (waiter) {
      pending.delete(event.id);
      event.error ? waiter.reject(Error(JSON.stringify(event.error))) : waiter.resolve(event.result);
    }
  };
  socket.onclose = () => {
    for (const waiter of pending.values()) waiter.reject(Error("owned debugger closed"));
    pending.clear();
  };
  const request = (method, params = {}) =>
    new Promise((resolve, reject) => {
      const id = ++sequence;
      pending.set(id, { resolve, reject });
      socket.send(JSON.stringify({ id, method, params }));
    });
  const evaluate = async (expression) => {
    const result = await request("Runtime.evaluate", { expression, returnByValue: true, awaitPromise: true });
    if (result.exceptionDetails) throw Error(JSON.stringify(result.exceptionDetails));
    return result.result?.value;
  };
  cdp = { request, evaluate, close: () => socket.close() };
  result.webview = await request("Browser.getVersion");
  for (let i = 0; i < 300 && !(await evaluate('!!document.querySelector(".app")')); i++) await sleep(200);
  await evaluate(
    `for (const [key,value] of Object.entries({page:"processes",perfDevice:"cpu",cpuMode:"overall","motion.chartMode":"fluid",interval:1000,detailsPanel:true,navPosition:"top"})) localStorage.setItem("argus."+key,JSON.stringify(value));`,
  );
  await request("Page.reload");
  for (let i = 0; i < 300 && !(await evaluate('!!document.querySelector(".nav-item")')); i++)
    await sleep(200);
}
async function state(page, motion) {
  if (!tauri)
    return app({
      page: page === "logical" ? "performance" : page,
      cpuMode: page === "logical" ? "logical" : "overall",
      motion,
    });
  const shell = JSON.parse(readFileSync("locales/zh-CN/shell.json"));
  const settings = JSON.parse(readFileSync("locales/zh-CN/settings.json"));
  const performance = JSON.parse(readFileSync("locales/zh-CN/performance.json"));
  const navigate = async (key) => {
    assert(
      await cdp.evaluate(
        `(()=>{const button=[...document.querySelectorAll('.nav-item')].find(el=>el.querySelector('.nav-label')?.textContent===${JSON.stringify(shell.page[key])});button?.click();return !!button})()`,
      ),
      `Tauri page ${key} unavailable`,
    );
    await sleep(400);
  };
  await navigate("settings");
  const selector = `#settings-motion button[aria-label=${JSON.stringify(settings.motion.chart.title)}]`;
  await cdp.evaluate(`document.querySelector(${JSON.stringify(selector)}).click()`);
  await cdp.evaluate(
    `document.querySelectorAll('[role=option]')[${["fluid", "balanced", "pixel", "off"].indexOf(motion)}].click()`,
  );
  await navigate(page === "logical" ? "performance" : page);
  if (page === "performance" || page === "logical") {
    await cdp.evaluate(
      `[...document.querySelectorAll('.perf-detail .segmented button')].find(el=>el.textContent===${JSON.stringify(page === "logical" ? performance.cpu.logical : performance.cpu.overall)}).click()`,
    );
  }
}
async function mark(tag) {
  send(`mark ${tag}`);
  for (let i = 0; i < 200 && !exited; i++) {
    const mark = result.events.find((event) => event.event === "mark" && event.tag === tag);
    if (mark) return mark;
    await sleep(50);
  }
  throw Error(`Missing mark ${tag}: ${stderr}`);
}
const sum = (rows, field) => rows.reduce((total, row) => total + row[field], 0);
function summarize(tag, before, after) {
  const ms = (after.qpc100ns - before.qpc100ns) / 10000;
  const uiBefore = before.threads?.find((t) => t.pid === shown.pid && t.tid === shown.uiThread);
  const uiAfter = after.threads?.find((t) => t.pid === shown.pid && t.tid === shown.uiThread && t.created === uiBefore?.created);
  const frames = summarizeFrames(result.events, before.qpc100ns, after.qpc100ns);
  const engines = {};
  for (const [key, value] of Object.entries(after.gpu?.gpuTime100ns ?? {})) {
    const engine = key.replace(/^pid_\d+_/, "");
    const delta = Math.max(0, value - (before.gpu?.gpuTime100ns?.[key] ?? value));
    engines[engine] = (engines[engine] ?? 0) + (delta / (ms * 10000)) * 100;
  }
  return {
    tag,
    durationMs: ms,
    uiThreadCpuMsPerSecond: uiBefore && uiAfter ? ((uiAfter.cpuMs-uiBefore.cpuMs)/ms)*1000 : null,
    cpuMsPerSecond: ((sum(after.processes, "cpuMs") - sum(before.processes, "cpuMs")) / ms) * 1000,
    cpuPercent:
      (((sum(after.processes, "cpuMs") - sum(before.processes, "cpuMs")) / ms) * 100) /
      result.logicalProcessors,
    gpuBusiestEnginePercent: Math.max(0, ...Object.values(engines)),
    gpuEngines: engines,
    privateMiB: sum(after.processes, "privateBytes") / 1048576,
    workingSetMiB: sum(after.processes, "workingSet") / 1048576,
    ...frames,
  };
}
try {
  for (let i = 0; i < 1200 && !shown && !exited; i++) await sleep(50);
  assert(shown, `Window unavailable: ${stderr}`);
  if (tauri) await connectWebview();
  send("size 1240 800");
  for (let i = 0; i < 200 && !result.events.some((event) => event.event === "resized") && !exited; i++)
    await sleep(50);
  result.client = result.events.findLast((event) => event.event === "resized");
  assert(result.client?.width === 1240 && result.client?.height === 800, "client size mismatch");
  send("watch");
  await state("processes", "fluid");
  console.log(`Warmup ${warmup}s: ${label}`);
  await sleep(warmup * 1000);
  for (let cycle = 0; cycle < captureCycles; cycle++) {
    assert(await stopWatching(), "capture cycle teardown failed");
    send("watch");
    await sleep(150);
  }
  if (tauri) {
    result.viewport = await cdp.evaluate("({width:innerWidth,height:innerHeight,scale:devicePixelRatio})");
    assert.equal(result.viewport.width * result.viewport.scale, 1240, "WebView physical width mismatch");
    assert.equal(result.viewport.height * result.viewport.scale, 800, "WebView physical height mismatch");
  }
  for (const [index, phase] of phases.entries()) {
    const [page, motion] = phase.split(":");
    if (page === "hidden") send("hide");
    else await state(page, motion);
    const tag = `${index}-${page}-${motion}`;
    if (!tauri) app({ pointerOutside: true, inspectLayout: `${tag}-begin` });
    // Inspection invalidates the test UI. Let it settle before the timing mark.
    await sleep(2500);
    if (!tauri) {
      const layout = result.events.findLast((event) => event.event === "ui-layout" && event.label === `${tag}-begin`);
      const conditions = result.events.findLast((event) => event.event === "ui-conditions" && event.label === `${tag}-begin`);
      assert(layout && conditions, "native viewport inspection missing");
      assert.equal(layout.width * conditions.dpiScale, 1240, "native physical width mismatch");
      assert.equal(layout.height * conditions.dpiScale, 800, "native physical height mismatch");
    }
    const before = await mark(`${tag}-begin`);
    await sleep(seconds * 1000);
    const after = await mark(`${tag}-end`);
    if (!tauri) app({ inspectLayout: `${tag}-end` });
    const summary = summarize(tag, before, after);
    result.summaries.push(summary);
    writeFileSync(resolve(output, "result.json"), JSON.stringify(result, null, 2));
    console.log(JSON.stringify(summary));
    if (page !== "hidden" && process.argv.includes("--capture")) {
      const shot = spawnSync(probe, ["capture", String(shown.pid), resolve(output, `${tag}.png`)], {
        windowsHide: true,
        encoding: "utf8",
        timeout: 20000,
      });
      assert.equal(shot.status, 0, shot.stderr);
    }
  }
} finally {
  writeFileSync(resolve(output, "result.json"), JSON.stringify(result, null, 2));
  if (!exited) {
    result.captureClosedBeforeExit = await stopWatching();
    if (tauri && cdp) await cdp.evaluate("window.__TAURI_INTERNALS__.invoke('quit')").catch(() => {});
    else app({ exit: true });
    send("stop");
  }
  for (let i = 0; i < 240 && !exited; i++) await sleep(50);
  child.stdin.end();
  cdp?.close();
  result.stderr = stderr;
  writeFileSync(resolve(output, "result.json"), JSON.stringify(result, null, 2));
}
assert.equal(result.exitCode, 0, stderr);
assert.equal(result.invalidStructuredLines, 0, "measurement stream contained corrupted JSON lines");
assert(result.captureClosedBeforeExit, "capture teardown did not complete before window shutdown");
assert(!result.events.some((event) => event.event === "focusViolation"), "focus isolation failed");
assert(
  result.events.filter((event) => event.capture).every((event) => !event.capture.error),
  "frame observer failed",
);
