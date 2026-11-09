// Runs in the VM user's interactive session; SSH forwards this loopback API.
import assert from "node:assert/strict";
import { createServer } from "node:http";
import { spawn, spawnSync } from "node:child_process";
import { appendFileSync, existsSync, mkdirSync, readFileSync, readdirSync, writeFileSync } from "node:fs";
import { resolve, join } from "node:path";

const root = resolve(import.meta.dirname, "..");
process.chdir(root);
const readSystem = () => {
  const result = spawnSync(resolve("target/interaction-tools/interaction.exe"), ["--system"], { encoding: "utf8", windowsHide: true });
  assert.equal(result.status, 0, result.stderr);
  return JSON.parse(result.stdout);
};
const initialSystem = readSystem();
assert(initialSystem.benchmarkVm, "The desktop agent requires the dedicated QEMU VM");
assert(initialSystem.elevated, "Register the agent with the VM user's interactive elevated token");
assert(initialSystem.sessionId > 0, "Use the user's interactive session, not SSH's service session");
let session = null;
const commandValid = value => typeof value === "string" && (
  /^(?:wait [0-9]{1,5}|elements|stop)$/.test(value)
  || /^click [0-9]{1,4} [0-9]{1,4}$/.test(value)
  || /^(?:bench [a-z0-9-]+|lifecycle (?:cold|resident|restore))(?: [0-9]{1,4}){9}$/.test(value)
);
function validateRequest(value) {
  if (value.stop === true) return;
  if (value.load !== undefined) { assert(Number.isInteger(value.load) && value.load >= 0 && value.load <= 1536); return; }
  assert(/^[a-z0-9-]{1,80}$/.test(value.id));
  assert(["argus", "native"].includes(value.tool));
  assert(Array.isArray(value.commands) && value.commands.length <= 30 && value.commands.every(commandValid));
}
function sessionStatus() {
  if (!session) return null;
  const folder = session.output;
  return { label: session.label, running: session.running, exitCode: session.exitCode, ready: existsSync(join(folder, "ready.json")), reports: readdirSync(folder).filter(name => name.endsWith(".json")) };
}
function statusSample() {
  if (!session?.running) return { system: readSystem() };
  const statePath = join(session.output, "state.json");
  if (existsSync(statePath)) return JSON.parse(readFileSync(statePath));
  const environmentPath = join(session.output, "environment.json");
  const environment = existsSync(environmentPath) ? JSON.parse(readFileSync(environmentPath)) : null;
  return { phase: "initializing", sampled: environment?.started, system: environment?.system || initialSystem };
}
async function body(request) {
  let length = 0; const chunks = [];
  for await (const chunk of request) { length += chunk.length; assert(length <= 64 * 1024); chunks.push(chunk); }
  return JSON.parse(Buffer.concat(chunks).toString("utf8"));
}
const server = createServer(async (request, response) => {
  response.setHeader("content-type", "application/json");
  try {
    // Browser pages cannot send commands to the local administrative agent.
    assert(!request.headers.origin);
    assert(request.headers.host === "127.0.0.1:18762");
    const url = new URL(request.url, "http://127.0.0.1:18762");
    if (request.method === "GET" && url.pathname === "/status") {
      response.end(JSON.stringify({ pid: process.pid, ...statusSample(), session: sessionStatus() }));
    } else if (request.method === "POST" && url.pathname === "/shutdown") {
      assert(!session?.running, "Stop the benchmark session before stopping the desktop agent");
      response.end('{"stopping":true}');
      server.close();
    } else if (request.method === "POST" && url.pathname === "/display") {
      assert(!session?.running, "Finish the benchmark before changing display geometry");
      const value = await body(request);
      assert(Number.isInteger(value.width) && Number.isInteger(value.height));
      const changed = spawnSync(resolve("target/interaction-tools/interaction.exe"), ["--display", String(value.width), String(value.height)], { encoding: "utf8", windowsHide: true });
      assert.equal(changed.status, 0, changed.stderr || changed.stdout);
      response.end(JSON.stringify(readSystem()));
    } else if (request.method === "POST" && url.pathname === "/sessions") {
      assert(!session?.running, "A benchmark session is already running");
      const value = await body(request);
      assert(/^[a-z0-9-]{1,80}$/.test(value.label));
      const output = resolve("target/interaction-validation", value.label);
      assert(!existsSync(output), "Use a new label to preserve existing measurements");
      mkdirSync(output, { recursive: true });
      writeFileSync(join(output, "requests.jsonl"), "");
      const child = spawn(process.execPath, ["scripts/measure-windows-interaction.mjs", value.label, "--interactive-desktop"], { windowsHide: true, stdio: ["ignore", "pipe", "pipe"] });
      const state = { label: value.label, output, child, running: true, exitCode: null, ids: new Set() };
      session = state;
      for (const stream of [child.stdout, child.stderr]) stream.on("data", data => appendFileSync(join(output, "controller.log"), data));
      child.on("exit", code => { state.running = false; state.exitCode = code; });
      child.on("error", error => { state.running = false; appendFileSync(join(output, "controller.log"), error.message); });
      response.writeHead(201).end(JSON.stringify(sessionStatus()));
    } else if (request.method === "POST" && url.pathname === "/queue") {
      assert(session?.running && existsSync(join(session.output, "ready.json")), "Start a session and wait for readiness");
      const value = await body(request);
      validateRequest(value);
      if (value.id) { assert(!session.ids.has(value.id), "Use a unique request ID"); session.ids.add(value.id); }
      appendFileSync(join(session.output, "requests.jsonl"), JSON.stringify(value) + "\n");
      response.writeHead(202).end('{"queued":true}');
    } else if (request.method === "GET" && url.pathname === "/report") {
      assert(session);
      const name = url.searchParams.get("name");
      assert(/^[a-zA-Z0-9_.-]+\.(?:json|log)$/.test(name));
      const path = join(session.output, name);
      if (!existsSync(path)) { response.writeHead(404).end('{"pending":true}'); return; }
      response.end(readFileSync(path));
    } else if (request.method === "GET" && url.pathname === "/frame") {
      const name = url.searchParams.get("name");
      assert(/^[a-zA-Z0-9_.-]+\.png$/.test(name));
      const run = url.searchParams.get("run");
      if (run !== null) assert(session && /^[a-z0-9-]{1,80}$/.test(run));
      response.setHeader("content-type", "image/png");
      response.end(readFileSync(run === null ? resolve("target/interaction-tools", name) : join(session.output, run, name)));
    } else response.writeHead(404).end('{"error":"Unknown endpoint"}');
  } catch (error) {
    response.writeHead(400).end(JSON.stringify({ error: error.message }));
  }
});
server.listen(18762, "127.0.0.1", () => console.log("VM desktop API ready on 127.0.0.1:18762"));
const parentPid = process.ppid;
let draining = false;
setInterval(() => {
  if (draining) return;
  try { process.kill(parentPid, 0); }
  catch {
    draining = true;
    if (session?.running) appendFileSync(join(session.output, "requests.jsonl"), '{"stop":true}\n');
    server.close();
  }
}, 1000).unref();
