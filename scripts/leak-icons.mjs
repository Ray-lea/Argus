// Replaces one visible process icon with 4096 distinct PNGs, then idles and hides.
import assert from "node:assert/strict";
import { spawn } from "node:child_process";
import { once } from "node:events";
import { existsSync, mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { join, resolve } from "node:path";
import { deflateSync } from "node:zlib";

process.chdir(resolve(import.meta.dirname, ".."));
const [label, executable] = process.argv.slice(2);
assert(/^[a-z0-9-]+$/.test(label ?? "") && executable,
  "usage: node scripts/leak-icons.mjs LABEL EXE [--probe EXE]");
const output = resolve("target/native-validation", label);
assert(!existsSync(join(output, "result.json")), "use a new measurement label");
mkdirSync(output, { recursive: true });

const crcTable = Uint32Array.from({ length: 256 }, (_, n) => {
  for (let k = 0; k < 8; k++) n = n & 1 ? 0xedb88320 ^ (n >>> 1) : n >>> 1;
  return n >>> 0;
});
function chunk(name, data) {
  const bytes = Buffer.concat([Buffer.from(name), data]);
  let crc = 0xffffffff;
  for (const byte of bytes) crc = crcTable[(crc ^ byte) & 255] ^ (crc >>> 8);
  const head = Buffer.alloc(4), tail = Buffer.alloc(4);
  head.writeUInt32BE(data.length);
  tail.writeUInt32BE((crc ^ 0xffffffff) >>> 0);
  return Buffer.concat([head, bytes, tail]);
}
function png(id) {
  const width = 96, height = 96, stride = 1 + width * 4;
  const raw = Buffer.alloc(height * stride);
  for (let y = 0; y < height; y++) {
    for (let x = 0; x < width; x++) {
      const at = y * stride + 1 + x * 4;
      raw[at] = (id + x) & 255;
      raw[at + 1] = (id >>> 8) + y;
      raw[at + 2] = (id * 17) & 255;
      raw[at + 3] = 255;
    }
  }
  const head = Buffer.alloc(13);
  head.writeUInt32BE(width);
  head.writeUInt32BE(height, 4);
  head[8] = 8;
  head[9] = 6;
  return Buffer.concat([Buffer.from([137, 80, 78, 71, 13, 10, 26, 10]),
    chunk("IHDR", head), chunk("IDAT", deflateSync(raw)), chunk("IEND", Buffer.alloc(0))]);
}
const fixture = JSON.parse(readFileSync("tools/window-probe/fixtures/desktop.json"));
fixture.snapshot.processes = [{
  ...fixture.snapshot.processes.find(process => process.path && process.pid > 4),
  pid: 47001, ppid: 0, name: "resource-test.exe", description: "Resource test",
  path: "C:\\ArgusTest\\resource-test.exe", icon: 0, appWindow: true, webView: null,
}];
fixture.icons = Array.from({ length: 4096 }, (_, id) => png(id).toString("base64"));
delete fixture.snapshots;
fixture.snapshot.timestamp = Date.now();
const fixturePath = join(output, "icons-fixture.json");
writeFileSync(fixturePath, JSON.stringify(fixture));
const sample = id => ({ sample: { at: 0, set: { "/processes/0/icon": id } }, selectRow: 0, details: "open" });
const phases = [{ name: "icon-warm", state: { page: "processes", ...sample(0) }, seconds: 15 }];
for (let round = 0; round < 4; round++) {
  phases.push({
    name: `icons-${round}`, state: { page: "processes" }, seconds: 70,
    repeat: Array.from({ length: 1024 }, (_, index) => ({ app: sample(round * 1024 + index), waitMs: 60 })),
  });
}
phases.push({ name: "icon-settle", state: { page: "processes", ...sample(0) }, seconds: 30 },
  { name: "icon-hidden", state: {}, hidden: true, seconds: 20 });
const scenario = join(output, "scenario.json");
writeFileSync(scenario, JSON.stringify(phases));
const probeAt = process.argv.indexOf("--probe");
const child = spawn(process.execPath, [
  "scripts/perf-native.mjs", label, executable, "--fixture", fixturePath,
  "--stream-fixture", "--scenario", scenario, "--warmup", "10", "--no-capture",
  ...(probeAt < 0 ? [] : ["--probe", process.argv[probeAt + 1]]),
], { windowsHide: true, stdio: "inherit" });
const [code] = await once(child, "exit");
assert.equal(code, 0, "icon resource test failed");
