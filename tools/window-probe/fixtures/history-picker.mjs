// Writes ../steps/history-picker.json: the picker opened from the range pill
// over made-up hours (62 days) and events, compact and with the overview. The
// hours and events are counted back from the fixture sample's time, so the
// steps are generated; edit them here.
//
//   node tools/window-probe/fixtures/history-picker.mjs           rewrite the steps
//   node tools/window-probe/fixtures/history-picker.mjs --check   fail when they are out of date
import fs from 'node:fs';
const fixture = JSON.parse(fs.readFileSync(new URL('desktop.json', import.meta.url), 'utf8'));
const NOW = fixture.snapshot.timestamp;
const HOUR = 3600e3, DAY = 24 * HOUR, MIN = 60e3;
const TZ = 8 * HOUR; // the steps run on a UTC+8 machine; hours still map by timestamp
const midnight = (t) => Math.floor((t + TZ) / DAY) * DAY - TZ;
const from = midnight(NOW) - 61 * DAY;
let seed = 7;
const rand = () => ((seed = (seed * 1103515245 + 12345) % 2147483648) / 2147483648);
const ts = [], span = [], frame = [], peak = [];
const cpu = [], gpu = [], memUsed = [], memTotal = [], temp = [], net = [];
for (let t = from; t < NOW; t += HOUR) {
  const day = Math.floor((t - from) / DAY);
  const hour = Math.floor(((t + TZ) % DAY) / HOUR);
  // Off at night most days, a week away in the middle, and the first days unrecorded.
  const asleep = (hour < 8 && day % 3) || (day >= 20 && day < 26) || day < 4;
  if (asleep) continue;
  const busy = hour >= 9 && hour <= 23 ? 1 : 0.4;
  ts.push(t); span.push(HOUR); frame.push(t + 5000); peak.push(t + 5000);
  cpu.push(+(4 + busy * 18 * rand() + (hour === 21 ? 30 : 0)).toFixed(1));
  gpu.push(+(busy * 30 * rand()).toFixed(1));
  memTotal.push(32 * 2 ** 30);
  memUsed.push(Math.round((12 + 10 * busy * rand()) * 2 ** 30));
  temp.push(+(45 + busy * 25 * rand()).toFixed(1));
  net.push(Math.round(busy * 3e6 * rand()));
}
const col = (mean) => ({ mean, low: mean, high: mean });
const historyDays = {
  bucket: HOUR, ts, span, frame, peak, gaps: [], settled: NOW, mergedDays: [],
  series: { cpu: col(cpu), gpu: col(gpu), memUsed: col(memUsed), memTotal: col(memTotal), cpuTemp: col(temp), netReceive: col(net) },
};
const ago = (h, m = 0) => NOW - h * HOUR - m * MIN;
const historyEvents = {
  truncated: false,
  events: [
    { kind: 'cpu', from: ago(1, 30), to: ago(1, 18), peak: 98.5, peakAt: ago(1, 24), top: { name: 'rustc.exe', share: 0.62, mean: 41.3 } },
    { kind: 'temp', from: ago(1, 28), to: ago(1, 24), peak: 92, peakAt: ago(1, 26), top: { name: 'rustc.exe', share: 0.58, mean: 39.1 } },
    { kind: 'net', from: ago(3, 10), to: ago(3, 2), peak: 48e6, peakAt: ago(3, 5), top: { name: 'chrome.exe', share: 0.91, mean: 21e6 } },
    { kind: 'gap', reason: 'sleep', from: ago(9), to: ago(7, 30) },
    { kind: 'mem', from: ago(26), to: ago(25, 54), peak: 91, peakAt: ago(25, 57), top: { name: 'Code.exe', share: 0.34, mean: 3.1e9 } },
    { kind: 'gpu', from: ago(30), to: ago(29, 20), peak: 97, peakAt: ago(29, 40), top: { name: 'Steam.exe', share: 0.8, mean: 71 } },
    { kind: 'disk', from: ago(50), to: ago(49, 50), peak: 410e6, peakAt: ago(49, 55), top: { name: 'MsMpEng.exe', share: 0.7, mean: 180e6 } },
  ],
};
// Many events in the day shown: the list scrolls inside its column.
const kinds = ['cpu', 'net', 'disk', 'temp', 'gpu', 'mem'];
const manyEvents = {
  truncated: false,
  events: Array.from({ length: 30 }, (_, i) => ({
    kind: kinds[i % kinds.length],
    from: ago(0, 20 + i * 20),
    to: ago(0, 10 + i * 20),
    peak: [99, 88e6, 350e6, 91, 95, 90][i % 6],
    peakAt: ago(0, 15 + i * 20),
    top: { name: ['rustc.exe', 'System', 'bili-media.exe', 'rustc.exe', 'Steam.exe', 'Code.exe'][i % 6], share: 0.5, mean: 10 },
  })),
};
const steps = {
  about: '历史页的时间范围：快捷范围的下拉列表与键盘选择、自定义范围对话框，以及带 30 天热力图和事件列表的概览。62 天的逐小时数据与事件按模拟快照的时间推算，由 fixtures/history-picker.mjs 生成，不要手改本文件。',
  window: [1280, 900],
  steps: [
    // ui-shots waits for the instance to settle after each step; only the
    // theme switch waits for the system backdrop's transition.
    { control: { page: 'history', pointerOutside: true, preference: { 'history.overview': false } } },
    { control: { data: { historyDays, historyEvents } } },
    { control: { clickAnchor: 'history-range' }, inspect: 'layout', label: 'range-menu', capture: 'range-menu', expect: { 'history.ranges': 0 } },
    { crop: { from: 'range-menu', name: 'range-menu-crop', region: [0, 380, 260, 260], scale: 2 } },
    { control: { keys: ['down', 'down', 'down', 'down'] }, inspect: 'layout', label: 'range-keys', capture: 'range-keys', expect: { 'history.ranges': 4 } },
    { control: { keys: ['enter'] }, inspect: 'layout', label: 'picker', capture: 'picker',
      expect: { 'history.picker': true, 'history.ranges': null } },
    { control: { keys: ['escape'] }, inspect: 'layout', label: 'closed', expect: { 'history.picker': false } },
    { control: { preference: { 'history.overview': true } } },
    { control: { clickAnchor: 'history-range' } },
    { control: { keys: ['end', 'enter'] }, capture: 'overview' },
    { control: { data: { historyEvents: manyEvents } }, capture: 'overview-many' },
    { control: { theme: 'light' }, wait: 700, capture: 'overview-light' },
  ],
};
const file = new URL('../steps/history-picker.json', import.meta.url);
const text = JSON.stringify(steps) + '\n';
const current = fs.existsSync(file) ? fs.readFileSync(file, 'utf8') : '';
if (process.argv.includes('--check')) {
  if (current !== text) {
    console.error('history-picker.json differs from what history-picker.mjs writes');
    process.exit(1);
  }
  console.log('history-picker.json is up to date');
} else if (current !== text) {
  fs.writeFileSync(file, text);
  console.log(`history-picker.json: rewrote ${ts.length} hours`);
} else {
  console.log('history-picker.json: nothing to change');
}
