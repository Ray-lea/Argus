import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { existsSync, mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";
import { test } from "node:test";

const script = resolve(import.meta.dirname, "../export-vm-bench.mjs");
function fixture() {
  const root = mkdtempSync(join(tmpdir(), "argus-vm-export-"));
  const folder = join(root, "target/interaction-validation/cohort");
  mkdirSync(folder, { recursive: true });
  const put = (name, value) => writeFileSync(join(folder, name), JSON.stringify(value));
  put("finished.json", { nativeSettingsRestored: true });
  put("environment.json", { started: "2026-11-09T00:00:00Z", argusMode: "Normal startup with an isolated profile", system: { benchmarkVm: true, cpu: "Test CPU", windowsRelease: "26H2", windowsBuild: "test", logicalProcessors: 4, totalBytes: 8 * 1024 ** 3 } });
  function run(id, tool, operation, load) {
    mkdirSync(join(folder, id));
    const names = operation === "cpu" || operation === "sort" ? [`${operation}-a`, `${operation}-b`] : [`${operation}-visible`, `${operation}-ready`];
    const events = Array.from({ length: 6 }, (_, i) => {
      const frame = `sample-${i}.png`;writeFileSync(join(folder, id, frame), "fixture");
      return { event: "measurement", operation: names[i % 2], ms: 50 + i, processCount: 140 + load, frame };
    });
    if (!["sort", "cpu"].includes(operation)) events.push(...Array.from({ length: 3 }, () => ({ event: "input-ack" })));
    events.push({ event: "complete", command: `${["sort", "cpu"].includes(operation) ? "bench" : "lifecycle"} ${operation}` });
    writeFileSync(join(folder, id, "1-final.png"), "fixture");
    put(`${id}.json`, { tool, addedProcesses: load, exitCode: 0, helperPid: 1, events });
  }
  for (let round = 1; round <= 2; round++) for (const operation of ["cold", "resident", "restore"]) for (const tool of ["argus", "native"]) run(`r${round}-${tool}-${operation}`, tool, operation, 0);
  for (const load of [0, 512, 1024]) for (let round = 1; round <= 2; round++) for (const operation of ["sort", "cpu"]) for (const tool of ["argus", "native"]) run(`p${load}-r${round}-${tool}-${operation}`, tool, operation, load);
  return { root, folder, put };
}
for (const failure of [null, "missing-frame", "failed-trial", "incomplete-cleanup"]) test(`VM cohort export: ${failure || "complete paired data"}`, () => {
  const f = fixture();
  try {
    if (failure === "missing-frame") rmSync(join(f.folder, "p1024-r2-native-cpu/1-final.png"));
    if (failure === "failed-trial") { const path = join(f.folder, "r1-argus-cold.json"); const report = JSON.parse(readFileSync(path));report.failure = "Incomplete content";writeFileSync(path, JSON.stringify(report)); }
    if (failure === "incomplete-cleanup") f.put("finished.json", { nativeSettingsRestored: false });
    const output = join(f.root, "public.json");
    const result = spawnSync(process.execPath, [script, "cohort", output], { cwd: f.root, encoding: "utf8", windowsHide: true });
    if (failure) { assert.notEqual(result.status, 0);assert(!existsSync(output)); }
    else {
      assert.equal(result.status, 0, result.stderr);
      const data = JSON.parse(readFileSync(output));assert.equal(data.runs.length, 60);
      assert.equal(data.runs.reduce((sum, run) => sum + run.ms.length, 0), 216);
      assert(data.runs.every(run => run.processCount === 140 + run.addedProcesses));
      assert(!readFileSync(output, "utf8").includes(f.root));
    }
  } finally {
    assert.equal(dirname(f.root), tmpdir());
    rmSync(f.root, { recursive: true, force: true });
  }
});
