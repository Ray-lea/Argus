// Decode owned RGB/RGBA window captures to distinguish chart rounding from static ink.
import { readFileSync } from "node:fs";
import { inflateSync } from "node:zlib";
import assert from "node:assert/strict";

function pixels(path) {
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
