// Alternate read-only process sources under the same token and sampling cadence.
// The executable emits aggregates only; no process names, paths or command lines.
import { spawn } from "node:child_process";
import { mkdirSync, readFileSync, readdirSync, writeFileSync } from "node:fs";
import { resolve } from "node:path";
import { createInterface } from "node:readline";
import assert from "node:assert/strict";

const args = process.argv.slice(2);
process.chdir(resolve(import.meta.dirname, ".."));
const label = args[0];
assert(label && /^[a-z0-9-]+$/.test(label), "usage: node scripts/process-sources.mjs <label> [--samples 60] [--interval 1000]");
const option = (key, fallback) => args.includes(key) ? args[args.indexOf(key) + 1] : fallback;
const output = resolve("target/process-source-validation", label);
const previous = args.includes("--report-only") ? JSON.parse(readFileSync(resolve(output, "summary.json"), "utf8")) : null;
const samples = Number(option("--samples", previous?.block_samples ?? 60));
const interval = Number(option("--interval", previous?.interval_ms ?? 1000));
assert(Number.isInteger(samples) && samples >= 5 && samples <= 300);
assert(Number.isInteger(interval) && interval >= 250 && interval <= 5000);
const executable = resolve("target/release/examples/process_sources.exe");
mkdirSync(output, { recursive: true });
const rows = [];

async function run(mode, index, count = samples) {
  const values = [];
  const errors = [];
  const child = spawn(executable, [mode, String(count), String(interval)], { windowsHide: true, stdio: ["ignore", "pipe", "pipe"] });
  const timeoutMs = (count + 3) * interval + 45000;
  let timedOut = false;
  const timer = setTimeout(() => { timedOut = true; child.kill(); }, timeoutMs);
  const lines = createInterface({ input: child.stdout });
  lines.on("line", line => {
    try { values.push(JSON.parse(line)); } catch { errors.push("invalid JSON output"); }
  });
  child.stderr.on("data", bytes => errors.push(bytes.toString("utf8")));
  const code = await new Promise((done, reject) => { child.once("error", reject); child.once("close", done); }).finally(() => clearTimeout(timer));
  lines.close();
  const file = `${String(index).padStart(2, "0")}-${mode}.jsonl`;
  writeFileSync(resolve(output, file), values.map(value => JSON.stringify(value)).join("\n") + "\n");
  if (errors.length) writeFileSync(resolve(output, file + ".stderr.txt"), errors.join("\n"));
  rows.push({ mode, code, timedOut, file, samples: values });
  writeReport();
  const measured = values.filter(value => (value.event === "sample" && !value.warmup) || value.event === "comparison");
  console.log(JSON.stringify({ event: "block", index, mode, code, timedOut, samples: measured.length, max_ms: Math.max(0, ...measured.map(v => v.wall_ms ?? v.values.bracket_ms)) }));
  assert(code === 0 && !timedOut, `block ${index} failed; see ${output}`);
}

function statistics(values) {
  if (!values.length) return null;
  const sorted = values.slice().sort((a, b) => a - b);
  const p = q => sorted[Math.min(sorted.length - 1, Math.max(0, Math.ceil(q * sorted.length) - 1))];
  return { n: sorted.length, p50: p(.5), p95: p(.95), p99: p(.99), max: p(1), mean: sorted.reduce((a,b)=>a+b,0) / sorted.length };
}

function writeReport() {
  const results = ["full", "basic", "hybrid"].map(mode => {
    const all = rows.filter(row => row.mode === mode).flatMap(row => row.samples).filter(row => row.event === "sample");
    const values = all.filter(row => !row.warmup);
    const stats = key => statistics(values.map(row => row[key]));
    const coverage = Object.fromEntries(["cpu", "private_ws", "working_set", "commit", "io", "handles", "threads"].map(key => [key, statistics(values.map(v => 100 * v.coverage[key] / Math.max(1, v.processes)))]));
    return { mode, wall_ms: stats("wall_ms"), process_cpu_ms: stats("process_cpu_ms"), thread_cpu_ms: stats("thread_cpu_ms"), kernel_ms: stats("thread_kernel_ms"), system_busy_pct: stats("system_busy_pct"), coverage_pct: coverage,
      cold_max_ms: Math.max(0, ...all.filter(row=>row.warmup).map(row=>row.wall_ms)),
      private_bytes: statistics(values.map(v => v.own.private_bytes)), handles: statistics(values.map(v => v.own.handles)),
      processes: stats("processes"), slow_queries: values.filter(v => v.wall_ms > 100).map(v => ({wall_ms:v.wall_ms,native_ms:v.native_ms,thread_cpu_ms:v.thread_cpu_ms,thread_kernel_ms:v.thread_kernel_ms})),
      errors: values.at(-1)?.errors,
    };
  });
  const comparisons = rows.filter(row=>row.mode === "compare").flatMap(row=>row.samples).filter(row=>row.event === "comparison");
  const total = key => comparisons.reduce((sum,row)=>sum + (row.values[key] ?? 0),0);
  const memory = ["private_working_set", "working_set", "commit"].map((metric,index)=>{
    const stable = comparisons.reduce((sum,row)=>sum + row.values.memory_stable[index],0);
    const equal = comparisons.reduce((sum,row)=>sum + row.values.memory_equal[index],0);
    return {metric,stable,equal,equal_pct:stable ? equal / stable * 100 : null};
  });
  const validation = {rounds:comparisons.length,paired:total("paired"),cpu_outside_bracket:total("cpu_outside_bracket"),io_outside_bracket:total("io_outside_bracket"),identity_mismatches:total("identity_mismatches"),
    identity_rounds:comparisons.filter(row=>row.values.stable_identities !== undefined).length,stable_identities:total("stable_identities"),listed_stable_identities:total("listed_stable_identities"),basic_parent_mismatches:total("basic_parent_mismatches"),memory};
  const report = { interval_ms:interval, block_samples:samples, order:rows.map(({mode,file,code,timedOut})=>({mode,file,code,timedOut})), environment: rows[0]?.samples.find(row=>row.event === "environment"), results, validation, comparisons };
  writeFileSync(resolve(output, "summary.json"), JSON.stringify(report, null, 2));
  const escaped = value => String(value).replace(/[&<>"']/g, c=>({"&":"&amp;","<":"&lt;",">":"&gt;",'"':"&quot;","'":"&#39;"})[c]);
  const f = value => Number.isFinite(value) ? value.toFixed(3) : "—";
  const pct = value => Number.isFinite(value) ? value.toFixed(1) + "%" : "—";
  const table = results.map(row=>`<tr><td>${escaped(row.mode)}</td><td>${row.wall_ms?.n ?? 0}</td><td>${f(row.wall_ms?.p50)}</td><td>${f(row.wall_ms?.p95)}</td><td>${f(row.wall_ms?.p99)}</td><td>${f(row.wall_ms?.max)}</td><td>${f(row.process_cpu_ms?.mean)}</td><td>${pct(row.coverage_pct.cpu?.mean)}</td><td>${pct(row.coverage_pct.private_ws?.mean)}</td></tr>`).join("");
  writeFileSync(resolve(output, "report.html"), `<!doctype html><html lang="zh-CN"><meta charset="utf-8"><title>进程采集来源对照</title>
<style>body{font:15px system-ui;max-width:1100px;margin:40px auto;padding:0 24px;color:#202124}table{border-collapse:collapse;width:100%;font-variant-numeric:tabular-nums}th,td{border-bottom:1px solid #ddd;text-align:right;padding:10px}th:first-child,td:first-child{text-align:left}pre{white-space:pre-wrap;background:#f5f6f8;padding:18px;border-radius:8px}p{line-height:1.7}</style>
<h1>进程采集来源对照</h1><p>普通权限，发行构建；每 ${interval} 毫秒采样。顺序为 full → basic → hybrid → hybrid → basic → full，每段排除 3 次预热。basic 仅枚举身份，其耗时用于比较名单枚举成本。hybrid 读取 CPU 时间、私有工作集、工作集、提交、通用 I/O 和句柄数；仅磁盘字节、周期计数、线程数及暂停状态仍需其他来源。</p>
<table><thead><tr><th>来源</th><th>样本</th><th>P50 ms</th><th>P95 ms</th><th>P99 ms</th><th>最大 ms</th><th>自身 CPU ms/周期</th><th>CPU 时间覆盖</th><th>私有工作集覆盖</th></tr></thead><tbody>${table}</tbody></table>
<p>数值校验采用完整快照在前后夹取。CPU 校验针对累计内核与用户时间；内存动态值可能在区间内变化，计数越界与相对差异分别记录。采集程序自身从数值校验中排除。进程 CPU 时间按完整采样周期累计，包含输出与测量开销。</p>
<h2>数值校验汇总</h2><pre>${escaped(JSON.stringify(validation,null,2))}</pre><h2>环境与完整数据</h2><pre>${escaped(JSON.stringify(report,null,2))}</pre></html>`);
}

if (previous) {
  for (const file of readdirSync(output).sort()) {
    const match = file.match(/^\d+-(full|basic|hybrid|compare)\.jsonl$/);
    if (!match) continue;
    const saved = previous.order.find(row => row.file === file);
    const values = readFileSync(resolve(output, file), "utf8").trim().split(/\r?\n/).filter(Boolean).map(line => JSON.parse(line));
    rows.push({ mode: match[1], code: saved?.code ?? 0, timedOut: saved?.timedOut ?? false, file, samples: values });
  }
  writeReport();
} else {
  for (const [index, mode] of ["full", "basic", "hybrid", "hybrid", "basic", "full"].entries()) await run(mode,index+1);
  await run("compare",7,Math.min(30,samples));
}
console.log(`Report: ${resolve(output,"report.html")}`);
