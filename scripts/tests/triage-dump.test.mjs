import assert from "node:assert/strict";
import { test } from "node:test";
import { chain, parseDump, summaryLines } from "../lib/triage-dump.mjs";

const ALPHA = 0xfffff80000100000n;
const BETA = 0xfffff80000200000n;
const TOP = 0xffff800000001000n;

/** A made-up triage dump: two drivers, a stack with three return addresses and one stray value. */
function fakeDump() {
  const d = Buffer.alloc(0x4000);
  d.write("PAGEDU64", 0, "latin1");
  d.writeUInt32LE(26100, 0xc);
  d.writeUInt32LE(4, 0x34);
  d.writeUInt32LE(0x139, 0x38);
  d.writeBigUInt64LE(0x1dn, 0x40);
  d.writeBigUInt64LE(BETA + 0x30n, 0x348 + 0x78 + 16 * 8);
  const T = 0x2000;
  d.writeUInt32LE(0x3000, T + 0x28);
  d.writeUInt32LE(0x40, T + 0x2c);
  d.writeUInt32LE(0x2100, T + 0x30);
  d.writeUInt32LE(2, T + 0x34);
  d.writeBigUInt64LE(TOP, T + 0x48);
  const drivers = [
    ["alpha.sys", ALPHA, 0x10000],
    ["beta.sys", BETA, 0x8000],
  ];
  drivers.forEach(([name, base, size], i) => {
    const entry = 0x2100 + i * 0x98;
    const nameAt = 0x2400 + i * 0x40;
    d.writeUInt32LE(nameAt, entry);
    d.writeUInt32LE(name.length, nameAt);
    d.write(name, nameAt + 4, "utf16le");
    d.writeBigUInt64LE(base, entry + 8 + 0x30);
    d.writeUInt32LE(size, entry + 8 + 0x40);
  });
  [ALPHA + 0x10n, 0x1234n, ALPHA + 0x20n, BETA + 0x5n].forEach((value, i) => d.writeBigUInt64LE(value, 0x3000 + i * 8));
  return d;
}

test("a triage dump gives its bug check, drivers and stack as module+offset", () => {
  const dump = parseDump(fakeDump());
  assert.equal(dump.header.bugCheck, 0x139);
  assert.equal(dump.entrySize, 0x98);
  assert.deepEqual(dump.drivers.map((m) => m.name), ["alpha.sys", "beta.sys"]);
  assert.equal(dump.where(BETA + 0x30n), "beta+0x30");
  assert.equal(dump.where(0x1234n), null);
  assert.deepEqual(dump.stack().map((frame) => frame.at), ["alpha+0x10", "alpha+0x20", "beta+0x5"]);
  assert.deepEqual(dump.stack(TOP + 0x10n).map((frame) => frame.at), ["alpha+0x20", "beta+0x5"]);
  assert.deepEqual(chain(dump.stack()), [
    { module: "alpha", first: "alpha+0x10", count: 2 },
    { module: "beta", first: "beta+0x5", count: 1 },
  ]);

  const lines = summaryLines(dump);
  assert.match(lines[0], /^bug check 0x139 /);
  assert(lines.includes("context rip 0xfffff80000200030 beta+0x30"));
  assert(lines.includes("trap frame not captured"), "the trap frame lies outside the captured memory");
  assert.deepEqual(lines.slice(-2), ["  alpha+0x10 (+1 more in alpha)", "  beta+0x5"]);
});

test("other files are refused", () => {
  assert.throws(() => parseDump(Buffer.from("MDMP....")), /not a 64-bit kernel dump/);
  const short = Buffer.alloc(0x100);
  short.write("PAGEDU64", 0, "latin1");
  assert.throws(() => parseDump(short), /ends before its triage part/);
});
