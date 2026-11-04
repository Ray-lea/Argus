// Runs the website development server. scripts/dev.mjs prepares the content
// and keeps it current; Next.js reloads pages as those files change. When a
// script that prepares the content changes, only that process starts again:
// Node keeps modules for the life of a process, so a changed data format
// would otherwise need a manual restart.
import { execFileSync, fork, spawn } from "node:child_process";
import { watch } from "node:fs";
import { join } from "node:path";
import { websiteRoot } from "./paths.mjs";

// start-website.bat keeps its earlier options: --open opens the browser, --host listens on the network.
const open = process.argv.includes("--open");
const args = process.argv
  .slice(2)
  .filter((arg) => arg !== "--open")
  .flatMap((arg) => (arg === "--host" ? ["--hostname", "0.0.0.0"] : [arg]));
if (!args.some((arg) => arg === "-p" || arg.startsWith("--port"))) args.push("--port", "4321");
const port =
  args.find((arg) => arg.startsWith("--port="))?.slice(7) ??
  args[args.findIndex((arg) => arg === "-p" || arg === "--port") + 1];

async function openWhenReady() {
  for (let attempt = 0; attempt < 120; attempt++) {
    try {
      await fetch("http://localhost:" + port + "/", { signal: AbortSignal.timeout(2000) });
      spawn("cmd", ["/c", "start", "", "http://localhost:" + port + "/"], { stdio: "ignore", detached: true });
      return;
    } catch {
      await new Promise((resolve) => setTimeout(resolve, 1000));
    }
  }
}
const watched = new Set(["content.mjs", "dev.mjs", "paths.mjs", "release.mjs"]);
let content = null,
  next = null,
  stopping = false,
  timer;

// Windows does not pass signals to grandchildren; end the whole tree.
function end(child) {
  if (!child || child.exitCode !== null) return;
  if (process.platform === "win32")
    try {
      execFileSync("taskkill", ["/pid", String(child.pid), "/t", "/f"], { stdio: "ignore" });
    } catch {
      // The process has already exited.
    }
  else child.kill();
}

function prepare() {
  const child = fork(join(websiteRoot, "scripts/dev.mjs"), { cwd: websiteRoot, stdio: "inherit" });
  content = child;
  child.on("message", (message) => {
    if (message === "ready" && !next) startNext();
  });
  child.once("exit", (code) => {
    if (content === child) content = null;
    if (!stopping && code && !child.replaced)
      console.error(
        "[website] Preparing the content failed. It runs again when a script in website/scripts changes.",
      );
  });
}

function startNext() {
  next = spawn(process.execPath, [join(websiteRoot, "node_modules/next/dist/bin/next"), "dev", ...args], {
    cwd: websiteRoot,
    stdio: "inherit",
    env: { ...process.env, NEXT_TELEMETRY_DISABLED: "1" },
  });
  if (open && process.platform === "win32") void openWhenReady();
  next.once("exit", (code) => {
    next = null;
    if (!stopping) {
      console.error(`[website] The development server stopped (exit code ${code}).`);
      stop();
    }
  });
}

function stop() {
  stopping = true;
  end(content);
  end(next);
  process.exit(0);
}

watch(join(websiteRoot, "scripts"), (event, name) => {
  if (!name || !watched.has(name)) return;
  clearTimeout(timer);
  timer = setTimeout(() => {
    console.log(`[website] scripts/${name} changed; preparing the content again…`);
    if (content) content.replaced = true;
    end(content);
    prepare();
  }, 250);
});
for (const signal of ["SIGINT", "SIGTERM", "SIGBREAK"]) process.on(signal, stop);
prepare();
