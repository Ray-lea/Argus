import assert from "node:assert/strict";
import { test } from "node:test";
import { mkdirSync, mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { deflateSync } from "node:zlib";
import { spawnSync } from "node:child_process";
import { differences, parityDifferences, pixels } from "../lib/capture-pixels.mjs";

// An RGBA PNG from rows of [r, g, b, a] pixels; `filters` gives each row's PNG filter (0 or 1).
function png(rows, filters = rows.map(() => 0)) {
  const width = rows[0].length;
  const raw = Buffer.concat(
    rows.map((row, y) => {
      const bytes = row.flat();
      const line = filters[y] === 1 ? bytes.map((value, i) => (value - (i >= 4 ? bytes[i - 4] : 0)) & 255) : bytes;
      return Buffer.from([filters[y], ...line]);
    }),
  );
  const chunk = (type, data) => {
    const head = Buffer.alloc(8);
    head.writeUInt32BE(data.length, 0);
    head.write(type, 4, "ascii");
    return Buffer.concat([head, data, Buffer.alloc(4)]);
  };
  const header = Buffer.alloc(13);
  header.writeUInt32BE(width, 0);
  header.writeUInt32BE(rows.length, 4);
  header.set([8, 6, 0, 0, 0], 8);
  return Buffer.concat([Buffer.from([137, 80, 78, 71, 13, 10, 26, 10]), chunk("IHDR", header), chunk("IDAT", deflateSync(raw)), chunk("IEND", Buffer.alloc(0))]);
}

const grey = (value) => [value, value, value, 255];
const image = (edit = () => {}) => {
  const rows = Array.from({ length: 6 }, () => Array.from({ length: 5 }, () => grey(40)));
  edit(rows);
  return rows;
};

test("captures decode and their differences are located", () => {
  const dir = mkdtempSync(join(tmpdir(), "argus-pixels-"));
  try {
    const a = join(dir, "a.png");
    const b = join(dir, "b.png");
    writeFileSync(a, png(image(), [0, 1, 0, 1, 0, 1]));
    writeFileSync(
      b,
      png(
        image((rows) => {
          rows[1][2] = grey(43);
          rows[2][3] = [40, 52, 40, 255];
          rows[5][1] = [40, 40, 40, 0];
        }),
      ),
    );
    const left = pixels(a);
    assert.deepEqual([left.width, left.height, left.channels], [5, 6, 4]);
    assert.deepEqual([...left.data.subarray(0, 4)], [40, 40, 40, 255]);
    assert.deepEqual(differences(left, pixels(a)), { changedPixels: 0, maxChannelDelta: 0, box: null, rows: [] });
    // Alpha alone is no difference; rows 1 and 2 hold the two changed pixels.
    assert.deepEqual(differences(left, pixels(b)), { changedPixels: 2, maxChannelDelta: 12, box: [2, 1, 2, 2], rows: [[1, 2]] });
    assert.deepEqual(differences(left, pixels(b), 3), { changedPixels: 1, maxChannelDelta: 12, box: [3, 2, 1, 1], rows: [[2, 2]] });
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
});

test("two sets of runs agree when any pair of them shows the same picture", () => {
  const dir = mkdtempSync(join(tmpdir(), "argus-parity-"));
  try {
    const at = (run) => join(dir, run);
    const write = (run, name, rows) => {
      mkdirSync(at(run), { recursive: true });
      writeFileSync(join(at(run), `${name}.png`), png(rows));
    };
    const hovered = image((rows) => (rows[3] = rows[3].map(() => grey(50))));
    const shifted = image((rows) => (rows[1][1] = grey(90)));
    for (const run of ["ours-1", "ours-2", "theirs-1", "theirs-2"]) write(run, "page", image());
    write("ours-1", "menu", image());
    write("theirs-1", "menu", hovered);
    assert.deepEqual(parityDifferences([at("ours-1")], [at("theirs-1")]), { compared: 2, differ: [{ name: "menu.png", changedPixels: 5, maxChannelDelta: 10, box: [0, 3, 5, 1], rows: [[3, 3]] }], missing: [] });
    // A second attempt on both sides: one clean run of theirs is enough.
    write("ours-2", "menu", hovered);
    write("theirs-2", "menu", image());
    assert.deepEqual(parityDifferences([at("ours-1"), at("ours-2")], [at("theirs-1"), at("theirs-2")]), { compared: 2, differ: [], missing: [] });
    // A real difference stays one however often it is taken, and a capture the other side lacks is named.
    write("ours-1", "list", image());
    write("ours-2", "list", image());
    write("theirs-1", "list", shifted);
    write("theirs-2", "list", shifted);
    write("ours-1", "only", image());
    const result = parityDifferences([at("ours-1"), at("ours-2")], [at("theirs-1"), at("theirs-2")]);
    assert.deepEqual(result.differ.map((entry) => [entry.name, entry.changedPixels, entry.maxChannelDelta]), [["list.png", 1, 50]]);
    assert.deepEqual(result.missing, ["only.png"]);
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
});

test("compare-shots passes a capture that any old run matches", () => {
  const dir = mkdtempSync(join(tmpdir(), "argus-shots-"));
  try {
    const at = (run) => join(dir, run);
    const write = (run, name, rows) => {
      mkdirSync(at(run), { recursive: true });
      writeFileSync(join(at(run), `${name}.png`), png(rows));
    };
    const hovered = image((rows) => (rows[3] = rows[3].map(() => grey(50))));
    write("new", "page", image());
    write("new", "menu", hovered);
    write("old-1", "page", hovered);
    write("old-1", "menu", hovered);
    write("old-2", "page", image());
    const run = (...args) => spawnSync(process.execPath, [join(import.meta.dirname, "../compare-shots.mjs"), ...args], { encoding: "utf8" });

    const both = run(at("new"), at("old-1"), at("old-2"));
    assert.equal(both.status, 0, both.stdout + both.stderr);
    assert.match(both.stdout, /2 captures: 2 identical, 0 differ/);

    const one = run(at("new"), at("old-1"));
    assert.equal(one.status, 1);
    assert.match(one.stdout, /1 identical, 1 differ/);
    assert.deepEqual(JSON.parse(one.stdout.split("\n")[1]), { name: "page.png", against: "old-1", changedPixels: 5, maxChannelDelta: 10, box: [0, 3, 5, 1], rows: [[3, 3]] });

    const tolerant = run(at("new"), at("old-1"), "--tolerance", "10");
    assert.equal(tolerant.status, 0, tolerant.stdout);

    const absent = run(at("new"), at("old-2"), "--only", "menu");
    assert.equal(absent.status, 1);
    assert.match(absent.stdout, /in no old run: menu\.png/);
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
});
