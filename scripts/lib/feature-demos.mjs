// The feature demos in the READMEs: short animated crops of the app, one
// standalone SVG per demo, README language and theme. GitHub serves
// repository SVG files with "default-src 'none'; style-src 'unsafe-inline';
// sandbox", so each file draws everything itself and moves only through CSS
// keyframes inside it: no script, no filter, no external font or image.
//
// The interface text comes from locales/ (scripts/readme-features.mjs passes
// the strings in); the demo data (process names, ports, addresses from the
// documentation ranges) is invented. Each element's attributes hold the
// demo's resting frame, the one that explains the feature best, shown when
// the viewer prefers reduced motion.
//
// No DOM here: scripts/readme-features.mjs runs this module in a headless
// page, where `measure` comes from a canvas. Tests run it in Node.

export const W = 440;
export const H = 260;

const SC = "'PingFang SC','Microsoft YaHei UI','Microsoft YaHei','Noto Sans CJK SC','Noto Sans SC'";
const TC = "'PingFang TC','Microsoft JhengHei UI','Microsoft JhengHei','Noto Sans CJK TC','Noto Sans TC'";
const JA = "'Hiragino Sans','Yu Gothic UI','Meiryo UI','Meiryo','Noto Sans CJK JP','Noto Sans JP'";

// One entry per README. `comma`: decimal comma, as Windows formats figures in
// that region. `explorer` is how Task Manager names explorer.exe there;
// `report` names the demo's spreadsheet.
export const languages = [
  { lang: "zh-CN", locale: "zh-CN", cjk: SC, comma: false, explorer: "Windows 资源管理器", report: "季度报告" },
  { lang: "zh-TW", locale: "zh-TW", cjk: TC, comma: false, explorer: "Windows 檔案總管", report: "季度報告" },
  { lang: "en", locale: "en-US", cjk: SC, comma: false, explorer: "Windows Explorer", report: "Quarterly report" },
  { lang: "es", locale: "es-ES", cjk: SC, comma: true, explorer: "Explorador de Windows", report: "Informe trimestral" },
  { lang: "de", locale: "de-DE", cjk: SC, comma: true, explorer: "Windows-Explorer", report: "Quartalsbericht" },
  { lang: "ja", locale: "ja-JP", cjk: JA, comma: false, explorer: "エクスプローラー", report: "四半期レポート" },
  { lang: "fr", locale: "fr-FR", cjk: SC, comma: true, explorer: "Explorateur Windows", report: "Rapport trimestriel" },
  { lang: "ru", locale: "ru-RU", cjk: SC, comma: true, explorer: "Проводник", report: "Квартальный отчёт" },
];
export const themes = ["light", "dark"];
export const demos = ["processes", "performance", "history", "files", "rules", "ports"];
export const demoPath = (id, lang, theme) => `docs/images/features/${id}-${lang}-${theme}.svg`;

// Every locale string the demos use, as "file:key.path".
export const KEYS = [
  ...["processes", "performance", "services", "startup", "network", "history"].map((k) => `shell:page.${k}`),
  "shell:viewMode.apps.label",
  "shell:viewMode.tree.label",
  "shell:viewMode.list.label",
  "shell:search.processes",
  "processes:section.apps",
  "processes:section.background",
  "processes:column.name.label",
  "processes:column.cpu.label",
  "processes:column.mem.label",
  "processes:menu.setPriority",
  "processes:menu.addRule",
  ...["realtime", "high", "aboveNormal", "normal", "belowNormal", "idle"].map((k) => `processes:priority.${k}`),
  "common:endProcess",
  "common:endProcessTree",
  "common:efficiencyMode",
  "common:openFileLocation",
  "common:resource.memory",
  "common:resource.temperature",
  "common:status",
  "actions:priority.done",
  "actions:efficiency.on.done",
  "fileLock:title",
  "fileLock:checking",
  "fileLock:found",
  "fileLock:handleCount",
  "fileLock:kind.explorer",
  "fileLock:end",
  "history:range.hours",
  "history:range.days",
  "history:busiest",
  "history:busiestAt",
  "history:hint.pick",
  "format:monthDay",
  "format:monthNames",
  "connections:search",
  ...["all", "listen", "established", "udp"].map((k) => `connections:filter.${k}.label`),
  ...["process", "localPort", "remoteAddress", "remotePort"].map((k) => `connections:column.${k}`),
  "connections:state.listen",
  "connections:state.established",
  ...["close", "end", "copyLocal", "copyRemote"].map((k) => `connections:menu.${k}`),
  "performance:cpu.utilization",
  "performance:cpu.speed",
  "performance:cpu.power",
  "performance:cpu.coreKind.p",
  "performance:cpu.coreKind.e",
  "performance:disk.titleVolumes",
];

// The strings in KEYS for one locale. `read(file)` returns that locale's
// locales/<locale>/<file>.json, parsed.
export function pickStrings(read) {
  const files = new Map();
  const strings = {};
  for (const key of KEYS) {
    const [file, path] = key.split(":");
    if (!files.has(file)) files.set(file, read(file));
    let value = files.get(file);
    for (const part of path.split(".")) value = value?.[part];
    if (value == null) throw Error(`${file}.json has no ${path}`);
    strings[key] = value;
  }
  return strings;
}

const esc = (s) => String(s).replace(/[&<>"]/g, (c) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;" })[c]);
const n = (v) => Math.round(v * 100) / 100;
const rgba = (v) => v.replace(/rgb\((\d+) (\d+) (\d+) \/ ([\d.]+)\)/, "rgba($1,$2,$3,$4)");

// Colours from resources/theme.json, flattened to solid fills where the app
// relies on the backdrop (flyouts, the window).
function tokens(palette, mode) {
  const t = palette[mode];
  const dark = mode === "dark";
  const heat = t["heat-rgb"].split(" ").join(",");
  return {
    dark,
    win: dark ? "#202020" : "#f3f3f3",
    layer: rgba(t.layer),
    layerStrong: rgba(t["layer-strong"]),
    hoverStrong: rgba(t["hover-strong"]),
    stroke: rgba(t.stroke),
    strokeStrong: rgba(t["stroke-strong"]),
    control: rgba(t.control),
    checked: rgba(t["control-checked"]),
    flyout: dark ? "#2a2b31" : "#fbfbfc",
    scrim: rgba(t.scrim),
    ink: t.ink,
    ink2: t["ink-2"],
    ink3: t["ink-3"],
    accent: t.accent,
    danger: t.danger,
    cpu: t.cpu,
    cpuE: t["cpu-e"],
    mem: t.mem,
    io: t.io,
    net: t.net,
    gpu: t.gpu,
    heat: (k) => `rgba(${heat},${n(palette.heat.opacity * k)})`,
    shadow: dark ? "rgba(0,0,0,0.35)" : "rgba(0,0,0,0.10)",
  };
}

const EASE = {
  out: "cubic-bezier(0.1,0.9,0.2,1)",
  move: "cubic-bezier(0.45,0,0.2,1)",
  lin: "linear",
};

// One looping timeline per image. cls() takes keys [time, {o, x, y, s, sx},
// ease] (the ease applies to the segment starting at that key) and returns a
// class name. Values carry forward between keys and back to the start.
// `phase` starts the loop part-way, so demos shown side by side move out of
// step.
class Motion {
  constructor(cycle, phase) {
    this.cycle = cycle;
    this.phase = phase;
    this.css = [];
  }
  cls(keys, origin) {
    const name = `a${this.css.length}`;
    const props = [...new Set(keys.flatMap((k) => Object.keys(k[1])))];
    const first = {};
    for (const p of props) first[p] = keys.find((k) => p in k[1])[1][p];
    let prev = first;
    const full = keys.map(([t, v, e]) => {
      prev = { ...prev, ...v };
      return [t, prev, e];
    });
    full.sort((a, b) => a[0] - b[0]);
    if (full[0][0] > 0) full.unshift([0, full[0][1]]);
    if (full.at(-1)[0] < this.cycle) full.push([this.cycle, full.at(-1)[1]]);
    const moves = props.some((p) => p !== "o");
    const frames = full.map(([t, v, e]) => {
      const d = [];
      if ("o" in v) d.push(`opacity:${v.o}`);
      if (moves) d.push(`transform:translate(${v.x ?? 0}px,${v.y ?? 0}px)${v.s != null ? ` scale(${v.s})` : ""}${v.sx != null ? ` scaleX(${v.sx})` : ""}`);
      if (e) d.push(`animation-timing-function:${EASE[e]}`);
      return `${+((t / this.cycle) * 100).toFixed(3)}%{${d.join(";")}}`;
    });
    const box = props.includes("s") || props.includes("sx") ? `;transform-box:fill-box;transform-origin:${origin ?? "center"}` : "";
    const delay = this.phase ? ` ${-this.phase}s` : "";
    this.css.push(`@keyframes ${name}{${frames.join("")}}.${name}{animation:${name} ${this.cycle}s linear${delay} infinite${box}}`);
    return name;
  }
  // Visible during each [a, b] window, fading over f seconds.
  shown(spans, f = 0.15) {
    return this.cls(
      spans.flatMap(([a, b]) => [
        [a - f, { o: 0 }, "out"],
        [a, { o: 1 }],
        [b, { o: 1 }, "out"],
        [b + f, { o: 0 }],
      ]),
    );
  }
  // An endless scroll with its own period.
  loop(period, frames) {
    const name = `a${this.css.length}`;
    this.css.push(`@keyframes ${name}{${frames}}.${name}{animation:${name} ${period}s linear infinite}`);
    return name;
  }
}

// --- Drawing helpers ---------------------------------------------------------

function text(x, y, s, { size = 12, fill, weight, anchor, opacity, cls } = {}) {
  const a = [`x="${n(x)}"`, `y="${n(y)}"`, `font-size="${size}"`, `fill="${fill}"`];
  if (weight) a.push(`font-weight="${weight}"`);
  if (anchor) a.push(`text-anchor="${anchor}"`);
  if (opacity != null) a.push(`opacity="${opacity}"`);
  if (cls) a.push(`class="${cls}"`);
  return `<text ${a.join(" ")}>${esc(s)}</text>`;
}

function rect(x, y, w, h, { rx = 0, fill = "none", stroke, sw = 1, opacity, cls, dash } = {}) {
  const a = [`x="${n(x)}"`, `y="${n(y)}"`, `width="${n(w)}"`, `height="${n(h)}"`, `fill="${fill}"`];
  if (rx) a.push(`rx="${rx}"`);
  if (stroke) a.push(`stroke="${stroke}"`, `stroke-width="${sw}"`);
  if (dash) a.push(`stroke-dasharray="${dash}"`);
  if (opacity != null) a.push(`opacity="${opacity}"`);
  if (cls) a.push(`class="${cls}"`);
  return `<rect ${a.join(" ")}/>`;
}

const group = (cls, body, { rest = 1, x, y } = {}) => {
  const a = [];
  if (cls) a.push(`class="${cls}"`);
  if (rest !== 1) a.push(`opacity="${rest}"`);
  if (x != null || y != null) a.push(`transform="translate(${n(x ?? 0)} ${n(y ?? 0)})"`);
  return `<g ${a.join(" ")}>${body}</g>`;
};

// 16 px process icons from plain shapes in a product colour; no third-party
// logos.
function glyph(kind, x, y, t) {
  const at = `transform="translate(${n(x)} ${n(y)})"`;
  const box = (fill, inner) => `<g ${at}><rect width="16" height="16" rx="3.5" fill="${fill}"/>${inner}</g>`;
  const letter = (fill, ch) => box(fill, `<text x="8" y="12" font-size="10" font-weight="700" fill="#fff" text-anchor="middle">${ch}</text>`);
  const line = t.dark ? "#9aa0ad" : "#8e92a0";
  switch (kind) {
    case "folder":
      return `<g ${at}><path d="M1 4.5a1.5 1.5 0 0 1 1.5-1.5h3.8l1.6 1.6h5.6A1.5 1.5 0 0 1 15 6.1v6.4a1.5 1.5 0 0 1-1.5 1.5h-11A1.5 1.5 0 0 1 1 12.5z" fill="#e8b33a"/><path d="M1 6.5h14v6a1.5 1.5 0 0 1-1.5 1.5h-11A1.5 1.5 0 0 1 1 12.5z" fill="#f6cd5b"/></g>`;
    case "sheet":
      return box("#1f8f4e", `<path d="M4 5h8M4 8h8M4 11h8M8 4v8" stroke="#fff" stroke-width="1.1" opacity=".9"/>`);
    case "doc":
      return `<g ${at}><path d="M3 1.5h6.5L13 5v9.5H3z" fill="${t.dark ? "#e9ecf2" : "#ffffff"}" stroke="${line}"/><path d="M9.5 1.5V5H13" fill="none" stroke="${line}"/><rect x="5" y="7.5" width="6" height="5" rx=".8" fill="#1f8f4e"/></g>`;
    case "code":
      return box("#2f7fd8", `<path d="M6.3 5.2 3.6 8l2.7 2.8M9.7 5.2 12.4 8 9.7 10.8" fill="none" stroke="#fff" stroke-width="1.4" stroke-linecap="round" stroke-linejoin="round"/>`);
    case "globe":
      return `<g ${at}><circle cx="8" cy="8" r="7" fill="#1c9a8f"/><ellipse cx="8" cy="8" rx="3" ry="7" fill="none" stroke="#fff" opacity=".85"/><path d="M1.5 8h13" stroke="#fff" opacity=".85"/></g>`;
    case "film":
      return box("#d9682f", `<path d="M6.3 4.8v6.4L11.2 8z" fill="#fff"/>`);
    case "cloud":
      return box("#2a6fd6", `<path d="M5 11.5h6.3a2.2 2.2 0 0 0 .2-4.4 3 3 0 0 0-5.7-.6A2.5 2.5 0 0 0 5 11.5z" fill="#fff"/>`);
    case "node":
      return `<g ${at}><path d="M8 .8 14.3 4.4v7.2L8 15.2 1.7 11.6V4.4z" fill="#3c873a"/><text x="8" y="11.2" font-size="8" font-weight="700" fill="#fff" text-anchor="middle">n</text></g>`;
    case "svc":
      return box(t.dark ? "#4b5563" : "#6b7280", `<rect x="3.5" y="4" width="9" height="3" rx="1" fill="#fff" opacity=".9"/><rect x="3.5" y="9" width="9" height="3" rx="1" fill="#fff" opacity=".6"/>`);
    case "java":
      return letter("#c9473d", "J");
    case "curl":
      return letter("#245fb8", "c");
    case "cargo":
      return box(t.dark ? "#3c404c" : "#4a4f5c", `<path d="M4 5.5 6.5 8 4 10.5M8 11h4" fill="none" stroke="#fff" stroke-width="1.3" stroke-linecap="round" stroke-linejoin="round"/>`);
    default:
      return letter("#64748b", "?");
  }
}

const leaf = (x, y, color) =>
  `<path transform="translate(${n(x)} ${n(y)})" d="M2 12C2 6 6 2 13 2c0 7-4 11-10 11M2 12l6-6" fill="none" stroke="${color}" stroke-width="1.4" stroke-linecap="round" stroke-linejoin="round"/>`;
const tick = (x, y, color, w = 1.5) =>
  `<path transform="translate(${n(x)} ${n(y)})" d="M1 5.5 4 8.5 10 1.5" fill="none" stroke="${color}" stroke-width="${w}" stroke-linecap="round" stroke-linejoin="round"/>`;
const chevron = (x, y, color, down) =>
  `<path transform="translate(${n(x)} ${n(y)})" d="${down ? "M1 3l4 4 4-4" : "M3 1l4 4-4 4"}" fill="none" stroke="${color}" stroke-width="1.3" stroke-linecap="round" stroke-linejoin="round"/>`;
const magnifier = (x, y, color) =>
  `<g transform="translate(${n(x)} ${n(y)})" fill="none" stroke="${color}" stroke-width="1.4" stroke-linecap="round"><circle cx="6" cy="6" r="4.6"/><path d="m9.5 9.5 3.5 3.5"/></g>`;
const POINTER = `<path d="M0 0v17.2l4.3-3.9 2.7 6.3 2.6-1.1-2.7-6.2h5.8z" fill="#fff" stroke="#111" stroke-width="1.1" stroke-linejoin="round"/>`;

// The pointer along stops [[t, x, y]]; shown from the first stop to `out`.
function pointer(m, stops, out) {
  const move = m.cls(stops.map(([t, x, y]) => [t, { x, y }, "move"]));
  const shown = m.shown([[stops[0][0], out]], 0.2);
  const [, x, y] = stops.at(-1);
  return `<g class="${shown}" opacity="0"><g class="${move}" transform="translate(${n(x)} ${n(y)})">${POINTER}</g></g>`;
}

// A click ring at (x, y) at time t.
function ripple(m, t, x, y, color) {
  const c = m.cls(
    [
      [t, { o: 0, s: 0.3 }],
      [t + 0.01, { o: 0.55, s: 0.3 }, "out"],
      [t + 0.4, { o: 0, s: 1 }],
    ],
    "center",
  );
  return `<g transform="translate(${n(x)} ${n(y)})"><circle class="${c}" r="9" fill="none" stroke="${color}" stroke-width="1.5" opacity="0"/></g>`;
}

// Deterministic noise for the curves.
function seeded(seed) {
  let a = seed >>> 0;
  return () => {
    a = (a + 0x6d2b79f5) >>> 0;
    let x = a;
    x = Math.imul(x ^ (x >>> 15), x | 1);
    x ^= x + Math.imul(x ^ (x >>> 7), x | 61);
    return ((x ^ (x >>> 14)) >>> 0) / 4294967296;
  };
}

// A periodic series: sines whose periods divide the length, so a chart can
// scroll it forever without a seam.
function periodic(len, seed, base, amp) {
  const rand = seeded(seed);
  const waves = [1, 2, 3, 5].map((f, k) => ({ f, ph: rand() * Math.PI * 2, a: amp * [0.5, 0.3, 0.25, 0.18][k] }));
  return Array.from({ length: len }, (_, i) => base + waves.reduce((s, w) => s + w.a * Math.sin((i / len) * Math.PI * 2 * w.f + w.ph), 0));
}

// A chart that scrolls left forever: the period drawn twice, moved by one
// width per loop, clipped by a nested viewport.
function scroller(m, x, y, w, h, vals, color, period) {
  const N = vals.length;
  const step = w / N;
  const d = [...vals, ...vals, vals[0]].map((v, i) => `${i ? "L" : "M"}${n(i * step)} ${n(h - (Math.max(0, Math.min(100, v)) / 100) * (h - 2))}`).join("");
  const cls = m.loop(period, `from{transform:translateX(0)}to{transform:translateX(${-n(w)}px)}`);
  return `<svg x="${n(x)}" y="${n(y)}" width="${n(w)}" height="${n(h)}" overflow="hidden"><g class="${cls}"><path d="${d}L${n(2 * w + step)} ${h}L0 ${h}Z" fill="${color}" fill-opacity="0.16"/><path d="${d}" fill="none" stroke="${color}" stroke-width="1.3" stroke-linejoin="round"/></g></svg>`;
}

// --- The context every scene draws with ---------------------------------------

function context({ lang, theme, palette, strings, measure }) {
  const L = languages.find((l) => l.lang === lang);
  if (!L) throw Error(`unknown README language ${lang}`);
  const family = `'Segoe UI Variable Text','Segoe UI',${L.cjk},system-ui,-apple-system,BlinkMacSystemFont,sans-serif`;
  const w = (s, size, weight = 400) => measure(String(s), `${weight} ${size}px ${family}`);
  const plural = new Intl.PluralRules(L.locale);
  const tr = (key, vars = {}) => {
    let v = strings[key];
    if (v == null) throw Error(`${lang}: missing locale string ${key}`);
    if (typeof v === "object") v = v[plural.select(vars.count ?? 0)] ?? v.other;
    return String(v).replace(/\{(\w+)\}/g, (all, k) => (k in vars ? String(vars[k]) : all));
  };
  // The largest size up to `size` at which the text fits, then an ellipsis.
  const fit = (s, max, size, min = size - 2, weight = 400) => {
    let z = size;
    while (z > min && w(s, z, weight) > max) z -= 0.5;
    if (w(s, z, weight) <= max) return { text: s, size: z };
    const chars = [...s];
    while (chars.length > 1 && w(chars.join("") + "…", z, weight) > max) chars.pop();
    return { text: chars.join("").trimEnd() + "…", size: z };
  };
  const ftext = (x, y, s, max, o = {}) => {
    const f = fit(s, max, o.size ?? 12, o.min, o.weight);
    return text(x, y, f.text, { ...o, size: f.size });
  };
  const num = (s) => (L.comma ? s.replace(/(\d)\.(\d)/g, "$1,$2") : s);
  const monthDay = (month, day) => tr("format:monthDay", { month: tr("format:monthNames").split(" ")[month - 1], day });
  return { L, family, t: tokens(palette, theme), w, tr, fit, ftext, num, monthDay };
}

// The window frame and the page tabs; `active` indexes the six pages. Long
// languages first tighten the gaps and the type, then drop tabs after the
// active one, then before it.
function chrome(c, active) {
  const { t, w, tr } = c;
  let tabs = ["processes", "performance", "services", "startup", "network", "history"].map((k, i) => ({ i, label: tr(`shell:page.${k}`) }));
  let size = 12.5;
  let gap = 20;
  const span = () => tabs.reduce((s, it) => s + w(it.label, size, 600), 0) + gap * (tabs.length - 1);
  while (span() > W - 36 && gap > 14) gap -= 2;
  while (span() > W - 36 && size > 11.5) size -= 0.5;
  while (span() > W - 36) {
    const after = tabs.findLastIndex((it) => it.i > active);
    tabs.splice(after >= 0 ? after : tabs.findLastIndex((it) => it.i !== active), 1);
  }
  let x = 18;
  const out = [rect(0.5, 0.5, W - 1, H - 1, { rx: 10, fill: t.win, stroke: t.strokeStrong })];
  for (const it of tabs) {
    const on = it.i === active;
    out.push(text(x, 22, it.label, { size, fill: on ? t.ink : t.ink2, weight: on ? 600 : undefined }));
    if (on) out.push(rect(x + w(it.label, size, 600) / 2 - 8, 28, 16, 3, { rx: 1.5, fill: t.accent }));
    x += w(it.label, size, 600) + gap;
  }
  return out.join("");
}

// A process row: icon, name, image name, CPU and memory, with heat behind
// busy cells. Returns the markup and where the image name ends.
function procRow(c, y, [icon, name, exe, cpu, mem, heatCpu = 0, heatMem = 0], { h = 28, indent = 0, twisty } = {}) {
  const { t, w, fit, num } = c;
  const x = 18 + indent;
  const mid = y + h / 2;
  const out = [];
  if (heatCpu) out.push(rect(268, y + 1, 70, h - 2, { fill: t.heat(heatCpu) }));
  if (heatMem) out.push(rect(342, y + 1, 86, h - 2, { fill: t.heat(heatMem) }));
  if (twisty) out.push(chevron(x - 14, mid - 4.5, t.ink2, twisty === "open"));
  out.push(glyph(icon, x, mid - 8, t));
  const nameF = fit(name, 250 - (x + 24) - 30, 12.5, 11.5);
  out.push(text(x + 24, mid + 4.5, nameF.text, { size: nameF.size, fill: t.ink }));
  const exeX = x + 24 + w(nameF.text, nameF.size) + 7;
  let end = exeX;
  if (exe && 262 - exeX > 30) {
    const exeF = fit(exe, 262 - exeX, 11.5, 10.5);
    out.push(text(exeX, mid + 4.5, exeF.text, { size: exeF.size, fill: t.ink3 }));
    end = exeX + w(exeF.text, exeF.size);
  }
  out.push(text(330, mid + 4.5, num(cpu), { size: 12, fill: t.ink, anchor: "end" }), text(420, mid + 4.5, num(mem), { size: 12, fill: t.ink, anchor: "end" }));
  return { svg: out.join(""), end };
}

function columnHeads(c, y, cols) {
  return cols.map(([x, label, max, anchor]) => c.ftext(x, y, label, max, { size: 11, min: 9.5, fill: c.t.ink2, anchor })).join("");
}

// A flyout menu sized to its labels. Items: a label, "-" for a separator, or
// {label, hint, sub, danger, checked}. `x` is clamped into the image.
function menu(c, x, y, items) {
  const { t, w } = c;
  const width = Math.max(
    120,
    ...items.filter((it) => it !== "-").map((it) => {
      const o = typeof it === "string" ? { label: it } : it;
      return 30 + w(o.label, 12) + (o.hint ? w(o.hint, 11) + 24 : o.sub ? 30 : 16);
    }),
  );
  const mx = Math.min(x, W - width - 6);
  let cy = y + 4;
  const rows = [];
  const body = [];
  for (const it of items) {
    if (it === "-") {
      body.push(`<path d="M${n(mx + 1)} ${n(cy + 4.5)}h${n(width - 2)}" stroke="${t.stroke}"/>`);
      cy += 9;
      continue;
    }
    const o = typeof it === "string" ? { label: it } : it;
    const mid = cy + 12;
    rows.push({ ...o, y: cy, mid });
    if (o.checked) body.push(tick(mx + 11, mid - 5, t.ink, 1.4));
    body.push(text(mx + 30, mid + 4.2, o.label, { size: 12, fill: o.danger ? t.danger : t.ink }));
    if (o.hint) body.push(text(mx + width - 12, mid + 4, o.hint, { size: 11, fill: t.ink3, anchor: "end" }));
    if (o.sub) body.push(chevron(mx + width - 16, mid - 5, t.ink2));
    cy += 24;
  }
  const h = cy - y + 4;
  const shadow = rect(mx - 1, y + 2, width + 2, h + 2, { rx: 9, fill: t.shadow, opacity: 0.5 }) + rect(mx - 3, y + 4, width + 6, h + 4, { rx: 11, fill: t.shadow, opacity: 0.25 });
  return { svg: shadow + rect(mx, y, width, h, { rx: 8, fill: t.flyout, stroke: t.strokeStrong }) + body.join(""), rows, x: mx, w: width, h };
}

const hover = (t, mn, row, cls) => rect(mn.x + 4, row.y + 1, mn.w - 8, 22, { rx: 4, fill: t.hoverStrong, cls });

function toast(c, y, label, cls) {
  const { t, fit, w } = c;
  const f = fit(label, W - 80, 11.5, 10);
  const width = w(f.text, f.size) + 40;
  const x = W / 2 - width / 2;
  return `<g class="${cls}" opacity="0">${rect(x, y, width, 26, { rx: 6, fill: t.flyout, stroke: t.strokeStrong })}${tick(x + 11, y + 8.5, t.accent)}${text(x + 28, y + 17.3, f.text, { size: f.size, fill: t.ink })}</g>`;
}

// Segmented buttons from x (or ending at `right`), sized to their labels.
function segments(c, x, y, labels, { size = 11, right } = {}) {
  const { t, w } = c;
  const widths = labels.map((l) => Math.max(34, w(l, size, 600) + 18));
  const total = widths.reduce((a, b) => a + b, 0) + 4;
  let sx = right != null ? right - total : x;
  const start = sx;
  const items = [];
  sx += 2;
  for (const [i, l] of labels.entries()) {
    items.push({ label: l, x: sx, w: widths[i], mid: sx + widths[i] / 2 });
    sx += widths[i];
  }
  return { x: start, w: total, items, frame: rect(start, y, total, 26, { rx: 4, fill: t.control, stroke: t.stroke }), size };
}
const segChecked = (c, s, i, y, cls, rest = 1) =>
  group(
    cls,
    rect(s.items[i].x, y + 2, s.items[i].w, 22, { rx: 3, fill: c.t.checked, stroke: c.t.stroke }) + text(s.items[i].mid, y + 17, s.items[i].label, { size: s.size, fill: c.t.ink, anchor: "middle", weight: 600 }),
    { rest },
  );
const segLabels = (c, s, y) => s.items.map((it) => text(it.mid, y + 17, it.label, { size: s.size, fill: c.t.ink2, anchor: "middle" })).join("");

// Text typed into a box, one more character per step, with a caret.
function typing(c, m, x, y, word, start, step, until) {
  const out = [];
  for (let i = 1; i <= word.length; i++) {
    const s = word.slice(0, i);
    const a = start + (i - 1) * step;
    const b = i === word.length ? until : a + step;
    const cls = m.cls([
      [a - 0.01, { o: 0 }],
      [a, { o: 1 }],
      [b, { o: 1 }],
      [b + 0.01, { o: 0 }],
    ]);
    out.push(group(cls, text(x, y, s, { size: 12, fill: c.t.ink }) + rect(x + c.w(s, 12) + 1.5, y - 11.5, 1.2, 15, { fill: c.t.ink }), { rest: i === word.length ? 1 : 0 }));
  }
  return out.join("");
}

// --- Scenes -----------------------------------------------------------------

// Processes: the apps view, then the process tree, then a search.
function processes(c, m) {
  const { t, tr, w, L } = c;
  const p = [chrome(c, 0)];
  const views = segments(c, 12, 38, [tr("shell:viewMode.apps.label"), tr("shell:viewMode.tree.label"), tr("shell:viewMode.list.label")], { size: 11.5 });
  p.push(views.frame, segLabels(c, views, 38));
  const appsOn = m.cls([
    [0, { o: 1 }],
    [1.6, { o: 1 }, "out"],
    [1.75, { o: 0 }],
    [8.6, { o: 0 }, "out"],
    [8.8, { o: 1 }],
  ]);
  const treeOn = m.shown([[1.75, 8.6]], 0.15);
  p.push(segChecked(c, views, 0, 38, appsOn), segChecked(c, views, 1, 38, treeOn, 0));
  // Search box.
  const sx = views.x + views.w + 10;
  const sw = W - 12 - sx;
  p.push(rect(sx, 38, sw, 26, { rx: 4, fill: t.control, stroke: t.stroke }), magnifier(sx + 8, 44, t.ink2));
  const focus = m.shown([[4.55, 8.5]], 0.12);
  p.push(rect(sx, 62, sw, 2, { rx: 1, fill: t.accent, cls: focus, opacity: 0 }));
  const hint = m.cls([
    [0, { o: 1 }],
    [4.5, { o: 1 }],
    [4.55, { o: 0 }],
    [8.5, { o: 0 }],
    [8.65, { o: 1 }],
  ]);
  p.push(c.ftext(sx + 28, 55.5, tr("shell:search.processes"), sw - 36, { size: 11.5, min: 10, fill: t.ink3, cls: hint }));
  p.push(group(m.shown([[4.7, 8.5]], 0.01), typing(c, m, sx + 28, 55.5, "code", 4.7, 0.16, 8.5), { rest: 0 }));
  p.push(columnHeads(c, 80, [[18, tr("processes:column.name.label"), 200], [330, tr("processes:column.cpu.label"), 50, "end"], [420, tr("processes:column.mem.label"), 82, "end"]]));

  const RH = 21;
  const Y = 86;
  const section = (y, label, count) => {
    const lw = w(label, 12, 600);
    return chevron(16, y + RH / 2 - 4.5, t.ink2, true) + text(32, y + 15, label, { size: 12, fill: t.ink, weight: 600 }) + rect(38 + lw, y + 3.5, 20, 14, { rx: 7, fill: t.hoverStrong }) + text(48 + lw, y + 14.2, count, { size: 10, fill: t.ink2, anchor: "middle" });
  };
  const edge = ["globe", "Microsoft Edge", "msedge.exe", "3.1%", "2.67 GB", 0.12, 1];
  const code = ["code", "Visual Studio Code", "Code.exe", "1.4%", "1.34 GB", 0, 0.6];
  const excel = ["sheet", "Excel", "EXCEL.EXE", "0.4%", "212.0 MB"];
  const hb = ["film", "HandBrake", "HandBrake.exe", "38.6%", "1.10 GB", 0.9, 0.5];
  const drive = ["cloud", "OneDrive", "OneDrive.exe", "0.1%", "48.2 MB"];
  const node = ["node", "Node.js JavaScript Runtime", "node.exe", "0.4%", "84.1 MB"];
  const row = (y, r, o) => procRow(c, y, r, { h: RH, ...o }).svg;
  // Apps view: the resting frame.
  const apps = [section(Y, tr("processes:section.apps"), "4"), row(Y + RH, edge), row(Y + 2 * RH, code), row(Y + 3 * RH, excel), row(Y + 4 * RH, hb), section(Y + 5 * RH, tr("processes:section.background"), "2"), row(Y + 6 * RH, drive), row(Y + 7 * RH, node)];
  p.push(group(appsOn, apps.join("")));
  // Tree view: Explorer starts the apps.
  const guide = (x, y0, y1) => `<path d="M${x} ${y0}V${y1}" stroke="${t.stroke}"/>`;
  const tree = [
    row(Y, ["folder", L.explorer, "explorer.exe", "0.2%", "167.5 MB"], { indent: 14, twisty: "open" }),
    guide(29, Y + RH, Y + 7 * RH),
    row(Y + RH, edge, { indent: 34, twisty: "closed" }),
    row(Y + 2 * RH, code, { indent: 34, twisty: "open" }),
    guide(49, Y + 3 * RH, Y + 4 * RH),
    row(Y + 3 * RH, node, { indent: 54 }),
    row(Y + 4 * RH, excel, { indent: 34 }),
    row(Y + 5 * RH, hb, { indent: 34 }),
    row(Y + 6 * RH, drive, { indent: 34 }),
  ];
  p.push(group(m.shown([[1.8, 4.85]], 0.15), tree.join(""), { rest: 0 }));
  // Searching "code" keeps Visual Studio Code and its children.
  const found = [
    row(Y, code, { indent: 14, twisty: "open" }),
    guide(29, Y + RH, Y + 4 * RH),
    row(Y + RH, ["code", "Visual Studio Code", "Code.exe", "0.6%", "412.8 MB"], { indent: 34 }),
    row(Y + 2 * RH, ["code", "Visual Studio Code", "Code.exe", "0.3%", "186.0 MB"], { indent: 34 }),
    row(Y + 3 * RH, node, { indent: 34 }),
  ];
  p.push(group(m.shown([[4.95, 8.5]], 0.15), found.join(""), { rest: 0 }));
  const treeMid = views.items[1].mid;
  p.push(ripple(m, 1.6, treeMid, 51, t.ink2), ripple(m, 4.5, sx + 60, 51, t.ink2));
  p.push(
    pointer(
      m,
      [
        [0.3, 330, 244],
        [1.4, treeMid, 53],
        [4.0, treeMid, 53],
        [4.45, sx + 60, 53],
        [5.6, sx + 60, 53],
        [6.4, 330, 236],
      ],
      7.2,
    ),
  );
  return p.join("");
}

// Performance: the device list and the CPU's cores, curves always moving.
function performance(c, m) {
  const { t, tr, w, fit, num } = c;
  const p = [chrome(c, 1)];
  const P = 8;
  const devices = [
    ["CPU", num("41%  3.94 GHz"), t.cpu, periodic(32, 1, 40, 22)],
    [tr("common:resource.memory"), num("12.3 / 32.0 GB"), t.mem, periodic(32, 2, 39, 4)],
    [tr("performance:disk.titleVolumes", { index: 0, volumes: "C:" }), num("7.1 MB/s"), t.io, periodic(32, 3, 10, 18)],
    ["Wi-Fi", num("22.7 MB/s"), t.net, periodic(32, 4, 30, 24)],
    ["GPU 0", num("42%  52 °C"), t.gpu, periodic(32, 5, 42, 30)],
  ];
  devices.forEach(([name, value, color, vals], i) => {
    const y = 38 + i * 43;
    if (i === 0) p.push(rect(8, y, 142, 41, { rx: 6, fill: t.layerStrong, stroke: t.stroke }), rect(8, y + 12, 3, 17, { rx: 1.5, fill: t.accent }));
    p.push(c.ftext(20, y + 16, name, 124, { size: 11.5, min: 10, fill: t.ink }), c.ftext(20, y + 32, value, 76, { size: 10, min: 9, fill: t.ink2 }));
    p.push(scroller(m, 100, y + 20, 44, 16, vals, color, P));
  });
  // CPU heading with the core legend.
  p.push(text(162, 58, "CPU", { size: 15, fill: t.ink, weight: 600 }));
  let lx = 428;
  for (const [label, color] of [
    [tr("performance:cpu.coreKind.e"), t.cpuE],
    [tr("performance:cpu.coreKind.p"), t.cpu],
  ]) {
    const f = fit(label, 90, 10.5, 9);
    lx -= w(f.text, f.size);
    p.push(text(lx, 57, f.text, { size: f.size, fill: t.ink2 }), rect(lx - 13, 49, 8, 8, { rx: 2, fill: color }));
    lx -= 24;
  }
  // Figures, each column as wide as its label or value.
  const stats = [
    [tr("performance:cpu.utilization"), ["41%", "44%", "38%"]],
    [tr("performance:cpu.speed"), ["3.94 GHz", "4.12 GHz", "3.87 GHz"]],
    [tr("common:resource.temperature"), ["75 °C", "77 °C", "74 °C"]],
    [tr("performance:cpu.power"), ["88 W", "93 W", "84 W"]],
  ].map(([label, values]) => ({ label, values: values.map(num), width: Math.max(w(label, 10), ...values.map((v) => w(num(v), 13.5, 600))) }));
  const room = W - 12 - 162;
  const used = stats.reduce((s, st) => s + st.width, 0);
  const gap = Math.max(6, (room - used) / 3);
  const scale = used + 3 * gap > room ? (room - 18) / used : 1;
  const show = [0, 1, 2].map((k) =>
    m.loop(
      6,
      ["0%{opacity:1}31%{opacity:1}33.3%{opacity:0}97%{opacity:0}100%{opacity:1}", "0%{opacity:0}31%{opacity:0}33.3%{opacity:1}64%{opacity:1}66.6%{opacity:0}100%{opacity:0}", "0%{opacity:0}64%{opacity:0}66.6%{opacity:1}97%{opacity:1}100%{opacity:0}"][k],
    ),
  );
  let x = 162;
  for (const st of stats) {
    const colW = st.width * scale;
    p.push(c.ftext(x, 78, st.label, colW, { size: 10, min: 8.5, fill: t.ink3 }));
    st.values.forEach((v, k) => p.push(c.ftext(x, 96, v, colW + 4, { size: 13.5, min: 11, weight: 600, fill: t.ink, cls: show[k], opacity: k ? 0 : 1 })));
    x += colW + (scale < 1 ? 6 : gap);
  }
  // Four performance cores above four efficient ones.
  for (let r = 0; r < 2; r++)
    for (let k = 0; k < 4; k++) {
      const cx = 160 + k * 67;
      const cy = 108 + r * 76;
      const pc = r === 0;
      p.push(rect(cx, cy, 62, 70, { rx: 4, fill: t.layer, stroke: t.stroke }), text(cx + 6, cy + 13, `${pc ? "P" : "E"}${k}`, { size: 9.5, fill: t.ink3 }));
      p.push(scroller(m, cx + 1, cy + 18, 60, 51, periodic(24, 10 + r * 4 + k, pc ? 46 : 28, pc ? 26 : 18), pc ? t.cpu : t.cpuE, P));
    }
  return p.join("");
}

// History: click two moments on the seven-day curves.
function history(c, m) {
  const { t, tr, w, num, monthDay } = c;
  const p = [chrome(c, 5)];
  const ranges = segments(c, 12, 38, [tr("history:range.hours", { count: 1 }), tr("history:range.hours", { count: 6 }), tr("history:range.days", { count: 1 }), tr("history:range.days", { count: 7 })]);
  p.push(ranges.frame, segLabels(c, ranges, 38), segChecked(c, ranges, 3, 38));
  // Seven days, half-hour points.
  const rand = seeded(11);
  const N = 7 * 48;
  const spikes = [
    [58, 34, 3],
    [173, 62, 2.5],
    [234, 78, 3],
    [272, 41, 3],
    [308, 29, 2],
    [120, 26, 2],
  ];
  const cpu = [];
  const mem = [];
  let drift = 0;
  let level = 34;
  const rand2 = seeded(5);
  for (let i = 0; i < N; i++) {
    const h = (i % 48) / 2;
    const awake = h >= 9 && h <= 23.5 ? Math.sin(((h - 9) / 14.5) * Math.PI) : 0;
    drift = drift * 0.7 + (rand() - 0.5) * 3;
    let y = 4 + awake * 11 + drift + rand() * 3;
    for (const [at, peak, sd] of spikes) y += peak * Math.exp(-((i - at) ** 2) / (2 * sd * sd));
    cpu.push(Math.max(1.5, Math.min(96, y)));
    level += ((h >= 9 && h <= 23.5 ? 46 : 33) - level) * 0.08 + (rand2() - 0.5) * 1.2;
    mem.push(Math.max(25, Math.min(70, level + (i > 160 && i < 190 ? 6 : 0))));
  }
  const memLabel = tr("common:resource.memory");
  const labelW = Math.min(92, Math.max(w("CPU", 11), w(memLabel, 11), w("78.4%", 14, 600), w(num("18.9 GB"), 12.5, 600)));
  const X0 = 22 + labelW + 12;
  const PW = 420 - X0;
  const X = (i) => X0 + (i / (N - 1)) * PW;
  p.push(rect(12, 70, 416, 108, { rx: 6, fill: t.layer, stroke: t.stroke }));
  for (let d = 1; d < 7; d++) p.push(`<path d="M${n(X(d * 48))} 76V160" stroke="${t.stroke}"/>`);
  for (const [top, base, vals, color, label] of [
    [76, 120, cpu, t.cpu, "CPU"],
    [124, 160, mem, t.mem, memLabel],
  ]) {
    // Every other point is enough at this width.
    const line = vals
      .filter((_, i) => i % 2 === 0 || i === N - 1)
      .map((v, k, a) => `${k ? "L" : "M"}${n(X0 + (k / (a.length - 1)) * PW)} ${n(base - (v / 100) * (base - top))}`)
      .join("");
    p.push(`<path d="${line}L${X0 + PW} ${base}L${n(X0)} ${base}Z" fill="${color}" fill-opacity="0.16"/><path d="${line}" fill="none" stroke="${color}" stroke-width="1.3" stroke-linejoin="round"/>`);
    p.push(c.ftext(22, top + 14, label, labelW, { size: 11, min: 9.5, fill: t.ink2 }));
  }
  for (const d of [1, 3, 5]) p.push(text(X(d * 48), 174, monthDay(11, 7 + d), { size: 10, fill: t.ink3, anchor: "middle" }));
  // The two moments: indices into the even-point curve match the full one.
  const iA = 172;
  const iB = 234;
  const yA = 120 - (cpu[iA] / 100) * 44;
  const yB = 120 - (cpu[iB] / 100) * 44;
  const [xA, xB] = [X(iA), X(iB)];
  const now = m.cls([
    [0, { o: 1 }],
    [2.35, { o: 1 }, "out"],
    [2.5, { o: 0 }],
    [8.5, { o: 0 }, "out"],
    [8.7, { o: 1 }],
  ]);
  const atA = m.shown([[2.5, 5.75]], 0.15);
  const atB = m.shown([[5.9, 8.5]], 0.15);
  const figures = (cls, a, b, rest) => group(cls, text(22, 108, num(a), { size: 14, fill: t.ink, weight: 600 }) + text(22, 152, num(b), { size: 12.5, fill: t.ink, weight: 600 }), { rest });
  p.push(figures(now, "12.4%", "12.3 GB", 0), figures(atA, `${cpu[iA].toFixed(1)}%`, "18.9 GB", 1), figures(atB, `${cpu[iB].toFixed(1)}%`, "14.6 GB", 0));
  const path = [
    [0.3, 330, 244],
    [1.2, X0 + 60, 100],
    [2.25, xA, yA + 2],
    [4.6, xA, yA + 2],
    [5.6, xB, yB + 2],
    [7.0, xB, yB + 2],
    [7.8, 390, 244],
  ];
  const hoverX = m.cls(path.map(([s, x]) => [s, { x }, "move"]));
  p.push(group(m.shown([[1.2, 6.9]], 0.2), group(hoverX, `<path d="M0 76V160" stroke="${t.ink}" stroke-opacity="0.25"/>`, { x: xA }), { rest: 0 }));
  const sel = m.cls([
    [2.4, { x: xA, y: yA }],
    [5.8, { x: xA, y: yA }, "out"],
    [6.1, { x: xB, y: yB }],
  ]);
  const selShown = m.cls([
    [2.3, { o: 0 }, "out"],
    [2.45, { o: 1 }],
    [8.4, { o: 1 }, "out"],
    [8.6, { o: 0 }],
  ]);
  p.push(group(selShown, group(sel, `<path d="M0 ${n(76 - yA)}V${n(160 - yA)}" stroke="${t.accent}" stroke-width="1.5"/><circle r="3.5" fill="${t.accent}" stroke="${t.win}" stroke-width="1.5"/>`, { x: xA, y: yA })));
  p.push(ripple(m, 2.35, xA, yA + 2, t.accent), ripple(m, 5.75, xB, yB + 2, t.accent));
  // The busiest processes then.
  p.push(rect(12, 184, 416, 70, { rx: 6, fill: t.layer, stroke: t.stroke }));
  const sets = [
    [now, 0, tr("history:busiest"), tr("history:hint.pick"), [["globe", "msedge.exe", "3.1%", "2.67 GB"], ["code", "Code.exe", "1.4%", "1.34 GB"], ["folder", "explorer.exe", "0.2%", "167.5 MB"]]],
    [atA, 1, tr("history:busiestAt", { time: `${monthDay(11, 10)} 14:30` }), "", [["cargo", "cargo.exe", "38.6%", "1.21 GB"], ["cargo", "rustc.exe", "21.7%", "864.2 MB"], ["globe", "msedge.exe", "1.2%", "2.41 GB"]]],
    [atB, 0, tr("history:busiestAt", { time: `${monthDay(11, 11)} 21:00` }), "", [["film", "HandBrake.exe", "71.9%", "1.10 GB"], ["globe", "msedge.exe", "3.4%", "2.52 GB"], ["folder", "explorer.exe", "0.6%", "171.0 MB"]]],
  ];
  for (const [cls, rest, head, hint, list] of sets) {
    const hf = c.fit(head, 396, 11.5, 10, 600);
    const body = [text(22, 201, hf.text, { size: hf.size, fill: t.ink, weight: 600 })];
    const hx = 22 + w(hf.text, hf.size, 600) + 10;
    if (hint && 418 - hx > 90) body.push(c.ftext(hx, 201, hint, 418 - hx, { size: 10.5, min: 9.5, fill: t.ink3 }));
    list.forEach(([icon, exe, cp, mm], i) => {
      const y = 208 + i * 15;
      body.push(`<g transform="translate(22 ${y}) scale(.75) translate(-22 ${-y})">${glyph(icon, 22, y, t)}</g>`);
      body.push(text(38, y + 10.5, exe, { size: 11, fill: t.ink }), text(330, y + 10.5, num(cp), { size: 11, fill: t.ink, anchor: "end" }), text(416, y + 10.5, num(mm), { size: 11, fill: t.ink, anchor: "end" }));
    });
    p.push(group(cls, body.join(""), { rest }));
  }
  p.push(pointer(m, path, 7.8));
  return p.join("");
}

// Files: drag a spreadsheet onto the window, see who holds it, end one.
function files(c, m) {
  const { t, tr, w, fit, L } = c;
  const p = [chrome(c, 0)];
  p.push(columnHeads(c, 50, [[18, tr("processes:column.name.label"), 200], [330, tr("processes:column.cpu.label"), 50, "end"], [420, tr("processes:column.mem.label"), 82, "end"]]));
  const rows = [
    ["folder", L.explorer, "explorer.exe", "0.2%", "167.5 MB"],
    ["sheet", "Excel", "EXCEL.EXE", "0.4%", "212.0 MB"],
    ["code", "Visual Studio Code", "Code.exe", "1.4%", "1.34 GB", 0, 0.6],
    ["globe", "Microsoft Edge", "msedge.exe", "3.1%", "2.67 GB", 0.12, 1],
    ["cloud", "OneDrive", "OneDrive.exe", "0.1%", "48.2 MB"],
    ["node", "Node.js JavaScript Runtime", "node.exe", "0.4%", "84.1 MB"],
  ];
  rows.forEach((r, i) => p.push(procRow(c, 56 + i * 28, r).svg));
  // The drop highlight while the file is over the window.
  p.push(group(m.shown([[1.05, 1.95]]), rect(6, 36, 428, 218, { rx: 8, fill: t.accent, opacity: 0.07 }) + rect(6.5, 36.5, 427, 217, { rx: 8, stroke: t.accent, sw: 1.5, dash: "5 4" }), { rest: 0 }));
  // The dialog over a scrim.
  const scrim = m.cls([
    [1.95, { o: 0 }, "out"],
    [2.2, { o: 1 }],
    [8.3, { o: 1 }, "out"],
    [8.6, { o: 0 }],
  ]);
  p.push(rect(1, 1, W - 2, H - 2, { rx: 9.5, fill: t.scrim, cls: scrim }));
  const dlg = m.cls([
    [1.95, { o: 0, s: 0.97 }, "out"],
    [2.25, { o: 1, s: 1 }],
    [8.3, { o: 1, s: 1 }, "out"],
    [8.55, { o: 0, s: 0.98 }],
  ]);
  const file = `${L.report}.xlsx`;
  const d = [rect(34, 42, 372, 202, { rx: 8, fill: t.flyout, stroke: t.strokeStrong })];
  d.push(c.ftext(52, 70, tr("fileLock:title"), 320, { size: 14, min: 12, weight: 600, fill: t.ink }));
  d.push(`<path d="M384 59l8 8M392 59l-8 8" stroke="${t.ink2}" stroke-width="1.2" stroke-linecap="round"/>`);
  d.push(rect(50, 82, 340, 42, { rx: 6, fill: t.layerStrong, stroke: t.stroke }), glyph("doc", 60, 95, t));
  d.push(c.ftext(84, 99, file, 296, { size: 12.5, min: 11, weight: 600, fill: t.ink }), c.ftext(84, 115, `C:\\Users\\demo\\Documents\\${file}`, 296, { size: 10.5, min: 9, fill: t.ink3 }));
  const checking = m.shown([[2.25, 3.2]], 0.12);
  const seg = m.cls([
    [2.2, { x: 0 }, "move"],
    [2.7, { x: 250 }],
    [2.71, { x: 0 }, "move"],
    [3.2, { x: 250 }],
  ]);
  d.push(group(checking, text(50, 144, tr("fileLock:checking"), { size: 11.5, fill: t.ink2 }) + rect(50, 152, 340, 3, { rx: 1.5, fill: t.hoverStrong }) + rect(50, 152, 90, 3, { rx: 1.5, fill: t.accent, cls: seg }), { rest: 0 }));
  const found2 = m.shown([[3.3, 6.35]], 0.15);
  const found1 = m.cls([
    [6.4, { o: 0 }, "out"],
    [6.55, { o: 1 }],
    [8.3, { o: 1 }],
    [8.31, { o: 0 }],
  ]);
  d.push(c.ftext(50, 146, tr("fileLock:found", { count: 2 }), 340, { size: 11.5, min: 9.5, fill: t.ink2, cls: found2 }));
  d.push(c.ftext(50, 146, tr("fileLock:found", { count: 1 }), 340, { size: 11.5, min: 9.5, fill: t.ink2, cls: found1, opacity: 0 }));
  const end = tr("fileLock:end");
  const bw = Math.max(36, w(end, 11) + 16);
  const bx = 384 - bw;
  // Tags sit right to left before the button; the kind tag gives way when
  // the name would not fit beside it.
  const tagsWidth = (tags) => tags.reduce((s, l) => s + w(l, 10.5) + 17, 0);
  const holder = (y, icon, name, sub, tags) => {
    const out = [rect(50, y, 340, 34, { rx: 6, fill: t.layer, stroke: t.stroke }), glyph(icon, 60, y + 9, t)];
    if (tags.length > 1 && bx - 6 - tagsWidth(tags) - 88 < w(name, 12)) tags = tags.slice(1);
    let right = bx - 6;
    for (const label of [...tags].reverse()) {
      const f = fit(label, 110, 10.5, 9.5);
      const tw = w(f.text, f.size) + 12;
      out.push(rect(right - tw, y + 9, tw, 17, { rx: 8.5, fill: t.hoverStrong }), text(right - tw / 2, y + 21.2, f.text, { size: f.size, fill: t.ink2, anchor: "middle" }));
      right -= tw + 5;
    }
    out.push(c.ftext(84, y + 15, name, right - 88, { size: 12, min: 10.5, fill: t.ink }), c.ftext(84, y + 28, sub, right - 88, { size: 10, min: 9, fill: t.ink3 }));
    out.push(rect(bx, y + 7, bw, 20, { rx: 4, fill: t.control, stroke: t.strokeStrong }), text(bx + bw / 2, y + 21, end, { size: 11, fill: t.danger, anchor: "middle" }));
    return out.join("");
  };
  const row1 = m.cls([
    [3.2, { o: 0, y: 4 }, "out"],
    [3.45, { o: 1, y: 0 }],
    [6.3, { o: 1, y: 0 }, "out"],
    [6.5, { o: 0, y: 0 }],
  ]);
  const row2 = m.cls([
    [3.35, { o: 0, y: 4 }, "out"],
    [3.6, { o: 1, y: 0 }],
    [6.4, { o: 1, y: 0 }, "out"],
    [6.75, { o: 1, y: -40 }],
    [8.3, { o: 1, y: -40 }, "out"],
    [8.55, { o: 0, y: -40 }],
  ]);
  const pressed = group(m.shown([[5.4, 6.3]], 0.1), `<rect x="${n(bx)}" y="167" width="${n(bw)}" height="20" rx="4" fill="${t.danger}" fill-opacity="0.16"/>`, { rest: 0 });
  d.push(group(row1, holder(160, "sheet", "Excel", "EXCEL.EXE  PID 9876", [tr("fileLock:handleCount", { count: 2 })]) + pressed));
  d.push(group(row2, holder(200, "folder", L.explorer, "explorer.exe  PID 6120", [tr("fileLock:kind.explorer"), tr("fileLock:handleCount", { count: 1 })])));
  p.push(group(dlg, d.join("")));
  const bxMid = bx + bw / 2;
  p.push(ripple(m, 6.2, bxMid, 177, t.ink2));
  // The file being dragged in, with the pointer.
  const travel = [
    [0.2, 470, 196],
    [0.45, 470, 196],
    [1.75, 236, 142],
  ];
  const chipMove = m.cls(travel.map(([s, x, y]) => [s, { x, y }, "move"]));
  const chipShown = m.cls([
    [0.2, { o: 1, s: 1 }],
    [1.9, { o: 1, s: 1 }, "out"],
    [2.1, { o: 0, s: 0.85 }],
  ]);
  const cf = fit(file, 150, 11.5, 10);
  const chipW = w(cf.text, cf.size) + 36;
  p.push(group(chipMove, group(chipShown, rect(12, 14, chipW, 26, { rx: 6, fill: t.flyout, stroke: t.strokeStrong }) + glyph("doc", 18, 19, t) + text(40, 31, cf.text, { size: cf.size, fill: t.ink }), { rest: 0 }), { x: 236, y: 142 }));
  p.push(
    pointer(
      m,
      [
        ...travel,
        [4.6, 236, 142],
        [5.35, bxMid - 2, 178],
        [6.7, bxMid - 2, 178],
        [7.6, 300, 236],
      ],
      7.6,
    ),
  );
  return p.join("");
}

// Rules: lower a process's priority, then turn on Efficiency mode.
function rules(c, m) {
  const { t, tr, w, L } = c;
  const p = [chrome(c, 0)];
  p.push(columnHeads(c, 50, [[18, tr("processes:column.name.label"), 200], [330, tr("processes:column.cpu.label"), 50, "end"], [420, tr("processes:column.mem.label"), 82, "end"]]));
  p.push(rect(8, 58, 424, 28, { rx: 4, fill: t.hoverStrong, cls: m.shown([[1.4, 3.4], [5.1, 6.1]]) }));
  const rows = [
    ["film", "HandBrake", "HandBrake.exe", "86.2%", "1.10 GB", 1, 0.5],
    ["code", "Visual Studio Code", "Code.exe", "1.4%", "1.34 GB", 0, 0.6],
    ["globe", "Microsoft Edge", "msedge.exe", "3.1%", "2.67 GB", 0.12, 1],
    ["folder", L.explorer, "explorer.exe", "0.2%", "167.5 MB"],
    ["cloud", "OneDrive", "OneDrive.exe", "0.1%", "48.2 MB"],
    ["node", "Node.js JavaScript Runtime", "node.exe", "0.4%", "84.1 MB"],
  ];
  let leafAt = 0;
  rows.forEach((r, i) => {
    const row = procRow(c, 58 + i * 28, r);
    if (i === 0) leafAt = row.end + 8;
    p.push(row.svg);
  });
  const leafShown = m.cls([
    [5.95, { o: 0, s: 0.6 }, "out"],
    [6.2, { o: 1, s: 1 }],
    [8.3, { o: 1, s: 1 }, "out"],
    [8.5, { o: 0, s: 1 }],
  ]);
  p.push(group(leafShown, leaf(leafAt, 65, t.accent), { rest: 0 }));
  const PX = 100;
  const PY = 72;
  const items = [{ label: tr("common:endProcess"), hint: "Del", danger: true }, tr("common:endProcessTree"), "-", { label: tr("processes:menu.setPriority"), sub: true }, tr("common:efficiencyMode"), tr("processes:menu.addRule"), "-", tr("common:openFileLocation")];
  const mn = menu(c, PX + 4, 76, items);
  const prio = mn.rows[2];
  const eff = mn.rows[3];
  const levels = ["realtime", "high", "aboveNormal", "normal", "belowNormal", "idle"].map((k) => tr(`processes:priority.${k}`));
  const subWidth = Math.max(100, ...levels.map((l) => 46 + w(l, 12)));
  const subX = Math.min(mn.x + mn.w - 4, W - subWidth - 6);
  const subH = 6 * 24 + 8;
  const sub = menu(c, subX, Math.min(prio.y - 4, H - subH - 4), levels.map((l, i) => (i === 3 ? { label: l, checked: true } : l)));
  const low = sub.rows[5];
  const open1 = m.cls([
    [1.38, { o: 0, y: -4 }, "out"],
    [1.55, { o: 1, y: 0 }],
    [3.3, { o: 1, y: 0 }, "out"],
    [3.45, { o: 0, y: 0 }],
  ]);
  const open2 = m.cls([
    [5.08, { o: 0, y: -4 }, "out"],
    [5.25, { o: 1, y: 0 }],
    [5.95, { o: 1, y: 0 }, "out"],
    [6.1, { o: 0, y: 0 }],
  ]);
  const subOpen = m.cls([
    [2.2, { o: 0, x: -4 }, "out"],
    [2.38, { o: 1, x: 0 }],
    [3.3, { o: 1, x: 0 }, "out"],
    [3.45, { o: 0, x: 0 }],
  ]);
  p.push(group(open1, mn.svg + hover(t, mn, prio, m.shown([[2.05, 3.35]], 0.08))));
  p.push(group(subOpen, sub.svg + group(m.shown([[3.0, 3.35]], 0.06), hover(t, sub, low), { rest: 0 })));
  p.push(group(open2, mn.svg + group(m.shown([[5.65, 6.0]], 0.06), hover(t, mn, eff), { rest: 0 }), { rest: 0 }));
  p.push(toast(c, 224, tr("actions:priority.done", { name: "HandBrake", priority: levels[5] }), m.shown([[3.55, 4.9]], 0.2)));
  p.push(toast(c, 224, tr("actions:efficiency.on.done", { name: "HandBrake" }), m.shown([[6.2, 7.7]], 0.2)));
  const prioX = mn.x + Math.min(mn.w - 30, 70);
  const lowX = sub.x + 34;
  const effX = mn.x + Math.min(mn.w - 30, 64);
  p.push(ripple(m, 1.35, PX, PY, t.ink2), ripple(m, 3.25, lowX, low.mid, t.ink2), ripple(m, 5.05, PX, PY, t.ink2), ripple(m, 5.9, effX, eff.mid, t.ink2));
  p.push(
    pointer(
      m,
      [
        [0.3, 360, 240],
        [1.2, PX, PY],
        [1.8, PX, PY],
        [2.15, prioX, prio.mid],
        [2.55, lowX, prio.mid + 2],
        [3.0, lowX, low.mid],
        [4.3, lowX, low.mid],
        [5.0, PX, PY],
        [5.25, PX, PY],
        [5.65, effX, eff.mid],
        [6.8, effX, eff.mid],
        [7.6, 380, 244],
      ],
      7.6,
    ),
  );
  return p.join("");
}

// Ports: search 8080, find who listens and who is connected, close one.
function ports(c, m) {
  const { t, tr, w } = c;
  const p = [chrome(c, 4)];
  let filters = segments(c, 0, 38, ["all", "listen", "established", "udp"].map((k) => tr(`connections:filter.${k}.label`)), { right: 428 });
  if (filters.w > 230) filters = segments(c, 0, 38, filters.items.map((it) => it.label), { right: 428, size: 10 });
  p.push(filters.frame, segLabels(c, filters, 38), segChecked(c, filters, 0, 38));
  const sw = filters.x - 10 - 12;
  p.push(rect(12, 38, sw, 26, { rx: 4, fill: t.control, stroke: t.stroke }), magnifier(20, 44, t.ink2));
  p.push(rect(12, 62, sw, 2, { rx: 1, fill: t.accent, cls: m.shown([[1.15, 7.6]], 0.15) }));
  const hint = m.cls([
    [0, { o: 1 }],
    [1.1, { o: 1 }],
    [1.15, { o: 0 }],
    [7.6, { o: 0 }],
    [7.75, { o: 1 }],
  ]);
  p.push(c.ftext(40, 55.5, tr("connections:search"), sw - 36, { size: 11.5, min: 9.5, fill: t.ink3, cls: hint, opacity: 0 }));
  p.push(typing(c, m, 40, 55.5, "8080", 1.35, 0.18, 7.6));
  // Column heads; a long one wraps onto two lines at its space.
  const head = (x, label, max, anchor) => {
    for (const size of [11, 10.5, 10]) if (w(label, size) <= max) return text(x, 84, label, { size, fill: t.ink2, anchor });
    const words = label.split(" ");
    if (words.length > 1) {
      let best = null;
      for (let i = 1; i < words.length; i++) {
        const a = words.slice(0, i).join(" ");
        const b = words.slice(i).join(" ");
        const wide = Math.max(w(a, 10), w(b, 10));
        if (!best || wide < best.wide) best = { a, b, wide };
      }
      if (best.wide <= max) return text(x, 77, best.a, { size: 10, fill: t.ink2, anchor }) + text(x, 88, best.b, { size: 10, fill: t.ink2, anchor });
    }
    return c.ftext(x, 84, label, max, { size: 10, min: 9, fill: t.ink2, anchor });
  };
  p.push(head(18, tr("connections:column.process"), 92), head(178, tr("connections:column.localPort"), 64, "end"), head(190, tr("connections:column.remoteAddress"), 84), head(330, tr("connections:column.remotePort"), 54, "end"), head(342, tr("common:status"), 84));
  const listen = tr("connections:state.listen");
  const established = tr("connections:state.established");
  const conns = [
    ["svc", "svchost.exe", "135", "—", "—", listen],
    ["globe", "msedge.exe", "52144", "203.0.113.10", "443", established],
    ["java", "java.exe", "8080", "—", "—", listen],
    ["code", "Code.exe", "52011", "127.0.0.1", "52010", established],
    ["node", "node.exe", "5173", "—", "—", listen],
    ["curl", "curl.exe", "52190", "127.0.0.1", "8080", established],
  ];
  const Y0 = 94;
  const RH = 26;
  const line = (y, [icon, name, lp, ra, rp, st], lit) => {
    const hit = (s) => (lit && s === "8080" ? { fill: t.accent, weight: 600 } : { fill: s === "—" ? t.ink3 : t.ink });
    return [glyph(icon, 18, y + 5, t), text(42, y + 17.5, name, { size: 12, fill: t.ink }), text(178, y + 17.5, lp, { size: 12, anchor: "end", ...hit(lp) }), text(190, y + 17.5, ra, { size: 12, ...hit(ra) }), text(330, y + 17.5, rp, { size: 12, anchor: "end", ...hit(rp) }), c.ftext(342, y + 17.5, st, 86, { size: 12, min: 10, fill: t.ink2 })].join("");
  };
  const gone = m.cls([
    [0, { o: 1 }],
    [2.15, { o: 1 }, "out"],
    [2.3, { o: 0 }],
    [7.7, { o: 0 }, "out"],
    [7.95, { o: 1 }],
  ]);
  p.push(group(gone, conns.map((r, i) => ([2, 5].includes(i) ? "" : line(Y0 + i * RH, r))).join(""), { rest: 0 }));
  const up = (i, to) =>
    m.cls([
      [2.2, { y: 0 }, "out"],
      [2.55, { y: (to - i) * RH }],
      [7.7, { y: (to - i) * RH }, "out"],
      [8.05, { y: 0 }],
    ]);
  const plain = m.cls([
    [2.2, { o: 1 }],
    [2.25, { o: 0 }],
    [7.7, { o: 0 }],
    [7.75, { o: 1 }],
  ]);
  const lit = m.cls([
    [2.2, { o: 0 }],
    [2.25, { o: 1 }],
    [7.7, { o: 1 }],
    [7.75, { o: 0 }],
  ]);
  const both = (i) => group(plain, line(Y0 + i * RH, conns[i]), { rest: 0 }) + group(lit, line(Y0 + i * RH, conns[i], true));
  p.push(group(up(2, 0), both(2), { y: -2 * RH }));
  const curlShown = m.cls([
    [4.85, { o: 1 }, "out"],
    [5.15, { o: 0 }],
    [7.7, { o: 0 }, "out"],
    [8.0, { o: 1 }],
  ]);
  p.push(group(up(5, 1), group(curlShown, rect(8, Y0 + 5 * RH, 424, RH, { rx: 4, fill: t.hoverStrong, cls: m.shown([[3.75, 4.9]], 0.1) }) + both(5)), { y: -4 * RH }));
  // The connection's menu.
  const PX = 120;
  const PY = Y0 + RH + 14;
  const mn = menu(c, PX + 4, PY + 4, [tr("connections:menu.close"), tr("connections:menu.end", { name: "curl.exe" }), "-", tr("connections:menu.copyLocal"), tr("connections:menu.copyRemote")]);
  const close = mn.rows[0];
  const closeX = mn.x + Math.min(mn.w - 30, 60);
  const open = m.cls([
    [3.78, { o: 0, y: -4 }, "out"],
    [3.95, { o: 1, y: 0 }],
    [4.75, { o: 1, y: 0 }, "out"],
    [4.9, { o: 0, y: 0 }],
  ]);
  p.push(group(open, mn.svg + hover(t, mn, close, m.shown([[4.35, 4.8]], 0.06))));
  p.push(ripple(m, 1.05, 110, 51, t.ink2), ripple(m, 3.72, PX, PY, t.ink2), ripple(m, 4.72, closeX, close.mid, t.ink2));
  p.push(
    pointer(
      m,
      [
        [0.3, 300, 240],
        [1.0, 110, 51],
        [2.7, 110, 51],
        [3.6, PX, PY],
        [3.95, PX, PY],
        [4.35, closeX, close.mid],
        [5.6, closeX, close.mid],
        [6.5, 380, 244],
      ],
      6.5,
    ),
  );
  return p.join("");
}

// Loop lengths and start offsets: the six tiles share a page, so their
// pointers move at different moments.
const SCENES = {
  processes: { draw: processes, dur: 10, phase: 0 },
  performance: { draw: performance, dur: 8, phase: 0 },
  history: { draw: history, dur: 10, phase: 3.2 },
  files: { draw: files, dur: 10, phase: 6.1 },
  rules: { draw: rules, dur: 9, phase: 4.6 },
  ports: { draw: ports, dur: 10, phase: 1.7 },
};

// The title each demo carries (screen readers, hover).
function title(id, c) {
  const { tr } = c;
  switch (id) {
    case "processes":
      return tr("shell:page.processes");
    case "performance":
      return tr("shell:page.performance");
    case "history":
      return tr("shell:page.history");
    case "files":
      return tr("fileLock:title");
    case "rules":
      return `${tr("processes:menu.setPriority")} · ${tr("common:efficiencyMode")}`;
    default:
      return tr("shell:page.network");
  }
}

/**
 * One demo as an SVG document. `strings` maps every key in KEYS to the
 * language's locale value, `palette` is resources/theme.json, and
 * `measure(text, cssFont)` gives text widths in pixels.
 */
export function featureDemo({ id, lang, theme, palette, strings, measure }) {
  const scene = SCENES[id];
  if (!scene) throw Error(`unknown demo ${id}`);
  const c = context({ lang, theme, palette, strings, measure });
  const m = new Motion(scene.dur, scene.phase);
  const body = scene.draw(c, m);
  const name = esc(`Argus · ${title(id, c)}`);
  return `<svg xmlns="http://www.w3.org/2000/svg" width="${W}" height="${H}" viewBox="0 0 ${W} ${H}" role="img" aria-label="${name}"><title>${name}</title><style>text{font-family:${c.family};font-variant-numeric:tabular-nums}${m.css.join("")}@media (prefers-reduced-motion:reduce){*{animation:none!important}}</style>${body}</svg>`;
}
