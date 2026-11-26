import assert from "node:assert/strict";
import { existsSync } from "node:fs";
import { resolve } from "node:path";
import { test } from "node:test";
import { gzipSync } from "node:zlib";
import { formatRows, keeps, manifestVersion, mergePlan, packageFiles, parseDiffStat, readTar, sortRows, totals, VENDORED } from "../lib/vendor-patches.mjs";

const root = resolve(import.meta.dirname, "../..");

// One tar entry: a ustar header (name split into prefix and name past 100
// bytes) and the data padded to 512 bytes.
function entry(name, data = "", type = "0", prefix = "") {
  const header = Buffer.alloc(512);
  header.write(name, 0);
  header.write("0000644\0", 100);
  const body = Buffer.from(data);
  header.write(`${body.length.toString(8).padStart(11, "0")}\0`, 124);
  header.write(type, 156);
  header.write("ustar\u000000", 257);
  header.write(prefix, 345);
  header.fill(" ", 148, 156);
  const sum = header.reduce((total, byte) => total + byte, 0);
  header.write(`${sum.toString(8).padStart(6, "0")}\0 `, 148);
  return Buffer.concat([header, body, Buffer.alloc((512 - (body.length % 512)) % 512)]);
}
const pax = (records) => records.map(([key, value]) => {
  const record = ` ${key}=${value}\n`;
  let length = record.length + 1;
  while (`${length}${record}`.length !== length) length++;
  return `${length}${record}`;
}).join("");
const tar = (...entries) => Buffer.concat([...entries, Buffer.alloc(1024)]);

test("tar entries keep their full names", () => {
  const long = `pkg-1.0.0/src/${"nested/".repeat(20)}deep.rs`;
  const files = readTar(
    tar(
      entry("pkg-1.0.0/", "", "5"),
      entry("pkg-1.0.0/Cargo.toml", '[package]\nversion = "1.0.0"\n'),
      entry("././@LongLink", `${long}\0`, "L"),
      entry(long.slice(0, 99), "fn deep() {}\n"),
      entry("PaxHeader", pax([["mtime", "0"], ["path", "pkg-1.0.0/src/pax.rs"]]), "x"),
      entry("truncated", "pax\n"),
      entry("lib.rs", "x".repeat(700), "0", "pkg-1.0.0/src"),
    ),
  );
  assert.deepEqual(files.map((file) => file.path), ["pkg-1.0.0/Cargo.toml", long, "pkg-1.0.0/src/pax.rs", "pkg-1.0.0/src/lib.rs"]);
  assert.equal(files[1].data.toString(), "fn deep() {}\n");
  assert.equal(files[3].data.length, 700, "data spanning two blocks is read whole");
  assert.equal(files[0].mode, 0o644);
  assert.throws(() => readTar(tar(entry("link", "", "2"))), /unsupported entry type/);
  assert.throws(() => readTar(entry("cut", "abc").subarray(0, 514)), /truncated/);
});

test("a package keeps what its snapshot keeps", () => {
  const spec = { name: "pkg", omit: ["Cargo.lock", "Cargo.toml.orig", "examples/"], keep: ["examples/fixture.jpg"] };
  assert.equal(keeps(spec, "src/lib.rs"), true);
  assert.equal(keeps(spec, "Cargo.lock"), false);
  assert.equal(keeps(spec, "src/Cargo.lock"), true, "a rule without a slash names one path");
  assert.equal(keeps(spec, "examples/hello.rs"), false);
  assert.equal(keeps(spec, "examples/fixture.jpg"), true);
  assert.equal(keeps(spec, "examples-more/a.rs"), true);
  const crate = gzipSync(
    tar(
      entry("pkg-1.0.0/src/lib.rs", "lib"),
      entry("pkg-1.0.0/Cargo.lock", "lock"),
      entry("pkg-1.0.0/examples/hello.rs", "example"),
      entry("pkg-1.0.0/examples/fixture.jpg", "jpg"),
      entry("pkg-1.0.0/Cargo.toml", "manifest"),
    ),
  );
  assert.deepEqual(packageFiles(crate, spec, "1.0.0").map((file) => [file.path, file.data.toString()]), [
    ["Cargo.toml", "manifest"],
    ["examples/fixture.jpg", "jpg"],
    ["src/lib.rs", "lib"],
  ]);
  assert.throws(() => packageFiles(crate, spec, "1.0.1"), /outside pkg-1\.0\.1\//);
  assert.throws(() => packageFiles(gzipSync(tar(entry("pkg-1.0.0/../evil", "x"))), spec, "1.0.0"), /unsafe path/);
});

test("the vendored packages are the folders under vendor/", () => {
  for (const spec of VENDORED) {
    assert(existsSync(resolve(root, "vendor", spec.name, "Cargo.toml")), spec.name);
    assert.equal(keeps(spec, "Cargo.toml"), true);
    assert.equal(keeps(spec, "Cargo.toml.orig"), false);
  }
});

test("the version comes from the package table", () => {
  const manifest = '[workspace]\nversion = "9"\n\n[package]\nname = "gpui-pre"\nversion = "0.3.12"\n\n[dependencies.x]\nversion = "1"\n';
  assert.equal(manifestVersion(manifest), "0.3.12");
  assert.equal(manifestVersion(manifest.replaceAll("\n", "\r\n")), "0.3.12");
  assert.equal(manifestVersion('[dependencies]\nversion = "1"\n'), null);
});

test("diff statistics take their status from the name status", () => {
  const numstat = ["12\t3\tvendor/a/src/x.rs", "40\t0\tvendor/a/src/new.rs", "-\t-\tvendor/b/image.jpg", "1\t1\tvendor/b/tab\tname.rs", ""].join("\0");
  const nameStatus = ["M", "vendor/a/src/x.rs", "A", "vendor/a/src/new.rs", "M", "vendor/b/image.jpg", "M", "vendor/b/tab\tname.rs", ""].join("\0");
  const rows = parseDiffStat(numstat, nameStatus);
  assert.deepEqual(rows[1], { path: "vendor/a/src/new.rs", status: "A", added: 40, removed: 0 });
  assert.deepEqual(rows[2], { path: "vendor/b/image.jpg", status: "M", added: null, removed: null });
  assert.equal(rows[3].path, "vendor/b/tab\tname.rs");
  const sorted = sortRows(rows);
  assert.deepEqual(sorted.map((row) => row.path), ["vendor/a/src/new.rs", "vendor/a/src/x.rs", "vendor/b/tab\tname.rs", "vendor/b/image.jpg"]);
  assert.deepEqual([...totals(rows)], [
    ["a", { files: 2, added: 52, removed: 3 }],
    ["b", { files: 2, added: 1, removed: 1 }],
    ["", { files: 4, added: 53, removed: 4 }],
  ]);
  assert.deepEqual(formatRows(sorted.slice(0, 2)), ["40  0  A  vendor/a/src/new.rs", "12  3  M  vendor/a/src/x.rs"]);
  assert.deepEqual(formatRows([rows[2]]), ["- -  M  vendor/b/image.jpg"]);
});

test("an upgrade merges only what both sides changed", () => {
  const base = new Map([
    ["unpatched", "b1"],
    ["patched", "b2"],
    ["patched-untouched", "b3"],
    ["dropped-upstream", "b4"],
    ["dropped-patched", "b5"],
    ["deleted-locally", "b6"],
    ["same-change", "b7"],
  ]);
  const ours = new Map([
    ["unpatched", "b1"],
    ["patched", "o2"],
    ["patched-untouched", "o3"],
    ["dropped-upstream", "b4"],
    ["dropped-patched", "o5"],
    ["same-change", "t7"],
    ["local", "o8"],
    ["both-added", "o9"],
  ]);
  const theirs = new Map([
    ["unpatched", "t1"],
    ["patched", "t2"],
    ["patched-untouched", "b3"],
    ["deleted-locally", "t6"],
    ["same-change", "t7"],
    ["new-upstream", "t10"],
    ["both-added", "t9"],
  ]);
  assert.deepEqual(mergePlan(base, ours, theirs), [
    { path: "both-added", action: "conflict", reason: "added both upstream and locally" },
    { path: "deleted-locally", action: "conflict", reason: "removed locally, changed upstream" },
    { path: "dropped-patched", action: "conflict", reason: "removed upstream, patched locally" },
    { path: "dropped-upstream", action: "take", deleted: true },
    { path: "new-upstream", action: "take", deleted: false },
    { path: "patched", action: "merge" },
    { path: "unpatched", action: "take", deleted: false },
  ]);
});
