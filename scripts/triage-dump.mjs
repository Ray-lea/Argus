// A first look at a user's kernel crash dump without a debugger: bug check,
// faulting address and the driver chain on the crashed thread's stack.
// Reads only the file; what it prints names the reporter's drivers and
// processes, so it stays out of issues and commits. Usage and reading:
// docs/development/README.md#分析崩溃转储.
//
//   node scripts/triage-dump.mjs <dump> [summary]
//   node scripts/triage-dump.mjs <dump> header | drivers [regex] | stamps [regex] | blocks
//   node scripts/triage-dump.mjs <dump> stack [from] | trap | process
//   node scripts/triage-dump.mjs <dump> mem <address> [length] | find <text> | scan <qword> | tail
import { readFileSync } from "node:fs";
import { hex, parseDump, summaryLines } from "./lib/triage-dump.mjs";

const [file, command = "summary", ...rest] = process.argv.slice(2);
if (!file) {
  console.error("usage: node scripts/triage-dump.mjs <dump> [summary|header|drivers|stamps|blocks|stack|trap|process|mem|find|scan|tail]");
  process.exit(2);
}
const d = readFileSync(file);
let dump;
try {
  dump = parseDump(d);
} catch (error) {
  console.error(error.message);
  process.exit(1);
}
const { header, triage, context, where } = dump;
const filter = (value) => (value ? new RegExp(value, "i") : null);

switch (command) {
  case "summary":
    console.log(summaryLines(dump).join("\n"));
    break;
  case "header":
    console.log("build", header.build, "major", hex(header.major), "processors", header.processors, "machine", hex(header.machine));
    console.log("bug check", hex(header.bugCheck), header.parameters.map((p) => hex(p, 16)).join(" "));
    console.log("dump type", header.dumpType, "file size", d.length, "system time (UTC)", header.systemTime, "uptime (s)", header.uptimeSeconds);
    if (header.exception) {
      const e = header.exception;
      console.log("exception", hex(e.code), "flags", hex(e.flags), "address", hex(e.address, 16), where(e.address) ?? "", "parameters", e.parameters, hex(e.first));
    }
    console.log("context", Object.entries(context).map(([name, value]) => `${name}=${hex(value, 16)}`).join(" "));
    console.log("context rip", where(context.rip));
    console.log("triage", Object.entries(triage).map(([name, value]) => `${name}=${typeof value === "bigint" ? hex(value, 16) : hex(value)}`).join(" "));
    break;
  case "drivers": {
    const only = filter(rest[0]);
    for (const m of dump.modules) if (!only || only.test(m.name)) console.log(hex(m.base, 16), hex(m.size, 8), m.name);
    console.log("count", dump.modules.length, "entry size", hex(dump.entrySize));
    break;
  }
  case "stamps": {
    // Link time and checksum tell a driver's version when the reporter cannot.
    const only = filter(rest[0]);
    for (const m of dump.drivers)
      if (!only || only.test(m.name))
        console.log(m.name, "size", hex(m.size), "checksum", hex(m.checksum), "stamp", hex(m.stamp), new Date(m.stamp * 1000).toISOString());
    break;
  }
  case "blocks":
    for (const block of dump.memory) console.log(hex(block.address, 16), hex(block.size), "file", hex(block.offset));
    break;
  case "stack":
    for (const frame of dump.stack(rest[0] ? BigInt(rest[0]) : undefined)) console.log(hex(frame.slot, 16), hex(frame.value, 16), frame.at);
    break;
  case "trap": {
    const { frame, record } = dump.trap();
    console.log(frame ? `trap frame ${Object.entries(frame).map(([name, value]) => `${name}=${hex(value, 16)}`).join(" ")}` : "trap frame not captured");
    if (frame) console.log("trap rip", where(frame.rip));
    console.log(record ? `exception record ${hex(record.code)} at ${hex(record.address, 16)} ${where(record.address) ?? ""} first ${hex(record.first)}` : "exception record not captured");
    break;
  }
  case "process":
    for (const { offset, text } of dump.processNames()) console.log(hex(offset), text);
    break;
  case "mem": {
    const address = BigInt(rest[0]);
    const data = dump.read(address, Number(rest[1] ?? 0x100));
    if (!data) {
      console.log("not captured");
      break;
    }
    for (let o = 0; o + 8 <= data.length; o += 16) {
      const a = data.readBigUInt64LE(o);
      const b = o + 16 <= data.length ? data.readBigUInt64LE(o + 8) : 0n;
      const text = data.toString("latin1", o, Math.min(o + 16, data.length)).replace(/[^\x20-\x7e]/g, ".");
      console.log(hex(address + BigInt(o), 16), hex(a, 16), hex(b, 16), text, where(a) ?? "", where(b) ?? "");
    }
    break;
  }
  case "find":
    for (const [label, pattern] of [
      ["ascii", Buffer.from(rest[0], "latin1")],
      ["utf16", Buffer.from(rest[0], "utf16le")],
    ]) {
      let at = -1;
      for (let hits = 0; (at = d.indexOf(pattern, at + 1)) !== -1 && hits < 12; hits++) console.log(label, hex(at));
    }
    break;
  case "scan": {
    // File offsets of every 8-byte-aligned occurrence of a value.
    const value = BigInt(rest[0]);
    for (let o = 0, hits = 0; o + 8 <= d.length && hits < 40; o += 8)
      if (d.readBigUInt64LE(o) === value) {
        console.log(hex(o));
        hits++;
      }
    break;
  }
  case "tail": {
    const end = triage.sizeOfDump;
    console.log("triage ends", hex(end), "file", hex(d.length));
    console.log(d.subarray(end, end + 0x80).toString("hex").replace(/(.{32})/g, "$1\n"));
    break;
  }
  default:
    console.error(`unknown command ${command}`);
    process.exit(2);
}
