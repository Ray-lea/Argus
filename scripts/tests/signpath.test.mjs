import assert from "node:assert/strict";
import { test } from "node:test";
import { peHeaders, verifySignedPayload } from "../signpath.mjs";

function executable(magic = 0x20b, length = 513) {
  const data = Buffer.alloc(length, 0x31);
  data.writeUInt16LE(0x5a4d, 0);
  data.writeUInt32LE(0x80, 0x3c);
  data.writeUInt32LE(0x4550, 0x80);
  data.writeUInt16LE(magic, 0x98);
  const directories = 0x98 + (magic === 0x20b ? 112 : 96);
  data.fill(0, directories + 32, directories + 40);
  return data;
}

function signed(original) {
  const { checksum, security } = peHeaders(original);
  const certificate = Math.ceil(original.length / 8) * 8;
  const data = Buffer.alloc(certificate + 16);
  original.copy(data);
  data.writeUInt32LE(12345, checksum);
  data.writeUInt32LE(certificate, security);
  data.writeUInt32LE(16, security + 4);
  data.writeUInt32LE(16, certificate);
  data.writeUInt16LE(0x200, certificate + 4);
  data.writeUInt16LE(2, certificate + 6);
  return data;
}

test("signing may append a certificate and align a 32-bit or 64-bit PE", () => {
  for (const magic of [0x10b, 0x20b]) {
    for (const size of [512, 513, 519]) {
      const input = executable(magic, size);
      assert.doesNotThrow(() => verifySignedPayload(input, signed(input)));
    }
  }
});

test("changed executable bytes, overlays and nonzero alignment padding are rejected", () => {
  const input = executable();
  for (const offset of [2, 0x180, input.length - 1, input.length]) {
    const output = signed(input);
    output[offset] ^= 1;
    assert.throws(() => verifySignedPayload(input, output));
  }
});

test("a removed, overlapping, oversized or truncated certificate is rejected", () => {
  const input = executable();
  const header = peHeaders(input);
  for (const [offset, size] of [[0, 0], [512, 16], [528, 0], [520, 10000]]) {
    const output = signed(input);
    output.writeUInt32LE(offset, header.security);
    output.writeUInt32LE(size, header.security + 4);
    assert.throws(() => verifySignedPayload(input, output));
  }
  assert.throws(() => verifySignedPayload(input, signed(input).subarray(0, 520)));
  assert.throws(() => verifySignedPayload(signed(input), signed(input)));
});

test("an invalid or truncated PE header fails before file replacement", () => {
  for (const input of [Buffer.alloc(2), Buffer.alloc(64), executable().subarray(0, 180)]) {
    assert.throws(() => peHeaders(input));
  }
  const input = executable();
  input.writeUInt32LE(0xfffffff0, 0x3c);
  assert.throws(() => peHeaders(input));
});
