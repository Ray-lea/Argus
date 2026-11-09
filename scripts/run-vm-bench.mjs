// Run paired trials through the interactive VM agent, without opening a console.
import assert from "node:assert/strict";
import { cpSync, existsSync, mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { basename, join, resolve } from "node:path";

const label = process.argv[2];
assert(typeof label === "string" && /^[a-z0-9-]{1,80}$/.test(label), "Provide a new measurement label");
const output = resolve("target/interaction-validation", label);
assert(!existsSync(output), "Use a new label to preserve local measurements");
const resumeLabel = process.argv[3] === "--resume" ? process.argv[4] : null;
assert(process.argv.length <= 3 || (process.argv.length === 5 && /^[a-z0-9-]{1,80}$/.test(resumeLabel)), "Use --resume <previous label>");
const previous = resumeLabel ? resolve("target/interaction-validation", resumeLabel) : null;
mkdirSync(output, { recursive: true });
let environment;
const base = "http://127.0.0.1:18762";
const sleep = ms => new Promise(done => setTimeout(done, ms));
async function api(path, body) {
  const response = await fetch(base + path, { method: body === undefined ? "GET" : "POST", headers: body === undefined ? {} : { "content-type": "application/json" }, body: body === undefined ? undefined : JSON.stringify(body), signal: AbortSignal.timeout(15000) });
  if (response.status === 404) return null;
  const value = await response.json();
  assert(response.ok, value.error || `HTTP ${response.status}`);
  return value;
}
async function report(name, timeout = 240000) {
  const deadline = Date.now() + timeout;
  while (Date.now() < deadline) {
    const value = await api(`/report?name=${encodeURIComponent(name)}`);
    if (value) return value;
    const status = await api("/status");
    assert(status.session?.running, "The measurement controller stopped");
    await sleep(500);
  }
  throw new Error(`Timed out waiting for ${name}`);
}
const geometry = {
  argus: { sort: "490 267 490 267 16 285 1200 482", cpu: "52 72 145 72 16 285 1200 482", life: "52 72 600 30 16 285 1200 482" },
  native: { sort: "220 118 220 118 60 130 1160 668", cpu: "24 316 24 156 60 130 1160 668", life: "24 316 600 26 60 130 1160 668" },
};
async function trial(id, tool, command) {
  // Sorting is always recollected. Other completed operations use the same
  // application builds and completion checks, retaining their original cohort.
  if (previous && !command.startsWith("bench sort ") && existsSync(join(previous, `${id}.json`))) {
    const prior = JSON.parse(readFileSync(join(previous, `${id}.json`)));
    if (!prior.failure && prior.exitCode === 0) {
      cpSync(join(previous, id), join(output, id), { recursive: true });
      writeFileSync(join(output, `${id}.json`), JSON.stringify({ ...prior, sourceCohort: prior.sourceCohort || resumeLabel, probeSHA256: prior.probeSHA256 || environment.resumedFrom.probeSHA256 }, null, 2));
      console.log(`${id}: retained completed samples from ${prior.sourceCohort || resumeLabel}`);
      return;
    }
  }
  await api("/queue", { id, tool, commands: ["wait 3000", command] });
  const value = await report(`${id}.json`);
  value.sourceCohort = label;
  value.probeSHA256 = environment.probeSHA256;
  writeFileSync(join(output, `${id}.json`), JSON.stringify(value, null, 2));
  const frames = new Set(value.events.filter(event => event.event === "measurement").map(event => event.frame));
  for (const event of value.events.filter(event => event.event === "reference")) { frames.add(event.a); frames.add(event.b); }
  if (!value.failure && value.exitCode === 0) frames.add(`${value.helperPid}-final.png`);
  for (const frame of frames) {
    const name = basename(frame.replaceAll("\\", "/"));
    const response = await fetch(`${base}/frame?run=${id}&name=${encodeURIComponent(name)}`);
    assert(response.ok, "Measurement frame is unavailable");
    const folder = join(output, id); mkdirSync(folder, { recursive: true });
    writeFileSync(join(folder, name), Buffer.from(await response.arrayBuffer()));
  }
  assert(!value.failure && value.exitCode === 0, `${id}: ${value.stderr || value.failure}`);
  console.log(`${id}: ${value.events.filter(event => event.event === "measurement").map(event => `${event.operation}=${event.ms.toFixed(1)} ms`).join(", ")}`);
}
const status = await api("/status");
assert(status.system.benchmarkVm && status.system.elevated && status.system.sessionId > 0);
assert.deepEqual(status.system.desktopPixels, [1920, 1080]);
assert.equal(status.system.dpi, 96);
assert(!status.session?.running, "Finish the previous session first");
await api("/sessions", { label });
const readyBy = Date.now() + 60000;
while (true) {
  const current = await api("/status");
  assert(current.session?.running, "The measurement controller exited during setup");
  if (current.session.ready) break;
  assert(Date.now() < readyBy, "The measurement controller did not become ready");
  await sleep(300);
}
try {
  environment = await report("environment.json");
  if (previous) {
    const prior = JSON.parse(readFileSync(join(previous, "environment.json")));
    assert(!prior.resumedFrom, "Resume directly from the original cohort");
    for (const field of ["argusSHA256", "nativeSHA256", "clientPixels", "settings", "argusMode", "build", "privileges"]) assert.deepEqual(environment[field], prior[field], `Changed condition: ${field}`);
    for (const field of ["cpu", "logicalProcessors", "totalBytes", "windowsBuild", "desktopPixels", "dpi", "refreshHz"]) assert.deepEqual(environment.system[field], prior.system[field], `Changed system: ${field}`);
    environment.resumedFrom = { id: resumeLabel, started: prior.started, probeSHA256: prior.probeSHA256, retainedOperations: "Completed launch, restore and CPU-page samples; every sort trial is recollected" };
  }
  writeFileSync(join(output, "environment.json"), JSON.stringify(environment, null, 2));
  for (let round = 1; round <= 2; round++) for (const mode of ["cold", "resident", "restore"]) for (const tool of round === 1 ? ["argus", "native"] : ["native", "argus"])
    await trial(`r${round}-${tool}-${mode}`, tool, `lifecycle ${mode} ${geometry[tool].life} 3`);
  for (const load of [0, 512, 1024]) {
    await api("/queue", { load });
    writeFileSync(join(output, `load-${load}.json`), JSON.stringify(await report(`load-${load}.json`), null, 2));
    for (let round = 1; round <= 2; round++) for (const operation of ["sort", "cpu"]) for (const tool of round === 1 ? ["argus", "native"] : ["native", "argus"])
      await trial(`p${load}-r${round}-${tool}-${operation}`, tool, `bench ${operation} ${geometry[tool][operation]} 3`);
  }
} finally {
  const current = await api("/status");
  if (current.session?.running) await api("/queue", { stop: true });
  const deadline = Date.now() + 30000;
  while ((await api("/status")).session?.running && Date.now() < deadline) await sleep(500);
  const finished = await api("/report?name=finished.json");
  if (finished) writeFileSync(join(output, "finished.json"), JSON.stringify(finished, null, 2));
}
