import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import test from "node:test";
import { previewFixture } from "../lib/readme-fixture.mjs";

test("both README previews replay all five measured series through the final readout", () => {
  const trace = JSON.parse(readFileSync("tools/window-probe/fixtures/readme-trace.json", "utf8"));
  for (const language of ["zh-CN", "en-US"]) {
    const fixture = previewFixture(language);
    const frames = [...fixture.snapshots, fixture.snapshot];
    assert.equal(frames.length, trace.length);
    for (const [i, { timestamp, system, hardware }] of frames.entries()) {
      const point = trace[i];
      assert.equal(timestamp - frames[0].timestamp, point.ms);
      assert.equal(system.cpu, point.cpu);
      assert.equal(system.memUsed, Math.round(system.memTotal * point.memoryPercent / 100));
      assert.equal(hardware.disks.reduce((n, d) => n + d.read + d.write, 0),
        Math.round(1024 * point.readKiB) + Math.round(1024 * point.writeKiB));
      assert.equal(hardware.nics.reduce((n, d) => n + d.rx + d.tx, 0),
        Math.round(1024 * point.rxKiB) + Math.round(1024 * point.txKiB));
      assert.equal(Math.max(...hardware.gpus.map(g => g.util)), point.gpu);
    }
    const apps = ["argus.exe", "msedge.exe", "Code.exe", "explorer.exe", "QQ.exe"];
    const images = apps.map(name => {
      const row = fixture.snapshot.processes.find(p => p.name === name);
      const png = Buffer.from(fixture.icons[row.icon], "base64");
      assert.equal(png.toString("hex", 0, 8), "89504e470d0a1a0a");
      return fixture.icons[row.icon];
    });
    assert.equal(new Set(images).size, apps.length);
  }
});
