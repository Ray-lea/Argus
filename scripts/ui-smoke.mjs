// Owned native-window validation. Capture only the test HWND and reject foreground changes.
import { spawn, spawnSync } from "node:child_process";
import { mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { resolve } from "node:path";
import { createInterface } from "node:readline";
const root = resolve(import.meta.dirname, "..");
process.chdir(root);
const label = process.argv[2] ?? `smoke-${Date.now()}`;
if (!/^[a-z0-9-]+$/.test(label)) throw Error("Invalid test label");
const live = process.argv.includes("--live");
const output = resolve("target/native-validation", label);
mkdirSync(output, { recursive: true });
const metadata = JSON.parse(readFileSync("resources/app.json"));
const identifier = `${metadata.identifier}.test.${label}`;
const config = resolve(process.env.APPDATA, identifier);
mkdirSync(config, { recursive: true });
writeFileSync(
  resolve(config, "settings.json"),
  JSON.stringify({ language: "zh-CN", theme: "dark", closeToTray: false, updateCheck: false }),
);
let fixture;
if (!live) {
  fixture = JSON.parse(readFileSync("tools/window-probe/fixtures/desktop.json"));
  const samples = JSON.parse(readFileSync("resources/motion.json")).chartSamples;
  const history = fixture.historySeries ?? [];
  fixture.snapshots = Array.from({ length: samples - 1 }, (_, i) => {
    const frame = structuredClone(fixture.snapshot),
      past = history[Math.floor((i * Math.max(0, history.length - 1)) / samples)];
    frame.timestamp = fixture.snapshot.timestamp - (samples - 1 - i) * 1000;
    frame.seq = i + 1;
    if (past) {
      frame.system.cpu = past.cpu;
      frame.system.memUsed = past.memUsed;
    }
    return frame;
  });
  fixture.snapshot.seq = samples;
  writeFileSync(resolve(output, "fixture.json"), JSON.stringify(fixture));
}
const probe = resolve("target/release/argus-window-probe.exe");
const executable = resolve("target/release/argus.exe");
const child = spawn(
  probe,
  [
    "run",
    executable,
    "--hidden",
    "--test",
    "--identifier",
    identifier,
    "--stop-after",
    "180",
    ...(live ? [] : ["--fixture", resolve(output, "fixture.json")]),
  ],
  { windowsHide: true, stdio: ["pipe", "pipe", "pipe"] },
);
const events = [];
let error = "",
  shown,
  exited = false,
  exitCode;
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
child.stderr.on("data", (b) => (error += b));
child.on("error", (e) => {
  error += e.message;
  exited = true;
});
child.on("exit", (code) => {
  exited = true;
  exitCode = code;
});
createInterface({ input: child.stdout }).on("line", (line) => {
  try {
    const event = JSON.parse(line);
    events.push(event);
    if (event.event === "shown") shown = event;
  } catch {
    error += line;
  }
});
const send = (control) => child.stdin.write(`app ${JSON.stringify(control)}\n`);
const capture = (name) => {
  const result = spawnSync(probe, ["capture", String(shown.pid), resolve(output, `${name}.png`)], {
    windowsHide: true,
    encoding: "utf8",
    timeout: 20000,
  });
  if (result.status !== 0) throw Error(`Capture ${name}: ${result.stderr}`);
  console.log(`Captured ${name}`);
};
try {
  for (let i = 0; i < 600 && !shown && !exited; i++) await sleep(50);
  if (!shown) throw Error(`Window unavailable: ${error}`);
  await sleep(2500);
  if (live) {
    send({ page: "performance", motion: "balanced" });
    child.stdin.write("watch\n");
    await sleep(18000);
    capture("live-performance");
    child.stdin.write("hide\n");
    await sleep(3500);
    child.stdin.write("show\n");
    await sleep(4500);
    capture("restored-performance");
  } else {
    for (const page of [
      "processes",
      "performance",
      "services",
      "startup",
      "network",
      "history",
      "settings",
    ]) {
      send({ page });
      await sleep(650);
      capture(page);
    }
    for (const [name, state] of [
      ["processes-light", { page: "processes", theme: "light" }],
      ["processes-en-light", { language: "en-US" }],
      ["processes-en-dark", { theme: "dark" }],
      ["selected-process", { selectRow: 0 }],
      ["logical-cores", { page: "performance", cpuMode: "logical" }],
      ["cpu-telemetry", { cpuMode: "details" }],
      ["task-dialog", { dialog: "task" }],
      ["file-lock-dialog", { dismiss: true, dialog: "filelock" }],
      ["welcome-dialog", { dismiss: true, dialog: "onboarding", args: { step: 0 } }],
      ["settings-hotkey", { dismiss: true, page: "settings", settingsSection: "hotkey" }],
      ["settings-rules", { settingsSection: "rules" }],
      ["settings-update", { settingsSection: "update" }],
    ]) {
      send(state);
      await sleep(650);
      capture(name);
    }
    send({ page: "processes", language: "zh-CN", theme: "light" });
    child.stdin.write(`size ${metadata.window.minWidth} ${metadata.window.minHeight}\n`);
    await sleep(800);
    capture("narrow");
  }
} finally {
  if (!exited) {
    child.stdin.write("stop\n");
    for (let i = 0; i < 240 && !exited; i++) await sleep(50);
  }
  writeFileSync(
    resolve(output, "events.json"),
    JSON.stringify({ identifier, events, stderr: error, exitCode }, null, 2),
  );
}
if (exitCode !== 0 || error || events.some((e) => e.event === "focusViolation"))
  throw Error(`Native validation failed: ${error}`);
console.log(`Native validation passed: ${identifier}; foreground violations: 0`);
