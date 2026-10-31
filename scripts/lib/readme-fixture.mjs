import assert from "node:assert/strict";
import { readFileSync, readdirSync } from "node:fs";

const source = "tools/window-probe/fixtures";
const json = path => JSON.parse(readFileSync(path, "utf8"));
const trace = json(`${source}/readme-trace.json`);
const samples = json("resources/motion.json").chartSamples;
assert.equal(trace.length, samples);
assert(trace.every((p, i) => !i || p.ms > trace[i - 1].ms), "expected consecutive readings");
for (const series of [p => p.cpu, p => p.memoryPercent, p => p.readKiB + p.writeKiB, p => p.rxKiB + p.txKiB, p => p.gpu]) {
  const values = trace.map(series);
  assert(values.every(Number.isFinite) && new Set(values).size > 1, "missing variation in a README chart");
}

const english = {
  "Windows 会话管理器": "Windows Session Manager", "客户端服务器运行时进程": "Client Server Runtime Process",
  "Windows 启动应用程序": "Windows Start-Up Application", "服务和控制器应用": "Services and Controller app",
  "Windows 服务主进程": "Host Process for Windows Services", "桌面窗口管理器": "Desktop Window Manager",
  "Windows 资源管理器": "Windows Explorer", "搜索": "Search", "计算器": "Calculator",
  "控制台窗口主进程": "Console Window Host", "画图": "Paint",
};

// Use the app names and icons of this sample desktop in both languages.
const replacements = {
  "chrome.exe": ["msedge.exe", "Microsoft Edge", "Microsoft Corporation", "C:\\Program Files (x86)\\Microsoft\\Edge\\Application\\msedge.exe"],
  "steam.exe": ["git-bash.exe", "Git Bash", "The Git Development Community", "C:\\Program Files\\Git\\git-bash.exe"],
  "Spotify.exe": ["notepad++.exe", "Notepad++", "Notepad++ Team", "C:\\Program Files\\Notepad++\\notepad++.exe"],
  "Weixin.exe": ["mspaint.exe", "画图", "Microsoft Corporation", "C:\\Windows\\System32\\mspaint.exe"],
};
const shellIcon = new Set(["csrss", "dwm", "lsass", "services", "smss", "svchost", "wininit"]);
const networkClients = new Set(["msedge.exe", "Code.exe", "QQ.exe", "cargo.exe", "svchost.exe", "msedgewebview2.exe"]);
const icons = new Map([["argus", readFileSync("resources/icons/32x32.png")]]);
for (const file of readdirSync(`${source}/readme-icons`).sort()) {
  icons.set(file.slice(0, -4), readFileSync(`${source}/readme-icons/${file}`));
}
const iconIds = new Map([...icons.keys()].map((name, index) => [name, index]));

export function previewFixture(language) {
  const fixture = json(`${source}/desktop.json`);
  fixture.icons = [...icons.values()].map(png => png.toString("base64"));
  const base = fixture.snapshot;
  base.processes = base.processes.filter(p => p.webView?.hostName !== "argus.exe"
    && !["steamwebhelper.exe", "WeChatAppEx.exe"].includes(p.name)
    && (!["steam.exe", "Weixin.exe"].includes(p.name) || p.appWindow));
  for (const p of base.processes) {
    if (replacements[p.name]) {
      [p.name, p.description, p.company, p.path] = replacements[p.name];
    }
    const key = p.name.toLowerCase().replace(/\.exe$/, "");
    p.icon = iconIds.get(shellIcon.has(key) ? "windows-executable" : key) ?? null;
    if (!networkClients.has(p.name)) p.netReceived = p.netSent = 0;
    if (language === "en-US") {
      p.description = english[p.description] ?? p.description;
      if (p.webView) p.webView.hostDescription = english[p.webView.hostDescription] ?? p.webView.hostDescription;
    }
  }
  base.hardware.cpu.name = "Intel Core i5-13500HX";
  base.system.processCount = base.processes.length;
  const sum = field => base.processes.reduce((n, p) => n + (p[field] ?? 0), 0);
  base.system.threadCount = sum("threads");
  base.system.handleCount = sum("handles");
  const totals = Object.fromEntries(["cpu", "ioRead", "ioWrite", "netReceived", "netSent", "gpu"].map(k => [k, sum(k)]));
  const cores = base.system.cores;
  const meanCore = cores.reduce((total, value) => total + value, 0) / cores.length;
  // Replay the measured totals. Split throughput and CPU time between the
  // fictional processes according to their shares in the desktop fixture.
  const frames = trace.map((point, i) => {
    const frame = structuredClone(base);
    frame.seq = i + 1;
    frame.timestamp = base.timestamp - trace.at(-1).ms + point.ms;
    const { system, hardware } = frame;
    system.cpu = point.cpu;
    system.cpuKernel = point.kernel;
    system.cores = cores.map(n => point.cpu <= meanCore
      ? n * point.cpu / meanCore
      : n + (100 - n) * (point.cpu - meanCore) / (100 - meanCore));
    system.memUsed = Math.round(system.memTotal * point.memoryPercent / 100);
    system.memAvailable = system.memTotal - system.memUsed;
    const memoryRatio = system.memUsed / base.system.memUsed;
    system.commitUsed = Math.round(system.commitUsed * memoryRatio);
    const read = Math.round(point.readKiB * 1024), write = Math.round(point.writeKiB * 1024);
    const rx = Math.round(point.rxKiB * 1024), tx = Math.round(point.txKiB * 1024);
    system.ioRead = read + rx;
    system.ioWrite = write + tx;
    for (const [j, disk] of hardware.disks.entries()) {
      disk.read = j === 0 ? read : 0;
      disk.write = j === 0 ? write : 0;
      disk.active = j === 0 ? point.active : 0;
    }
    for (const [j, nic] of hardware.nics.entries()) {
      nic.rx = j === 0 ? rx : 0;
      nic.tx = j === 0 ? tx : 0;
    }
    const [gpu] = hardware.gpus;
    const enginePeak = Math.max(...gpu.engines);
    gpu.engines = gpu.engines.map(value => value * point.gpu / enginePeak);
    gpu.util = point.gpu;
    for (const p of frame.processes) {
      p.cpu *= point.cpu / totals.cpu;
      p.diskRead = Math.round(p.ioRead * read / totals.ioRead);
      p.diskWrite = Math.round(p.ioWrite * write / totals.ioWrite);
      p.netReceived = Math.round(p.netReceived * rx / totals.netReceived);
      p.netSent = Math.round(p.netSent * tx / totals.netSent);
      p.ioRead = p.diskRead + p.netReceived;
      p.ioWrite = p.diskWrite + p.netSent;
      p.gpu *= point.gpu / totals.gpu;
      p.privateWs = Math.round(p.privateWs * memoryRatio);
      p.workingSet = Math.round(p.workingSet * memoryRatio);
      p.commit = Math.round(p.commit * memoryRatio);
    }
    return frame;
  });
  fixture.snapshot = frames.pop();
  fixture.snapshots = frames;
  fixture.settings.language = fixture.settings.locale = fixture.info.locale = language;
  return fixture;
}
