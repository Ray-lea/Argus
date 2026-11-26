// Reads a Windows x64 kernel triage dump (the minidump Windows writes at a bug
// check, usually C:\Windows\Minidump\*.dmp) without a debugger: the bug check
// and its parameters, the processor context and trap frame, the loaded
// drivers, and the return addresses on the faulting thread's stack as
// module+offset. Function names need symbols and a debugger; module+offset is
// enough to tell which driver chain a crash went through.
//
// Layout: DUMP_HEADER64 (signature "PAGEDU64") and, at 0x2000, TRIAGE_DUMP64.
// Offsets are those of Windows 10 and 11; the driver entry size differs
// between builds and is found from the entries themselves.

const TRIAGE = 0x2000;
const CONTEXT = 0x348;
const REGISTERS = ["rax", "rcx", "rdx", "rbx", "rsp", "rbp", "rsi", "rdi", "r8", "r9", "r10", "r11", "r12", "r13", "r14", "r15", "rip"];
const ENTRY_SIZES = [0x98, 0xa0, 0xa8, 0xb0, 0xb8, 0xc0, 0x90, 0x88];
const KERNEL = 0xfffffn;

export const hex = (value, width = 0) => `0x${value.toString(16).padStart(width, "0")}`;

function filetime(value) {
  const ms = Number(value / 10000n) - 11644473600000;
  return Number.isFinite(ms) && ms > 0 ? new Date(ms).toISOString() : null;
}

/** Printable runs of at least `least` characters in [from, to), as [offset, text]. */
function strings(d, from, to, least = 5) {
  const out = [];
  let start = -1;
  for (let i = from; i <= to; i++) {
    const c = i < to ? d[i] : 0;
    if (c >= 0x20 && c < 0x7f) {
      if (start < 0) start = i;
    } else {
      if (start >= 0 && i - start >= least) out.push([start, d.toString("latin1", start, i)]);
      start = -1;
    }
  }
  return out;
}

/** Parses a triage dump held in `d` (a Buffer). Throws when it is not one. */
export function parseDump(d) {
  const signature = d.toString("latin1", 0, 8);
  if (signature !== "PAGEDU64") throw new Error(`not a 64-bit kernel dump (signature ${JSON.stringify(signature)})`);
  if (d.length < TRIAGE + 0x60) throw new Error("the dump ends before its triage part");
  const u32 = (o) => d.readUInt32LE(o);
  const u64 = (o) => d.readBigUInt64LE(o);
  const inside = (o, n) => o > 0 && o + n <= d.length;

  const T = TRIAGE;
  const triage = {
    sizeOfDump: u32(T + 4),
    validOffset: u32(T + 8),
    contextOffset: u32(T + 0xc),
    exceptionOffset: u32(T + 0x10),
    mmOffset: u32(T + 0x14),
    unloadedDriversOffset: u32(T + 0x18),
    prcbOffset: u32(T + 0x1c),
    processOffset: u32(T + 0x20),
    threadOffset: u32(T + 0x24),
    callStackOffset: u32(T + 0x28),
    sizeOfCallStack: u32(T + 0x2c),
    driverListOffset: u32(T + 0x30),
    driverCount: u32(T + 0x34),
    stringPoolOffset: u32(T + 0x38),
    stringPoolSize: u32(T + 0x3c),
    brokenDriverOffset: u32(T + 0x40),
    triageOptions: u32(T + 0x44),
    topOfStack: u64(T + 0x48),
    debuggerDataOffset: u32(T + 0x50),
    debuggerDataSize: u32(T + 0x54),
    dataBlocksOffset: u32(T + 0x58),
    dataBlocksCount: u32(T + 0x5c),
  };

  const header = {
    major: u32(8),
    build: u32(0xc),
    machine: u32(0x30),
    processors: u32(0x34),
    bugCheck: u32(0x38),
    parameters: [0x40, 0x48, 0x50, 0x58].map(u64),
    dumpType: d.length >= 0xf9c ? u32(0xf98) : null,
    systemTime: d.length >= 0xfb0 ? filetime(u64(0xfa8)) : null,
    uptimeSeconds: d.length >= 0x1038 ? Number(u64(0x1030) / 10000000n) : null,
    exception:
      d.length >= 0xf28 ? { code: u32(0xf00), flags: u32(0xf04), address: u64(0xf10), parameters: u32(0xf18), first: u64(0xf20) } : null,
  };
  const context = Object.fromEntries(REGISTERS.map((name, i) => [name, u64(CONTEXT + 0x78 + i * 8)]));

  // Each entry: a string-pool offset, then an LDR_DATA_TABLE_ENTRY at +8.
  const drivers = [];
  let entrySize = 0;
  const { driverListOffset: base, driverCount: count } = triage;
  if (base && count) {
    entrySize =
      ENTRY_SIZES.find((size) => {
        for (let i = 0; i < Math.min(count, 12); i++) {
          const at = base + i * size;
          if (!inside(at, size) || u64(at + 8 + 0x30) >> 44n !== KERNEL) return false;
        }
        return true;
      }) ?? 0;
    for (let i = 0; entrySize && i < count; i++) {
      const at = base + i * entrySize;
      if (!inside(at, entrySize)) break;
      const nameAt = u32(at);
      let name = "?";
      if (inside(nameAt, 4)) {
        const length = u32(nameAt);
        if (length < 260 && inside(nameAt + 4, length * 2)) name = d.toString("utf16le", nameAt + 4, nameAt + 4 + length * 2);
      }
      const entry = at + 8;
      drivers.push({
        name,
        base: u64(entry + 0x30),
        size: u32(entry + 0x40),
        checksum: inside(entry + 0x78, 4) ? u32(entry + 0x78) : 0,
        stamp: inside(entry + 0x80, 4) ? u32(entry + 0x80) : 0,
      });
    }
  }
  const modules = [...drivers].sort((a, b) => (a.base < b.base ? -1 : a.base > b.base ? 1 : 0));

  // Captured memory: the data blocks and the faulting thread's stack.
  const memory = [];
  for (let i = 0; i < triage.dataBlocksCount; i++) {
    const at = triage.dataBlocksOffset + i * 16;
    if (!inside(at, 16)) break;
    memory.push({ address: u64(at), offset: u32(at + 8), size: u32(at + 12) });
  }
  if (triage.sizeOfCallStack && inside(triage.callStackOffset, triage.sizeOfCallStack))
    memory.push({ address: triage.topOfStack, offset: triage.callStackOffset, size: triage.sizeOfCallStack });

  /** `length` bytes of captured memory at a kernel address, or null. */
  function read(address, length) {
    for (const block of memory) {
      if (address >= block.address && address + BigInt(length) <= block.address + BigInt(block.size)) {
        const at = block.offset + Number(address - block.address);
        return inside(at, length) ? d.subarray(at, at + length) : null;
      }
    }
    return null;
  }

  /** `module+0xoffset` for an address inside a loaded image, or null. */
  function where(address) {
    for (const m of modules)
      if (address >= m.base && address < m.base + BigInt(m.size)) return `${m.name.replace(/\.(sys|exe|dll)$/i, "")}+${hex(address - m.base)}`;
    return null;
  }

  /** Every stack slot from `from` up that points into a loaded image: return-address candidates, innermost first. */
  function stack(from = triage.topOfStack) {
    const out = [];
    if (!triage.sizeOfCallStack || !inside(triage.callStackOffset, triage.sizeOfCallStack)) return out;
    for (let o = 0; o + 8 <= triage.sizeOfCallStack; o += 8) {
      const slot = triage.topOfStack + BigInt(o);
      if (slot < from) continue;
      const value = u64(triage.callStackOffset + o);
      const at = where(value);
      if (at) out.push({ slot, value, at });
    }
    return out;
  }

  /** Bug check 0x139: parameter 2 is the trap frame, parameter 3 the exception record. */
  function trap() {
    const frame = read(header.parameters[1], 0x190);
    const record = read(header.parameters[2], 0x30);
    const q = (o) => frame.readBigUInt64LE(o);
    const offsets = { rax: 0x30, rcx: 0x38, rdx: 0x40, r8: 0x48, r9: 0x50, r10: 0x58, r11: 0x60, rbx: 0x140, rdi: 0x148, rsi: 0x150, rbp: 0x158, rip: 0x168, rsp: 0x180 };
    return {
      frame: frame ? Object.fromEntries(Object.entries(offsets).map(([name, o]) => [name, q(o)])) : null,
      record: record ? { code: record.readUInt32LE(0), address: record.readBigUInt64LE(0x10), first: record.readBigUInt64LE(0x20) } : null,
    };
  }

  /** Image-name-like strings of the process block: which program's thread crashed. */
  function processNames() {
    const { processOffset: from, threadOffset: to } = triage;
    if (!inside(from, 1) || !(to > from) || to > d.length) return [];
    return strings(d, from, to)
      .filter(([, text]) => /^[\w .-]{3,15}\.exe$/i.test(text) || /^[\w-]{4,15}$/.test(text))
      .map(([at, text]) => ({ offset: at - from, text }));
  }

  return { header, triage, context, drivers, modules, entrySize, memory, read, where, stack, trap, processNames };
}

/** Names that follow each other on the stack, a repeat of the one before folded in: the driver chain. */
export function chain(frames) {
  const out = [];
  for (const { at } of frames) {
    const module = at.split("+")[0];
    if (out.at(-1)?.module === module) out.at(-1).count++;
    else out.push({ module, first: at, count: 1 });
  }
  return out;
}

/** The lines of a first look: bug check, time, process, faulting address and the driver chain. */
export function summaryLines(dump) {
  const { header, context } = dump;
  const lines = [
    `bug check ${hex(header.bugCheck)} (${header.parameters.map((p) => hex(p, 16)).join(", ")})`,
    `build ${header.build}, ${header.processors} processors, dump type ${header.dumpType}`,
    `system time ${header.systemTime ?? "?"} UTC, uptime ${header.uptimeSeconds ?? "?"} s`,
  ];
  const names = dump.processNames();
  if (names.length) lines.push(`process block: ${[...new Set(names.map((n) => n.text))].join(", ")}`);
  lines.push(`context rip ${hex(context.rip, 16)} ${dump.where(context.rip) ?? "(outside the loaded drivers)"}`);
  if (header.bugCheck === 0x139) {
    const { frame, record } = dump.trap();
    lines.push(frame ? `trap rip ${hex(frame.rip, 16)} ${dump.where(frame.rip) ?? ""}`.trim() : "trap frame not captured");
    if (record) lines.push(`exception ${hex(record.code)} at ${hex(record.address, 16)} ${dump.where(record.address) ?? ""}, first parameter ${hex(record.first)}`.trim());
  }
  lines.push(`drivers ${dump.drivers.length} (entry size ${hex(dump.entrySize)}), stack ${hex(dump.triage.sizeOfCallStack)} bytes`);
  lines.push("stack, innermost first (slots that point into a driver; some are stale values):");
  for (const link of chain(dump.stack())) lines.push(`  ${link.first}${link.count > 1 ? ` (+${link.count - 1} more in ${link.module})` : ""}`);
  return lines;
}
