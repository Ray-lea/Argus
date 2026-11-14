// A small TrueType reader: enough to draw a word with a font's own glyph
// outlines as an SVG path, so a card shows the same letters everywhere
// without loading the font. It reads quadratic glyf outlines (simple and
// composite glyphs), cmap formats 4 and 12, and advance widths. It does no
// shaping: the caller passes where each glyph starts, as measured by a
// browser that laid the word out with the same font (kerning included).
// Runs in Node and in a browser page.

export function readFont(bytes) {
  const view = new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength);
  const tables = {};
  for (let i = 0, count = view.getUint16(4); i < count; i++) {
    const at = 12 + 16 * i;
    const tag = String.fromCharCode(view.getUint8(at), view.getUint8(at + 1), view.getUint8(at + 2), view.getUint8(at + 3));
    tables[tag] = view.getUint32(at + 8);
  }
  for (const tag of ["head", "maxp", "hhea", "hmtx", "loca", "glyf", "cmap"]) if (tables[tag] === undefined) throw Error("not a TrueType font: no " + tag);
  const unitsPerEm = view.getUint16(tables.head + 18);
  const longOffsets = view.getInt16(tables.head + 50) === 1;
  const metrics = view.getUint16(tables.hhea + 34);
  const loca = (g) => (longOffsets ? view.getUint32(tables.loca + 4 * g) : view.getUint16(tables.loca + 2 * g) * 2);
  const advance = (g) => view.getUint16(tables.hmtx + 4 * Math.min(g, metrics - 1));

  // The Unicode cmap: format 12 when there is one, else format 4.
  let format4 = -1,
    format12 = -1;
  for (let i = 0, count = view.getUint16(tables.cmap + 2); i < count; i++) {
    const at = tables.cmap + 4 + 8 * i;
    const platform = view.getUint16(at),
      offset = tables.cmap + view.getUint32(at + 4),
      format = view.getUint16(offset);
    if (platform !== 0 && platform !== 3) continue;
    if (format === 12) format12 = offset;
    else if (format === 4 && format4 < 0) format4 = offset;
  }
  function glyphOf(code) {
    if (format12 >= 0) {
      for (let i = 0, groups = view.getUint32(format12 + 12); i < groups; i++) {
        const at = format12 + 16 + 12 * i;
        if (code >= view.getUint32(at) && code <= view.getUint32(at + 4)) return view.getUint32(at + 8) + code - view.getUint32(at);
      }
      return 0;
    }
    if (format4 < 0 || code > 0xffff) return 0;
    const segments = view.getUint16(format4 + 6) / 2;
    const ends = format4 + 14,
      starts = ends + 2 * segments + 2,
      deltas = starts + 2 * segments,
      ranges = deltas + 2 * segments;
    for (let i = 0; i < segments; i++) {
      if (code > view.getUint16(ends + 2 * i)) continue;
      const start = view.getUint16(starts + 2 * i);
      if (code < start) return 0;
      const delta = view.getInt16(deltas + 2 * i),
        range = view.getUint16(ranges + 2 * i);
      if (!range) return (code + delta) & 0xffff;
      const glyph = view.getUint16(ranges + 2 * i + range + 2 * (code - start));
      return glyph ? (glyph + delta) & 0xffff : 0;
    }
    return 0;
  }

  // A glyph's contours: points in font units, y up, each on or off the curve.
  function contours(g, depth = 0) {
    const start = tables.glyf + loca(g),
      end = tables.glyf + loca(g + 1);
    if (start >= end || depth > 8) return [];
    const count = view.getInt16(start);
    if (count >= 0) {
      const ends = [];
      for (let i = 0; i < count; i++) ends.push(view.getUint16(start + 10 + 2 * i));
      const total = (ends.at(-1) ?? -1) + 1;
      let at = start + 10 + 2 * count;
      at += 2 + view.getUint16(at);
      const flags = [];
      while (flags.length < total) {
        const flag = view.getUint8(at++);
        flags.push(flag);
        if (flag & 8) for (let repeat = view.getUint8(at++); repeat > 0; repeat--) flags.push(flag);
      }
      const read = (short, same) => {
        const values = [];
        let value = 0;
        for (const flag of flags) {
          if (flag & short) value += flag & same ? view.getUint8(at++) : -view.getUint8(at++);
          else if (!(flag & same)) {
            value += view.getInt16(at);
            at += 2;
          }
          values.push(value);
        }
        return values;
      };
      const xs = read(2, 16),
        ys = read(4, 32);
      const out = [];
      let first = 0;
      for (const last of ends) {
        const points = [];
        for (let i = first; i <= last; i++) points.push({ x: xs[i], y: ys[i], on: (flags[i] & 1) === 1 });
        out.push(points);
        first = last + 1;
      }
      return out;
    }
    // A composite: other glyphs, moved and possibly scaled.
    const out = [];
    let at = start + 10,
      more = true;
    while (more) {
      const flags = view.getUint16(at),
        part = view.getUint16(at + 2);
      at += 4;
      let dx, dy;
      if (flags & 1) {
        dx = view.getInt16(at);
        dy = view.getInt16(at + 2);
        at += 4;
      } else {
        dx = view.getInt8(at);
        dy = view.getInt8(at + 1);
        at += 2;
      }
      if (!(flags & 2)) dx = dy = 0; // point matching is not supported
      const f2 = (offset) => view.getInt16(offset) / 16384;
      let a = 1,
        b = 0,
        c = 0,
        d = 1;
      if (flags & 8) {
        a = d = f2(at);
        at += 2;
      } else if (flags & 0x40) {
        a = f2(at);
        d = f2(at + 2);
        at += 4;
      } else if (flags & 0x80) {
        a = f2(at);
        b = f2(at + 2);
        c = f2(at + 4);
        d = f2(at + 6);
        at += 8;
      }
      for (const contour of contours(part, depth + 1)) out.push(contour.map(({ x, y, on }) => ({ x: a * x + c * y + dx, y: b * x + d * y + dy, on })));
      more = (flags & 0x20) !== 0;
    }
    return out;
  }
  return { unitsPerEm, glyphOf, advance, contours };
}

const round = (v) => Math.round(v * 100) / 100;

// The contours as path data: scaled, placed at (x, y) and flipped (font
// units go up). Two off-curve points in a row imply an on-curve point midway.
function contourPath(points, scale, x, y) {
  const at = (p) => `${round(x + p.x * scale)} ${round(y - p.y * scale)}`;
  const mid = (p, q) => ({ x: (p.x + q.x) / 2, y: (p.y + q.y) / 2, on: true });
  const n = points.length;
  if (n < 2) return "";
  const first = points.findIndex((p) => p.on);
  const start = first >= 0 ? points[first] : mid(points[n - 1], points[0]);
  const from = first >= 0 ? first + 1 : 0,
    steps = first >= 0 ? n - 1 : n;
  let d = "M" + at(start),
    control = null;
  for (let k = 0; k < steps; k++) {
    const p = points[(from + k) % n];
    if (p.on) {
      d += control ? `Q${at(control)} ${at(p)}` : `L${at(p)}`;
      control = null;
    } else {
      if (control) d += `Q${at(control)} ${at(mid(control, p))}`;
      control = p;
    }
  }
  if (control) d += `Q${at(control)} ${at(start)}`;
  return d + "Z";
}

/**
 * A word as one path. `glyphs` are the font's glyph ids, `offsets` where each
 * starts, in pixels from the word's left edge; `size` is the font size in
 * pixels and (x, y) the word's start on the baseline. Returns the path and
 * the ink's extent: above and below the baseline, left and right of (x, y)
 * (a script's swashes reach past its advance widths).
 */
export function wordPath(font, glyphs, offsets, size, x, y) {
  const scale = size / font.unitsPerEm;
  let d = "",
    top = 0,
    bottom = 0,
    left = Infinity,
    right = -Infinity;
  glyphs.forEach((g, i) => {
    for (const contour of font.contours(g)) {
      d += contourPath(contour, scale, x + offsets[i], y);
      for (const p of contour) {
        top = Math.max(top, p.y * scale);
        bottom = Math.max(bottom, -p.y * scale);
        left = Math.min(left, offsets[i] + p.x * scale);
        right = Math.max(right, offsets[i] + p.x * scale);
      }
    }
  });
  return { d, top: round(top), bottom: round(bottom), left: round(Number.isFinite(left) ? left : 0), right: round(Number.isFinite(right) ? right : 0) };
}
