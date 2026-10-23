// Regenerates the app icons in resources/icons from the two logo sources:
// resources/logo.svg for 48 px and up, resources/icons/logo-small.svg (bigger
// eye, heavier strokes) for 16–32 px, where the full logo stops reading.
// SVGs are rasterized by headless Microsoft Edge at each exact size.
// Usage: node scripts/icons.mjs

import { spawn } from "node:child_process";
import { existsSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const root = join(dirname(fileURLToPath(import.meta.url)), "..");
const icons = join(root, "resources", "icons");
const sources = {
  full: join(root, "resources", "logo.svg"),
  small: join(icons, "logo-small.svg"),
};
const SMALL_MAX = 32;
const ICO_SIZES = [16, 24, 32, 48, 64, 256];
const PNG_OUTPUTS = { "32x32.png": 32, "128x128.png": 128, "128x128@2x.png": 256, "icon.png": 512 };

const edge = [
  "C:\\Program Files (x86)\\Microsoft\\Edge\\Application\\msedge.exe",
  "C:\\Program Files\\Microsoft\\Edge\\Application\\msedge.exe",
].find(existsSync);
if (!edge) throw new Error("找不到 Microsoft Edge");

const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const temp = mkdtempSync(join(tmpdir(), "argus-icons-"));
const port = 9300 + Math.floor(Math.random() * 600);
const browser = spawn(
  edge,
  [
    "--headless=new",
    `--remote-debugging-port=${port}`,
    `--user-data-dir=${join(temp, "profile")}`,
    "about:blank",
  ],
  { stdio: "ignore" },
);

/** PNG bytes for every size, keyed by size, each drawn from the source for that size. */
async function rasterize(sizes) {
  let target;
  for (let i = 0; i < 50 && !target; i++) {
    await sleep(200);
    try {
      const list = await (await fetch(`http://127.0.0.1:${port}/json/list`)).json();
      target = list.find((t) => t.type === "page");
    } catch {}
  }
  if (!target) throw new Error("Edge 没有启动调试端口");
  const ws = new WebSocket(target.webSocketDebuggerUrl);
  await new Promise((r) => ws.addEventListener("open", r, { once: true }));
  let id = 0;
  const pending = new Map();
  ws.addEventListener("message", (e) => {
    const msg = JSON.parse(e.data);
    pending.get(msg.id)?.(msg);
    pending.delete(msg.id);
  });
  const send = (method, params = {}) =>
    new Promise((resolve, reject) => {
      const n = ++id;
      pending.set(n, (m) => (m.error ? reject(new Error(m.error.message)) : resolve(m.result)));
      ws.send(JSON.stringify({ id: n, method, params }));
    });

  // Data URLs: a file:// image would taint the canvas and block toDataURL.
  const dataUrl = (path) => `data:image/svg+xml;base64,${readFileSync(path).toString("base64")}`;
  const urls = { full: dataUrl(sources.full), small: dataUrl(sources.small) };
  const jobs = sizes.map((size) => ({ size, src: size <= SMALL_MAX ? urls.small : urls.full }));
  const expression = `(async (jobs) => {
    const load = (src) => new Promise((ok, fail) => { const i = new Image(); i.onload = () => ok(i); i.onerror = fail; i.src = src; });
    const out = {};
    for (const { size, src } of jobs) {
      const img = await load(src);
      const canvas = document.createElement("canvas");
      canvas.width = canvas.height = size;
      canvas.getContext("2d").drawImage(img, 0, 0, size, size);
      out[size] = canvas.toDataURL("image/png").split(",")[1];
    }
    return out;
  })(${JSON.stringify(jobs)})`;
  const result = await send("Runtime.evaluate", { expression, awaitPromise: true, returnByValue: true });
  if (result.exceptionDetails) {
    throw new Error(result.exceptionDetails.exception?.description ?? result.exceptionDetails.text);
  }
  try {
    await send("Browser.close");
  } catch {}
  ws.close();
  return Object.fromEntries(
    Object.entries(result.result.value).map(([s, b64]) => [s, Buffer.from(b64, "base64")]),
  );
}

/** An .ico whose frames are PNGs (Windows Vista and later read them at any size). */
function ico(frames) {
  const header = Buffer.alloc(6 + frames.length * 16);
  header.writeUInt16LE(0, 0);
  header.writeUInt16LE(1, 2);
  header.writeUInt16LE(frames.length, 4);
  let offset = header.length;
  frames.forEach(([size, png], i) => {
    const entry = 6 + i * 16;
    header.writeUInt8(size >= 256 ? 0 : size, entry);
    header.writeUInt8(size >= 256 ? 0 : size, entry + 1);
    header.writeUInt16LE(1, entry + 4);
    header.writeUInt16LE(32, entry + 6);
    header.writeUInt32LE(png.length, entry + 8);
    header.writeUInt32LE(offset, entry + 12);
    offset += png.length;
  });
  return Buffer.concat([header, ...frames.map(([, png]) => png)]);
}

try {
  const sizes = [...new Set([...ICO_SIZES, ...Object.values(PNG_OUTPUTS)])];
  const pngs = await rasterize(sizes);
  for (const [name, size] of Object.entries(PNG_OUTPUTS)) {
    writeFileSync(join(icons, name), pngs[size]);
    console.log(`${name.padEnd(16)} ${size}×${size}  ${pngs[size].length} B`);
  }
  const icon = ico(ICO_SIZES.map((size) => [size, pngs[size]]));
  writeFileSync(join(icons, "icon.ico"), icon);
  console.log(`${"icon.ico".padEnd(16)} ${ICO_SIZES.join(", ")}  ${icon.length} B`);
} finally {
  browser.kill();
  await sleep(500);
  rmSync(temp, { recursive: true, force: true, maxRetries: 5, retryDelay: 200 });
}
