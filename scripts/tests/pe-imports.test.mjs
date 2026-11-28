import assert from "node:assert/strict";
import fs from "node:fs";
import path from "node:path";
import { test } from "node:test";
import { missingImports, peExports, peImports, verifyComctl32Imports } from "../lib/pe-imports.mjs";

// A PE32+ image with one section at RVA 0x1000 (file offset 0x200) holding
// the import descriptors and the export directory.
function image({ imports = [], exports = null }) {
  const section = Buffer.alloc(0x1000);
  let used = 0;
  const alloc = (size) => {
    const rva = 0x1000 + used;
    used += (size + 7) & ~7;
    return rva;
  };
  const at = (rva) => rva - 0x1000;
  const string = (text) => {
    const rva = alloc(text.length + 1);
    section.write(text, at(rva), "latin1");
    return rva;
  };
  let importRva = 0;
  if (imports.length) {
    importRva = alloc((imports.length + 1) * 20);
    imports.forEach(({ dll, entries }, i) => {
      const table = alloc((entries.length + 1) * 8);
      entries.forEach((entry, j) => {
        let value;
        if (typeof entry === "number") value = (1n << 63n) | BigInt(entry);
        else {
          const hint = alloc(2 + entry.length + 1);
          section.write(entry, at(hint) + 2, "latin1");
          value = BigInt(hint);
        }
        section.writeBigUInt64LE(value, at(table) + j * 8);
      });
      const descriptor = at(importRva) + i * 20;
      section.writeUInt32LE(table, descriptor);
      section.writeUInt32LE(string(dll), descriptor + 12);
      section.writeUInt32LE(table, descriptor + 16);
    });
  }
  let exportRva = 0;
  if (exports) {
    exportRva = alloc(40);
    const functions = alloc(exports.count * 4);
    exports.missing ??= [];
    for (let i = 0; i < exports.count; i++)
      section.writeUInt32LE(exports.missing.includes(exports.base + i) ? 0 : 0x1800, at(functions) + i * 4);
    const named = Object.entries(exports.names);
    const names = alloc(named.length * 4);
    const ordinals = alloc(named.length * 2);
    named.forEach(([name, ordinal], i) => {
      section.writeUInt32LE(string(name), at(names) + i * 4);
      section.writeUInt16LE(ordinal - exports.base, at(ordinals) + i * 2);
    });
    const directory = at(exportRva);
    section.writeUInt32LE(exports.base, directory + 16);
    section.writeUInt32LE(exports.count, directory + 20);
    section.writeUInt32LE(named.length, directory + 24);
    section.writeUInt32LE(functions, directory + 28);
    section.writeUInt32LE(names, directory + 32);
    section.writeUInt32LE(ordinals, directory + 36);
  }
  const headers = Buffer.alloc(0x200);
  headers.writeUInt16LE(0x5a4d, 0);
  headers.writeUInt32LE(0x40, 0x3c);
  headers.writeUInt32LE(0x4550, 0x40);
  headers.writeUInt16LE(0x8664, 0x44);
  headers.writeUInt16LE(1, 0x46);
  headers.writeUInt16LE(240, 0x54);
  const optional = 0x58;
  headers.writeUInt16LE(0x20b, optional);
  headers.writeUInt32LE(16, optional + 108);
  headers.writeUInt32LE(exportRva, optional + 112);
  headers.writeUInt32LE(importRva, optional + 120);
  const sectionHeader = optional + 240;
  headers.writeUInt32LE(0x1000, sectionHeader + 8);
  headers.writeUInt32LE(0x1000, sectionHeader + 12);
  headers.writeUInt32LE(0x1000, sectionHeader + 16);
  headers.writeUInt32LE(0x200, sectionHeader + 20);
  return Buffer.concat([headers, section]);
}

// comctl32.dll 5.82: the subclass functions are ordinals 410-413, and 411
// (GetWindowSubclass) has no name. TaskDialogIndirect (345 in version 6) is absent.
const comctl582 = image({
  exports: {
    base: 340,
    count: 74,
    missing: [345],
    names: { SetWindowSubclass: 410, RemoveWindowSubclass: 412, DefSubclassProc: 413 },
  },
});

test("reads imports by name and by ordinal and the exports they resolve against", () => {
  const modules = peImports(
    image({
      imports: [
        { dll: "COMCTL32.dll", entries: ["SetWindowSubclass", 411, "DefSubclassProc"] },
        { dll: "kernel32.dll", entries: ["GetLastError"] },
      ],
    }),
  );
  assert.deepEqual(modules, [
    {
      dll: "COMCTL32.dll",
      delayed: false,
      imports: [{ name: "SetWindowSubclass" }, { ordinal: 411 }, { name: "DefSubclassProc" }],
    },
    { dll: "kernel32.dll", delayed: false, imports: [{ name: "GetLastError" }] },
  ]);
  const exports = peExports(comctl582);
  assert.ok(exports.ordinals.has(411) && !exports.ordinals.has(345));
  assert.ok(!exports.names.has("GetWindowSubclass"));
  assert.deepEqual(missingImports(modules, "comctl32.dll", exports), []);
  verifyComctl32Imports(image({ imports: [{ dll: "comctl32.dll", entries: [411, "SetWindowSubclass"] }] }), comctl582);
});

test("rejects comctl32 imports that only version 6 provides", () => {
  const executable = image({
    imports: [{ dll: "comctl32.dll", entries: ["GetWindowSubclass", "TaskDialogIndirect", 345, "SetWindowSubclass"] }],
  });
  assert.deepEqual(missingImports(peImports(executable), "comctl32.dll", peExports(comctl582)), [
    "GetWindowSubclass",
    "TaskDialogIndirect",
    "#345",
  ]);
  assert.throws(() => verifyComctl32Imports(executable, comctl582), /GetWindowSubclass, TaskDialogIndirect, #345/);
  assert.throws(() => peImports(Buffer.from("not a PE file")), /Not a PE file/);
});

test("the system comctl32.dll exports GetWindowSubclass only as ordinal 411", { skip: process.platform !== "win32" }, () => {
  const system = peExports(fs.readFileSync(path.join(process.env.SystemRoot ?? "C:\\Windows", "System32", "comctl32.dll")));
  assert.ok(system.ordinals.has(411));
  assert.ok(!system.names.has("GetWindowSubclass"));
  assert.ok(!system.names.has("TaskDialogIndirect"));
  assert.ok(system.names.has("SetWindowSubclass"));
});
