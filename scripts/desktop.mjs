// Native development owns its child processes and preserves the running build on compilation failure.
import { spawn } from "node:child_process";
import { createHash } from "node:crypto";
import { createReadStream, mkdirSync, copyFileSync, watch } from "node:fs";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const root = resolve(dirname(fileURLToPath(import.meta.url)), "..");
process.chdir(root);
const mode = process.argv[2] ?? "dev";
const once = process.argv.includes("--once");
const launchArgs = process.argv.slice(3).filter((arg) => arg !== "--once" && arg !== "--");
const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));
function run(program, args, options = {}) {
  const child = spawn(program, args, { cwd: root, windowsHide: true, stdio: "inherit", ...options });
  return new Promise((resolve, reject) => {
    child.once("error", reject);
    child.once("exit", (code) => resolve(code ?? 1));
  });
}
if (mode === "dev") {
  const executable = resolve("target/debug/argus.exe");
  const runs = resolve("target/native-dev/runs");
  mkdirSync(runs, { recursive: true });
  let application = null,
    building = false,
    queued = false,
    closed = false,
    lastHash = "",
    timer;
  const watchers = [];
  async function stop() {
    if (!application || application.exitCode !== null) return true;
    application.stdin.write('{"exit":true}\n');
    for (let n = 0; n < 160 && application.exitCode === null; n++) await sleep(50);
    return application.exitCode !== null;
  }
  async function fingerprint() {
    const hash = createHash("sha256");
    for await (const chunk of createReadStream(executable)) hash.update(chunk);
    return hash.digest("hex");
  }
  async function build() {
    if (building) {
      queued = true;
      return;
    }
    building = true;
    try {
      const code = await run("cargo", ["build", "-p", "argus"]);
      if (code !== 0 || closed) {
        if (code !== 0) {
          console.error("编译未通过，保留当前运行实例。");
          if (once) process.exitCode = code;
        }
        return;
      }
      const hash = await fingerprint();
      if (hash === lastHash && application?.exitCode === null) return;
      if (!(await stop())) {
        console.error("当前实例未完成退出，保留该实例，未启动新版本。");
        return;
      }
      const directory = resolve(runs, `run-${Date.now()}`);
      mkdirSync(directory, { recursive: true });
      const image = resolve(directory, "argus.exe");
      copyFileSync(executable, image);
      application = spawn(image, [...launchArgs, "--control-stdin"], {
        cwd: root,
        windowsHide: true,
        stdio: ["pipe", "inherit", "inherit"],
      });
      application.stdin.on("error", () => {});
      application.on("error", (error) => console.error(error.message));
      lastHash = hash;
      if (once) application.on("exit", (code) => process.exit(code ?? 1));
    } finally {
      building = false;
      if (queued && !closed) {
        queued = false;
        void build();
      }
    }
  }
  const schedule = () => {
    clearTimeout(timer);
    timer = setTimeout(() => void build(), 250);
  };
  if (!once)
    for (const path of ["src-gpui", "crates", "resources", "locales", "build-support"]) {
      watchers.push(
        watch(resolve(path), { recursive: true }, (_event, file) => {
          if (file && !String(file).includes("target")) schedule();
        }),
      );
    }
  if (!once) watchers.push(watch(resolve("Cargo.toml"), schedule));
  process.on("SIGINT", async () => {
    closed = true;
    clearTimeout(timer);
    for (const watcher of watchers) watcher.close();
    if (!(await stop())) console.error("开发实例仍在退出，请从其托盘菜单退出。");
    process.exit(0);
  });
  console.log("启动 GPUI 开发版；Rust 与共享资源修改后自动重建。按 Ctrl+C 退出。");
  await build();
} else {
  throw new Error(`Unknown desktop command: ${mode}`);
}
