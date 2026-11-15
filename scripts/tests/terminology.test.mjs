import assert from "node:assert/strict";
import test from "node:test";
import { textErrors } from "../lib/locale-validation.mjs";

test("resource labels keep the canonical abbreviation even if the source changes", () => {
  for (const [key, term, translated] of [
    ["common.resource.cpu", "CPU", "Процессор"],
    ["common.resource.gpu", "GPU", "Графический процессор"],
    ["processes.column.io.label", "I/O", "Ввод-вывод"],
    ["processes.column.pid.label", "PID", "Process ID"],
  ]) {
    assert.deepEqual(textErrors(term, term, { key }), []);
    assert.ok(textErrors(translated, translated, { key }).some(error => error.includes(`exactly ${term}`)));
  }
});

test("localized abbreviations cannot return in source text, prose or translated-only GPU labels", () => {
  for (const [source, text] of [
    ["CPU", "ЦП"], ["CPU", "ЦПУ"], ["核显", "Встроенный ГП"], ["GPU", "ГПУ"],
    ["I/O read", "Чтение (В/В)"], ["I/O read", "Lectura de E/S"], ["I/O read", "E/A lesen"],
    ["I/O read", "Lecture d’E/S"], ["E/S", "E/S"],
  ]) assert.ok(textErrors(source, text).some(error => error.startsWith("use ")), text);
  assert.ok(textErrors("I/O", "В/В", { key: "processes.column.io.title" }).some(error => error.startsWith("use ")));
});

test("full natural phrases are allowed only in their documented app context", () => {
  assert.deepEqual(textErrors("CPU 时间", "Temps processeur", { key: "details.cpuTime" }), []);
  assert.deepEqual(textErrors("I/O 操作", "Операции ввода-вывода", { key: "processes.column.io.title" }), []);
  assert.ok(textErrors("CPU 图表", "График процессора", { key: "performance.cpu.graph" }).length);
  assert.ok(textErrors("Separate read, write and I/O rates per process", "Отдельные скорости чтения, записи и ввода-вывода по процессам").length);
});

test("technical tokens support CJK adjacency and compounds without accepting longer identifiers", () => {
  assert.deepEqual(textErrors("CPU图表、GPU、I/O", "CPUグラフ・GPU・I/O"), []);
  assert.deepEqual(textErrors("CPU / I/O", "CPU-Auslastung / I/O-Raten"), []);
  assert.deepEqual(textErrors("CPUID", "CPUID"), []);
  assert.ok(textErrors("CPU", "CPUID").some(error => error.endsWith(": CPU")));
  assert.ok(textErrors("GPU", "vGPU").some(error => error.endsWith(": GPU")));
  assert.deepEqual(textErrors("{CPU} {GB}", "{CPU} {GB}"), []);
  for (const term of ["PID", "NVMe", "SMART", "RAPL", "CPUID", "PawnIO", "FCLK"]) {
    assert.ok(textErrors(term, term.toLowerCase()).some(error => error.endsWith(`: ${term}`)));
  }
});

test("unit symbols keep spelling, case and rate suffixes across languages", () => {
  for (const [source, text] of [
    ["{value} GB", "{value} ГБ"], ["1 MB", "1 МБ"], ["8 KiB", "8 КиБ"],
    ["8 KiB", "8 KB"], ["{count} Hz", "{count} Гц"], ["100 MB/s", "100 Mbps"],
    ["12V", "12 В"], ["{value} W", "{value} Вт"], ["5 A", "5 Ампер"],
  ]) assert.ok(textErrors(source, text).some(error => error.startsWith("unit changed")), text);
  assert.deepEqual(textErrors("+3.3 V", "+3,3 V в режиме ожидания"), []);
  assert.deepEqual(textErrors("{value} GB", "{value}\u00a0GB"), []);
  assert.deepEqual(textErrors("{value} W", "{value}<u>W</u>"), []);
  assert.deepEqual(textErrors("16 GB / 100 MB/s", "16\u00a0GB / 100\u00a0MB/s"), []);
  assert.deepEqual(textErrors("A native window", "原生窗口"), []);
  assert.deepEqual(textErrors("{V}", "{V}"), []);
});
