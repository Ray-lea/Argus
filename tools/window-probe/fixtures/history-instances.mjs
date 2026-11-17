// Generate synthetic history/live identities; never select a real process.
// Run from the repository root, then use steps/history-instances.json with
// --fixture target/history-instances.json.
import { mkdirSync, readFileSync, writeFileSync } from "node:fs";

const fixture = JSON.parse(readFileSync(new URL("desktop.json", import.meta.url), "utf8"));
const ts = fixture.historyMoment.system.ts;
const process = fixture.snapshot.processes[0];
const live = (pid, name, created) => ({ ...process, pid, ppid: 0, name, description: name,
  createTime: created, instanceKey: undefined, sequenceNumber: undefined, appWindow: true });
fixture.snapshot.processes = [live(44101, "restarted-worker.exe", ts + 60000),
  live(44102, "new-name.exe", ts + 60000), live(44103, "confirmed-worker.exe", ts + 1000)];
fixture.snapshot.timestamp = ts + 120000;
fixture.snapshot.system.processCount = 3;
fixture.historyMoment.processes = [
  { pid: 44101, name: "restarted-worker.exe", cpu: 85, mem: 1048576, io: 0, gpu: 0, net: 0 },
  { pid: 44102, name: "old-name.exe", cpu: 1, mem: 1048576, io: 0, gpu: 0, net: 0 },
  { pid: 44103, name: "confirmed-worker.exe", createTime: ts + 1000, cpu: 0.5, mem: 1048576, io: 0, gpu: 0, net: 0 },
];
mkdirSync("target", { recursive: true });
writeFileSync("target/history-instances.json", JSON.stringify(fixture));
