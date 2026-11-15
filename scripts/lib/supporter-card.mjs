// The supporter card in the READMEs: one standalone SVG per supporter, README
// language and theme, with the avatar embedded. GitHub serves repository SVG
// files with "default-src 'none'; style-src 'unsafe-inline'; sandbox": no
// script, no external font or image, so everything is drawn here and the
// motion is CSS inside the file. The card is animated, so it uses no SVG
// filter (an animated image is repainted whole on every frame); soft light
// is drawn with gradients.
//
// The light field takes its colours from the avatar. Five lights drift on
// their own paths: across, up and down, and their breathing run on separate
// clocks of unrelated lengths, so the field never repeats the same way and
// never stops all at once; each crosses a good part of the card in a few
// seconds, so the motion reads at a glance.
//
// No DOM here: scripts/supporters.mjs runs this module in a headless page,
// where `measure` comes from a canvas. Tests run it in Node.

export const W = 720;
export const H = 240;

const SC = "'PingFang SC','Microsoft YaHei UI','Microsoft YaHei','Noto Sans CJK SC','Noto Sans SC'";
const TC = "'PingFang TC','Microsoft JhengHei UI','Microsoft JhengHei','Noto Sans CJK TC','Noto Sans TC'";
const JA = "'Hiragino Sans','Yu Gothic UI','Meiryo','Noto Sans CJK JP','Noto Sans JP'";

// One entry per README, in the order of the language links at their top.
export const readmes = [
  { lang: "zh-CN", file: "README.md", tier: "项目支持 & 特别致谢", thanks: "感谢对 Argus 持续开发的支持", cjk: SC },
  { lang: "zh-TW", file: "README.zh-TW.md", tier: "專案支持 & 特別致謝", thanks: "感謝對 Argus 持續開發的支持", cjk: TC },
  { lang: "en", file: "README.en.md", tier: "Project support & special thanks", thanks: "Thank you for supporting the ongoing work on Argus", cjk: SC },
  { lang: "es", file: "README.es.md", tier: "Apoyo al proyecto & agradecimiento especial", thanks: "Gracias por apoyar el desarrollo continuo de Argus", cjk: SC },
  { lang: "de", file: "README.de.md", tier: "Projektunterstützung & besonderer Dank", thanks: "Danke für die Unterstützung der Weiterentwicklung von Argus", cjk: SC },
  { lang: "ja", file: "README.ja.md", tier: "プロジェクト支援 & スペシャルサンクス", thanks: "Argus の継続的な開発へのご支援に感謝します", cjk: JA },
  { lang: "fr", file: "README.fr.md", tier: "Soutien au projet & remerciements particuliers", thanks: "Merci de soutenir le développement continu d’Argus", cjk: SC },
  { lang: "ru", file: "README.ru.md", tier: "Поддержка проекта & особая благодарность", thanks: "Спасибо за поддержку развития Argus", cjk: SC },
];
export const themes = ["light", "dark"];

export const cardPath = (id, lang, theme) => `docs/images/supporters/${id}-${lang}-${theme}.svg`;
export const MARK_START = "<!-- supporters:start · node scripts/supporters.mjs -->";
export const MARK_END = "<!-- supporters:end -->";

const esc = (text) => String(text).replace(/[&<>"]/g, (c) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;" })[c]);
const n = (v) => Math.round(v * 100) / 100;

// The block between the markers in each README. White space inside a link
// is underlined as link text on GitHub, so each picture stays on one line.
// GitHub drops target from README links; renderers that keep it open a tab.
export function readmeBlock(lang, supporters) {
  const { tier } = readmes.find((r) => r.lang === lang);
  const colon = /^(zh|ja)/.test(lang) ? "：" : ": ";
  const cards = supporters.map(
    (s) =>
      `<p align="center">\n  <a href="${esc(s.url)}" target="_blank" rel="noopener"><picture><source media="(prefers-color-scheme: dark)" srcset="${cardPath(s.id, lang, "dark")}"><img src="${cardPath(s.id, lang, "light")}" width="${W}" alt="${esc(tier + colon + s.name)}"></picture></a>\n</p>`,
  );
  return [MARK_START, ...cards, MARK_END].join("\n");
}

// The avatar's colours, from its pixels (RGBA, as a canvas gives them). Only
// pixels with some colour count: a drawing on white with grey hair would
// otherwise come out grey. They are binned by hue; neighbouring bins merge
// into one colour each, largest first. When the avatar has fewer than four,
// the first ones lend shifted hues; a grey avatar gives greys.
export function extractPalette(rgba) {
  const bins = Array.from({ length: 24 }, () => ({ n: 0, r: 0, g: 0, b: 0 }));
  const grey = { n: 0, r: 0, g: 0, b: 0 };
  const add = (bin, r, g, b) => {
    bin.n++;
    bin.r += r;
    bin.g += g;
    bin.b += b;
  };
  let total = 0;
  for (let i = 0; i < rgba.length; i += 4) {
    if (rgba[i + 3] < 128) continue;
    const [r, g, b] = [rgba[i], rgba[i + 1], rgba[i + 2]];
    const [h, s, l] = hsl([r, g, b]);
    if (l <= 0.12 || l >= 0.93) continue;
    if (s >= 0.15) {
      add(bins[Math.floor(h / 15) % 24], r, g, b);
      total++;
    } else add(grey, r, g, b);
  }
  // Runs of non-empty bins round the colour wheel, each one colour.
  const groups = [];
  const start = bins.findIndex((bin) => bin.n === 0);
  for (let k = 0, run = null; start >= 0 && k <= 24; k++) {
    const bin = bins[(start + k) % 24];
    if (bin.n && k < 24) {
      run ??= { n: 0, r: 0, g: 0, b: 0 };
      for (const key of ["n", "r", "g", "b"]) run[key] += bin[key];
    } else if (run) {
      groups.push(run);
      run = null;
    }
  }
  if (start < 0) groups.push(bins.reduce((sum, bin) => ({ n: sum.n + bin.n, r: sum.r + bin.r, g: sum.g + bin.g, b: sum.b + bin.b }), { n: 0, r: 0, g: 0, b: 0 }));
  const colours = groups
    .filter((group) => group.n >= Math.max(2, total * 0.04))
    .sort((a, b) => b.n - a.n)
    .map((group) => hsl([group.r / group.n, group.g / group.n, group.b / group.n]));
  if (!colours.length) return Array(4).fill(grey.n ? hex(hsl([grey.r / grey.n, grey.g / grey.n, grey.b / grey.n])) : "#808080");
  const shifted = ([h, s, l], by) => [(h + by + 360) % 360, s, l];
  const palette = colours.slice(0, 4);
  const lend = [[0, -24], [1, 16], [0, 18], [1, -14]];
  for (let k = 0; palette.length < 4; k++) {
    const [from, by] = lend[k];
    palette.push(shifted(colours[from] ?? colours[0], by));
  }
  return palette.map(hex);
}

function hsl([r, g, b]) {
  r /= 255;
  g /= 255;
  b /= 255;
  const max = Math.max(r, g, b),
    min = Math.min(r, g, b),
    l = (max + min) / 2;
  if (max === min) return [0, 0, l];
  const d = max - min,
    s = l > 0.5 ? d / (2 - max - min) : d / (max + min);
  const h = max === r ? (g - b) / d + (g < b ? 6 : 0) : max === g ? (b - r) / d + 2 : (r - g) / d + 4;
  return [h * 60, s, l];
}
function hex([h, s, l]) {
  const f = (k) => {
    const a = s * Math.min(l, 1 - l),
      kk = (k + h / 30) % 12;
    return Math.round(255 * (l - a * Math.max(-1, Math.min(kk - 3, 9 - kk, 1))));
  };
  return "#" + [f(0), f(8), f(4)].map((v) => v.toString(16).padStart(2, "0")).join("");
}
// A colour moved into a lightness band, with at least some saturation:
// glowing colours on dark, pastel washes on light. The website derives its
// card colours the same way.
export function tone(colour, lo, hi, minSat) {
  const v = parseInt(colour.slice(1), 16);
  let [h, s, l] = hsl([v >> 16, (v >> 8) & 255, v & 255]);
  l = Math.min(hi, Math.max(lo, l));
  if (s > 0.06) s = Math.max(s, minSat);
  return hex([h, s, l]);
}
export const tones = (palette, theme) => palette.map((c) => (theme === "dark" ? tone(c, 0.54, 0.64, 0.6) : tone(c, 0.64, 0.75, 0.52)));

// The largest size up to `size` at which the text fits, then an ellipsis.
function fit(measure, text, max, size, min, font) {
  let s = size;
  while (s > min && measure(text, font(s)) > max) s -= 0.5;
  if (measure(text, font(s)) <= max) return { text, size: s };
  const chars = [...text];
  while (chars.length > 1 && measure(chars.join("") + "…", font(s)) > max) chars.pop();
  return { text: chars.join("").trimEnd() + "…", size: s };
}

// The nickname in Great Vibes, as outlines: `script(text, size)` gives the
// word's path drawn from (0, 0) on the baseline, or null when the font lacks
// one of its characters. Shrinks a long name to fit; null when even the
// smallest size is too wide, and the card then sets the name as text.
export const SCRIPT_SIZE = 62;
export function fitSignature(script, text, max, size = SCRIPT_SIZE, min = 34) {
  let art = script(text, size);
  if (!art) return null;
  const span = (a) => a.right - Math.min(0, a.left);
  if (span(art) > max) art = script(text, Math.max(min, (size * max) / span(art)));
  return art && span(art) <= max ? art : null;
}

/**
 * One card. `image` is the avatar as a data: URL, `grain` a data: URL of a
 * 72 px grain tile, `logo` the inner markup of resources/logo.svg with its
 * ids made unique, `measure(text, cssFont)` the text width in pixels, and
 * `script` draws the nickname in the script font (see fitSignature).
 */
export function supporterCard({ name, image, palette, grain, logo, lang, theme, measure, script, animate = true }) {
  const copy = readmes.find((r) => r.lang === lang);
  const dark = theme === "dark";
  const R = 20;
  const latin = "'Segoe UI Variable Display','Segoe UI',system-ui,-apple-system,BlinkMacSystemFont",
    text = "'Segoe UI Variable Text','Segoe UI',system-ui,-apple-system,BlinkMacSystemFont";
  const display = `${latin},${copy.cjk},sans-serif`,
    body = `${text},${copy.cjk},sans-serif`;
  const p = tones(palette, theme);
  const ink = dark ? "#f6f7f9" : "#111214",
    ink2 = dark ? "rgba(255,255,255,.76)" : "rgba(0,0,0,.66)";

  // x, y, radius, colour, strength; then the drift: x reach and seconds,
  // y reach and seconds, largest scale and seconds.
  const lights = [
    [130, 124, 180, p[0], dark ? 0.78 : 0.62, 220, 6.5, -55, 4.6, 1.25, 7.8],
    [420, 24, 190, p[1], dark ? 0.64 : 0.5, -240, 7.2, 80, 5.4, 1.2, 8.6],
    [640, 205, 190, p[2], dark ? 0.7 : 0.55, -260, 6.8, -80, 5.8, 1.25, 7.2],
    [300, 250, 160, p[3], dark ? 0.55 : 0.45, 230, 7.6, -105, 6.2, 1.2, 9.4],
    [560, 70, 140, p[0], dark ? 0.45 : 0.4, -200, 5.9, 62, 4.9, 1.3, 6.6],
  ];
  let gradients = "",
    field = "",
    motion = "";
  lights.forEach(([x, y, r, colour, a, dx, tx, dy, ty, k, tk], i) => {
    gradients += `<radialGradient id="f${i}"><stop offset="0" stop-color="${colour}" stop-opacity="${a}"/><stop offset=".35" stop-color="${colour}" stop-opacity="${n(a * 0.72)}"/><stop offset=".7" stop-color="${colour}" stop-opacity="${n(a * 0.24)}"/><stop offset="1" stop-color="${colour}" stop-opacity="0"/></radialGradient>`;
    field += `<g class="x${i}"><g class="y${i}"><g class="k${i}" style="transform-origin:${x}px ${y}px"><circle cx="${x}" cy="${y}" r="${r}" fill="url(#f${i})"/></g></g></g>`;
    motion +=
      `.x${i}{animation:x${i} ${tx}s ease-in-out infinite alternate}@keyframes x${i}{to{transform:translateX(${dx}px)}}` +
      `.y${i}{animation:y${i} ${ty}s ease-in-out infinite alternate}@keyframes y${i}{to{transform:translateY(${dy}px)}}` +
      `.k${i}{animation:k${i} ${tk}s ease-in-out infinite alternate}@keyframes k${i}{to{transform:scale(${k})}}`;
  });

  const x0 = 188,
    ax = 98,
    ay = 120,
    ar = 56;
  // The nickname: the script outlines, or text when the font cannot draw it.
  const art = script ? fitSignature(script, name, W - x0 - 64) : null;
  const title = art ? null : fit(measure, name, W - x0 - 64, 38, 22, (s) => `600 ${s}px ${display}`);
  const thanks = fit(measure, copy.thanks, W - x0 - 56, 15, 12, (s) => `400 ${s}px ${body}`);
  // Chip, name and thanks stacked round the avatar's centre, spaced by the
  // name's real ink: a script's tall loops and long tails need room.
  const above = art ? art.top : title.size * 0.78,
    below = art ? art.bottom : title.size * 0.22;
  const block = 28 + 12 + above + below + 14 + 11 + 4,
    chipY = n(ay - block / 2),
    baseline = n(chipY + 28 + 12 + above),
    thanksY = n(baseline + below + 14 + 11);
  // The chip: mark, "Argus", a hairline, the tier.
  const argusW = measure("Argus", `600 12.5px ${display}`),
    tier = fit(measure, copy.tier, W - x0 - 64 - (29 + argusW + 21 + 13), 12, 10.5, (s) => `500 ${s}px ${body}`),
    tierW = measure(tier.text, `500 ${tier.size}px ${body}`),
    chipW = n(29 + argusW + 21 + tierW + 13);
  const glass = dark ? "rgba(255,255,255,.08)" : "rgba(255,255,255,.66)",
    glassEdge = dark ? "rgba(255,255,255,.14)" : "rgba(0,0,0,.07)";

  const defs =
    gradients +
    `<clipPath id="fc"><rect width="${W}" height="${H}" rx="${R}"/></clipPath>` +
    `<clipPath id="fp"><circle cx="${ax}" cy="${ay}" r="${ar}"/></clipPath>` +
    `<radialGradient id="ft"><stop offset="0" stop-color="#000" stop-opacity=".34"/><stop offset=".6" stop-color="#000" stop-opacity=".16"/><stop offset="1" stop-color="#000" stop-opacity="0"/></radialGradient>` +
    `<linearGradient id="fw" x1="0" y1="0" x2="1" y2="1"><stop offset="0" stop-color="${p[0]}" stop-opacity="${dark ? 0.16 : 0.14}"/><stop offset="1" stop-color="${p[2]}" stop-opacity="${dark ? 0.14 : 0.12}"/></linearGradient>` +
    `<pattern id="fg" width="72" height="72" patternUnits="userSpaceOnUse"><image href="${grain}" width="72" height="72"/></pattern>` +
    `<radialGradient id="fv" cx=".5" cy=".5" r=".75"><stop offset=".55" stop-color="#000" stop-opacity="0"/><stop offset="1" stop-color="#000" stop-opacity="${dark ? 0.26 : 0}"/></radialGradient>` +
    `<radialGradient id="fa"><stop offset="0" stop-color="${p[0]}" stop-opacity="${dark ? 0.5 : 0.36}"/><stop offset="1" stop-color="${p[0]}" stop-opacity="0"/></radialGradient>` +
    `<linearGradient id="fe" x1="0" y1="0" x2="1" y2="1"><stop offset="0" stop-color="#fff" stop-opacity="${dark ? 0.32 : 0.95}"/><stop offset=".45" stop-color="#fff" stop-opacity="${dark ? 0.06 : 0.5}"/><stop offset="1" stop-color="${dark ? "#fff" : "#000"}" stop-opacity="${dark ? 0.1 : 0.08}"/></linearGradient>`;
  const style =
    `.d{font-family:${display}}.t{font-family:${body}}` +
    (animate ? motion : "") +
    (dark ? ".fl>g{mix-blend-mode:screen}" : "") +
    "@media (prefers-reduced-motion:reduce){*{animation:none!important}}";

  const alt = copy.tier + (/^(zh|ja)/.test(lang) ? "：" : ": ") + name;
  const svg =
    `<svg xmlns="http://www.w3.org/2000/svg" width="${W}" height="${H}" viewBox="0 0 ${W} ${H}" role="img" aria-label="${esc(alt)}"><title>${esc(alt)}</title><style>${style}</style><defs>${defs}</defs>` +
    `<g clip-path="url(#fc)"><rect width="${W}" height="${H}" fill="${dark ? "#0e0f12" : "#fbfbfa"}"/><rect width="${W}" height="${H}" fill="url(#fw)"/><g class="fl">${field}</g>` +
    `<circle cx="${ax}" cy="${ay}" r="${ar * 2.1}" fill="url(#fa)"/>` +
    // On dark, a still shade behind the words keeps them readable wherever
    // the lights meet.
    (dark ? `<ellipse cx="${x0 + 150}" cy="${ay + 4}" rx="300" ry="104" fill="url(#ft)"/>` : "") +
    `<rect width="${W}" height="${H}" fill="url(#fv)"/><rect width="${W}" height="${H}" fill="url(#fg)" opacity="${dark ? 0.7 : 0.45}"/></g>` +
    `<rect x=".5" y=".5" width="${W - 1}" height="${H - 1}" rx="${R - 0.5}" fill="none" stroke="url(#fe)"/>` +
    (dark ? "" : `<rect x="-.5" y="-.5" width="${W + 1}" height="${H + 1}" rx="${R + 0.5}" fill="none" stroke="#000" stroke-opacity=".07"/>`) +
    `<image href="${image}" x="${ax - ar}" y="${ay - ar}" width="${2 * ar}" height="${2 * ar}" clip-path="url(#fp)" preserveAspectRatio="xMidYMid slice"/>` +
    `<circle cx="${ax}" cy="${ay}" r="${ar + 0.5}" fill="none" stroke="#fff" stroke-opacity="${dark ? 0.4 : 0.95}" stroke-width="1.2"/>` +
    `<g transform="translate(${x0} ${chipY})"><rect x=".5" y=".5" width="${chipW}" height="27" rx="13.5" fill="${glass}" stroke="${glassEdge}"/>` +
    `<svg x="9" y="6.5" width="15" height="15" viewBox="0 0 1024 1024" aria-hidden="true">${logo}</svg>` +
    `<text x="29" y="18.5" class="d" font-size="12.5" font-weight="600" fill="${ink}">Argus</text>` +
    `<path d="M${n(29 + argusW + 10.5)} 8.5v11" stroke="${glassEdge}"/>` +
    `<text x="${n(29 + argusW + 21)}" y="18.5" class="t" font-size="${tier.size}" font-weight="500" fill="${ink2}">${esc(tier.text)}</text></g>` +
    (art
      ? `<path transform="translate(${x0 - Math.min(0, n(art.left))} ${baseline})" d="${art.d}" fill="${ink}"/>`
      : `<text x="${x0 - 2}" y="${baseline}" class="d" font-size="${title.size}" font-weight="600" fill="${ink}" letter-spacing="-.02em">${esc(title.text)}</text>`) +
    `<text x="${x0}" y="${thanksY}" class="t" font-size="${thanks.size}" fill="${ink2}">${esc(thanks.text)}</text>` +
    // A round button in the corner: the card leads to the supporter's page.
    `<g transform="translate(${W - 36} 36)"><circle r="15.5" fill="${glass}" stroke="${glassEdge}"/><path d="M-3.6 3.6 3.6-3.6M-1.6-3.6h5.2v5.2" fill="none" stroke="${ink}" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round"/></g>` +
    `</svg>`;
  return { svg, width: W, height: H };
}
