// Native development owns its child processes and preserves the running build on compilation failure.
import { spawn } from "node:child_process";
import { createHash } from "node:crypto";
import { createReadStream, mkdirSync, copyFileSync, watch } from "node:fs";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { developmentProfile, launchIdentifier, prepare } from "./dev-data.mjs";
import { HOLD, holdGate, readHold } from "./lib/dev-hold.mjs";
import { pruneRuns } from "./lib/dev-runs.mjs";
import { t } from "./lib/console-language.mjs";

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
  const stopping = new WeakSet();
  async function stop() {
    if (!application || application.exitCode !== null) return true;
    stopping.add(application);
    application.stdin.write('{"exit":true}\n');
    for (let n = 0; n < 160 && application.exitCode === null; n++) await sleep(50);
    return application.exitCode !== null;
  }
  // Files a format change within this release cycle left unreadable (scripts/dev-data.mjs).
  // Runs while no instance holds them open.
  function clearStaleData() {
    try {
      const { stale, removed } = prepare(developmentProfile(launchIdentifier(launchArgs, root)));
      if (stale.length)
        console.log(
          t(
            `数据格式已变更，已清除开发数据：${stale.join("、")}（${removed.length} 个文件）。`,
            `Data formats changed; cleared development data: ${stale.join(", ")} (${removed.length} files).`,
          ),
        );
    } catch (error) {
      console.error(t(`未能清除格式已变更的开发数据：${error.message}`, `Could not clear outdated development data: ${error.message}`));
    }
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
          console.error(t("编译未通过，保留当前运行实例。", "Build failed; the running instance is kept."));
          if (once) process.exitCode = code;
        }
        return;
      }
      const hash = await fingerprint();
      if (hash === lastHash && application?.exitCode === null) return;
      if (!(await stop())) {
        console.error(
          t("当前实例未完成退出，保留该实例，未启动新版本。", "The running instance did not exit; it is kept and the new build was not started."),
        );
        return;
      }
      clearStaleData();
      const name = `run-${Date.now()}`;
      const directory = resolve(runs, name);
      mkdirSync(directory, { recursive: true });
      const image = resolve(directory, "argus.exe");
      copyFileSync(executable, image);
      // The previous instance has exited; older copies are no longer used.
      pruneRuns(runs, name);
      application = spawn(image, [...launchArgs, "--control-stdin"], {
        cwd: root,
        // windowsHide also sets SW_HIDE for GUI windows; Argus handles --hidden itself.
        windowsHide: false,
        stdio: ["pipe", "inherit", "inherit"],
      });
      const child = application;
      child.stdin.on("error", () => {});
      child.once("spawn", () => {
        console.log(
          t(
            `GPUI 进程已启动（PID ${child.pid}）。${once ? "" : "正在监听文件变更。"}`,
            `GPUI process started (PID ${child.pid}).${once ? "" : " Watching for file changes."}`,
          ),
        );
      });
      child.once("error", (error) => {
        console.error(t(`GPUI 启动失败：${error.message}`, `GPUI failed to start: ${error.message}`));
        if (once) process.exitCode = 1;
      });
      child.once("exit", (code, signal) => {
        if (!closed && !stopping.has(child)) {
          const report = code === 0 ? console.log : console.error;
          report(
            t(
              `GPUI 进程已退出（${signal ? `信号 ${signal}` : `退出码 ${code}`}）。${once ? "" : "修改源文件后将重新启动。"}`,
              `GPUI process exited (${signal ? `signal ${signal}` : `exit code ${code}`}).${once ? "" : " It restarts after the next source change."}`,
            ),
          );
        }
        if (once) process.exitCode = code ?? 1;
      });
      lastHash = hash;
    } finally {
      building = false;
      if (queued && !closed) {
        queued = false;
        void build();
      }
    }
  }
  // A hold (scripts/dev-hold.mjs) postpones rebuilds while an edit spans
  // several files; what changed meanwhile is built once when it ends.
  const clock = (ms) => new Date(ms).toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" });
  const gate = holdGate({
    active: () => readHold(resolve(HOLD)),
    build: () => void build(),
    report: (hold) =>
      console.log(
        hold
          ? t(
              `自动重建已暂停，至 ${clock(hold.until)} 或解除暂停为止${hold.reason ? `（${hold.reason}）` : ""}；期间的修改在恢复后重建一次。`,
              `Rebuilds are on hold until ${clock(hold.until)} or the release of the hold${hold.reason ? ` (${hold.reason})` : ""}; changes made meanwhile are built once afterwards.`,
            )
          : t("暂停已结束，开始重建。", "The hold has ended; rebuilding."),
      ),
  });
  const schedule = () => {
    clearTimeout(timer);
    timer = setTimeout(() => gate.changed(), 250);
  };
  if (!once)
    for (const path of ["src-gpui", "crates", "resources", "locales", "build-support", "vendor"]) {
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
    gate.stop();
    for (const watcher of watchers) watcher.close();
    if (!(await stop()))
      console.error(t("开发实例仍在退出，请从其托盘菜单退出。", "The development instance is still exiting; quit it from its tray menu."));
    process.exit(0);
  });
  console.log(
    t(
      "启动 GPUI 开发版；Rust 与共享资源修改后自动重建。按 Ctrl+C 退出。",
      "Starting the GPUI development build; it rebuilds after Rust or shared resource changes. Press Ctrl+C to quit.",
    ),
  );
  await build();
} else {
  throw new Error(`Unknown desktop command: ${mode}`);
}
