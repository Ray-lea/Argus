// Decode owned RGB/RGBA window captures to distinguish chart rounding from static ink.
import { existsSync, readdirSync, readFileSync } from "node:fs";
import { join } from "node:path";
import { inflateSync } from "node:zlib";
import assert from "node:assert/strict";

/** An 8-bit RGB or RGBA PNG as `{ width, height, channels, data }`, rows top to bottom. */
export function pixels(path) {
  const bytes = readFileSync(path), parts = [];
  let width, height, channels;
  for (let at = 8; at < bytes.length;) {
    const length = bytes.readUInt32BE(at), type = bytes.toString("ascii", at + 4, at + 8);
    const data = bytes.subarray(at + 8, at + 8 + length); at += length + 12;
    if (type === "IHDR") {
      width = data.readUInt32BE(0); height = data.readUInt32BE(4);
      channels = data[9] === 2 ? 3 : data[9] === 6 ? 4 : 0;
      assert(data[8] === 8 && channels && data[12] === 0, "unsupported capture encoding");
    }
    if (type === "IDAT") parts.push(data);
  }
  const stride = width * channels;
  assert(width > 0 && height > 0 && stride * height <= 128 * 1024 ** 2, "invalid capture dimensions");
  const raw = inflateSync(Buffer.concat(parts), { maxOutputLength: (stride + 1) * height });
  assert.equal(raw.length, (stride + 1) * height);
  const result = Buffer.alloc(stride * height);
  for (let y = 0; y < height; y++) {
    const filter = raw[y * (stride + 1)]; assert(filter <= 4, "invalid PNG filter");
    for (let x = 0; x < stride; x++) {
      const at = y * stride + x, a = x >= channels ? result[at - channels] : 0;
      const b = y ? result[at - stride] : 0, c = y && x >= channels ? result[at - stride - channels] : 0;
      let predict = 0;
      if (filter === 1) predict = a;
      if (filter === 2) predict = b;
      if (filter === 3) predict = (a + b) >> 1;
      if (filter === 4) {
        const p = a + b - c, pa = Math.abs(p - a), pb = Math.abs(p - b), pc = Math.abs(p - c);
        predict = pa <= pb && pa <= pc ? a : pb <= pc ? b : c;
      }
      result[at] = (raw[y * (stride + 1) + 1 + x] + predict) & 255;
    }
  }
  return { width, height, channels, data: result };
}

/**
 * Where two decoded captures differ in colour (alpha is ignored): the count of
 * changed pixels, the largest channel difference, the box around them as
 * [x, y, width, height], and the runs of rows that hold any, as [first, last].
 * `tolerance` is the largest channel difference that still counts as equal.
 */
export function differences(a, b, tolerance = 0) {
  assert(a.width === b.width && a.height === b.height, "capture dimensions differ");
  let changedPixels = 0, maxChannelDelta = 0;
  let left = a.width, top = a.height, right = -1, bottom = -1;
  const rows = [];
  for (let y = 0; y < a.height; y++) {
    let any = false;
    for (let x = 0; x < a.width; x++) {
      let delta = 0;
      for (let channel = 0; channel < 3; channel++) delta = Math.max(delta, Math.abs(a.data[(y * a.width + x) * a.channels + channel] - b.data[(y * b.width + x) * b.channels + channel]));
      if (delta <= tolerance) continue;
      changedPixels++; maxChannelDelta = Math.max(maxChannelDelta, delta); any = true;
      left = Math.min(left, x); right = Math.max(right, x);
    }
    if (!any) continue;
    top = Math.min(top, y); bottom = y;
    if (rows.length && rows.at(-1)[1] === y - 1) rows.at(-1)[1] = y; else rows.push([y, y]);
  }
  return { changedPixels, maxChannelDelta, box: changedPixels ? [left, top, right - left + 1, bottom - top + 1] : null, rows };
}

const captures = (dir) => readdirSync(dir).filter((name) => name.endsWith(".png")).sort();

/**
 * Compares the captures of two sets of runs that should look the same, each
 * set a list of folders with the same capture names. A capture passes when
 * any run of one set equals any run of the other: something outside the
 * comparison (a curve moving with real time) may mark one run of either. Returns the captures that never matched, each with its closest
 * pair, and those the other set does not have at all.
 */
export function parityDifferences(ours, theirs, tolerance = 0) {
  const differ = [];
  const missing = [];
  for (const name of captures(ours[0])) {
    let closest;
    for (const mine of ours) {
      if (closest?.changedPixels === 0) break;
      if (!existsSync(join(mine, name))) continue;
      const image = pixels(join(mine, name));
      for (const other of theirs) {
        if (!existsSync(join(other, name))) continue;
        let result;
        try {
          result = differences(image, pixels(join(other, name)), tolerance);
        } catch (cause) {
          result = { error: cause.message, changedPixels: Infinity };
        }
        if (!closest || result.changedPixels < closest.changedPixels) closest = { name, ...result, rows: result.rows?.slice(0, 12) };
        if (result.changedPixels === 0) break;
      }
    }
    if (!closest) missing.push(name);
    else if (closest.changedPixels !== 0) differ.push(closest);
  }
  return { compared: captures(ours[0]).length, differ, missing };
}

export function compareChartPixels(left, right, charts, scale) {
  const a = pixels(left), b = pixels(right);
  assert(a.width === b.width && a.height === b.height, "capture dimensions differ");
  // WGC includes the one-device-pixel nonclient edge around the owned client.
  const regions = charts.map(({ clip: [x, y, w, h] }) => [x * scale - 2, y * scale - 2, (x + w) * scale + 2, (y + h) * scale + 2]);
  let changedPixels = 0, maxChannelDelta = 0, maxOutsideCharts = 0;
  for (let y = 0; y < a.height; y++) for (let x = 0; x < a.width; x++) {
    let delta = 0;
    for (let channel = 0; channel < 3; channel++) delta = Math.max(delta, Math.abs(a.data[(y * a.width + x) * a.channels + channel] - b.data[(y * b.width + x) * b.channels + channel]));
    if (!delta) continue;
    changedPixels++; maxChannelDelta = Math.max(maxChannelDelta, delta);
    if (!regions.some(([l, t, r, d]) => x >= l && x < r && y >= t && y < d)) maxOutsideCharts = Math.max(maxOutsideCharts, delta);
  }
  return { width: a.width, height: a.height, changedPixels, maxChannelDelta, maxOutsideCharts };
}
