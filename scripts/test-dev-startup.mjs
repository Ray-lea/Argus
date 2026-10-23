// Exercise the real launcher on an inactive desktop. The probe never shows the window itself.
import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { randomUUID } from "node:crypto";
import { mkdirSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { resolve } from "node:path";

const root = resolve(import.meta.dirname, "..");
process.chdir(root);
if (process.platform !== "win32") throw new Error("Startup visibility validation requires Windows.");
const app = JSON.parse(readFileSync("resources/app.json", "utf8"));
const output = resolve("target/startup-validation");
mkdirSync(output, { recursive: true });
function run(executable, args, options = {}) {
  const result = spawnSync(executable, args, { cwd: root, windowsHide: true, stdio: "inherit", ...options });
  if (result.error) throw result.error;
  if (result.status !== 0) throw new Error(`${executable} failed (${result.status}): ${result.stderr ?? ""}`);
  return result;
}
run("cargo", ["build", "--locked", "-p", "argus"]);
run("cargo", ["build", "--locked", "--release", "-p", "argus-window-probe", "--features", "capture"]);
for (const hidden of [false, true]) {
  const id = `${app.identifier}.test.startup.${randomUUID()}`;
  assert.match(id, /^[a-zA-Z0-9._-]+$/);
  const directories = [process.env.APPDATA, process.env.LOCALAPPDATA].map((base) => {
    assert.ok(base);
    return resolve(base, id);
  });
  mkdirSync(directories[0], { recursive: true });
  writeFileSync(
    resolve(directories[0], "settings.json"),
    JSON.stringify({
      language: "en-US",
      theme: "dark",
      closeToTray: false,
      hotkeyEnabled: false,
      history: false,
      kernelSensors: false,
      updateCheck: false,
      rules: [],
      onboarding: 1,
      seenVersion: app.version,
    }),
  );
  const label = hidden ? "hidden" : "visible";
  try {
    const result = run(
      resolve("target/release/argus-window-probe.exe"),
      [
        "startup",
        process.execPath,
        resolve("scripts/desktop.mjs"),
        "dev",
        "--once",
        "--identifier",
        id,
        "--stop-after",
        "6",
        ...(hidden ? ["--hidden"] : []),
      ],
      { encoding: "utf8", stdio: "pipe", timeout: 110_000 },
    );
    writeFileSync(resolve(output, `${label}.jsonl`), result.stdout);
    const events = result.stdout
      .trim()
      .split(/\r?\n/)
      .filter(Boolean)
      .map((line) => JSON.parse(line));
    const report = events.find((event) => event.event === "startup-result");
    assert.ok(report, "Missing startup result");
    assert.equal(report.exitCode, 0);
    assert.equal(report.foregroundViolations, 0);
    assert.ok(report.windows.length > 0, "The native window was never created");
    assert.ok(
      report.windows.every((window) => window.visible === !hidden),
      `Expected ${label} startup without probe intervention`,
    );
    console.log(`Startup ${label}: passed; foreground violations: 0`);
  } finally {
    for (const directory of directories) rmSync(directory, { recursive: true, force: true });
  }
}
