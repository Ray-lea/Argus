// Import and export tables of PE files, for checking that an executable still
// loads against the DLL versions Windows gives a process by default.
//
// argus.exe also runs from the installer's temporary folder (argus-version.exe),
// where a user's machine did not apply the manifest's Common Controls 6
// dependency. Such a process binds the system's comctl32.dll 5.82, and a
// single import that version lacks keeps the executable from loading at all:
// the system then shows "entry point not found" and the installer gets
// 0xC0000139 instead of an answer.

function layout(data) {
  if (data.length < 0x40 || data.readUInt16LE(0) !== 0x5a4d) throw new Error("Not a PE file");
  const pe = data.readUInt32LE(0x3c);
  if (pe + 24 > data.length || data.readUInt32LE(pe) !== 0x4550) throw new Error("Invalid PE header");
  const sectionCount = data.readUInt16LE(pe + 6);
  const optionalSize = data.readUInt16LE(pe + 20);
  const optional = pe + 24;
  const magic = data.readUInt16LE(optional);
  if (magic !== 0x10b && magic !== 0x20b) throw new Error(`Unknown optional header magic 0x${magic.toString(16)}`);
  const wide = magic === 0x20b;
  const directoryCount = data.readUInt32LE(optional + (wide ? 108 : 92));
  const directories = optional + (wide ? 112 : 96);
  const sections = [];
  for (let i = 0, at = optional + optionalSize; i < sectionCount; i++, at += 40)
    sections.push({
      va: data.readUInt32LE(at + 12),
      size: Math.max(data.readUInt32LE(at + 8), data.readUInt32LE(at + 16)),
      raw: data.readUInt32LE(at + 20),
    });
  const offset = (rva) => {
    const section = sections.find((s) => rva >= s.va && rva < s.va + s.size);
    if (!section) throw new Error(`RVA 0x${rva.toString(16)} is outside every section`);
    const at = section.raw + rva - section.va;
    if (at >= data.length) throw new Error(`RVA 0x${rva.toString(16)} is outside the file`);
    return at;
  };
  const directory = (index) =>
    index < directoryCount
      ? { rva: data.readUInt32LE(directories + index * 8), size: data.readUInt32LE(directories + index * 8 + 4) }
      : { rva: 0, size: 0 };
  const string = (rva) => {
    const at = offset(rva);
    const end = data.indexOf(0, at);
    if (end < 0) throw new Error("Unterminated string");
    return data.toString("latin1", at, end);
  };
  return { wide, offset, directory, string };
}

function thunks(data, pe, rva) {
  const entries = [];
  const step = pe.wide ? 8 : 4;
  for (let at = pe.offset(rva); ; at += step) {
    const value = pe.wide ? data.readBigUInt64LE(at) : BigInt(data.readUInt32LE(at));
    if (value === 0n) return entries;
    const byOrdinal = pe.wide ? value >> 63n : value >> 31n;
    entries.push(byOrdinal ? { ordinal: Number(value & 0xffffn) } : { name: pe.string(Number(value & 0x7fffffffn) + 2) });
  }
}

/** The imported DLLs, ordinary and delay-loaded, each with its imports by name or ordinal. */
export function peImports(data) {
  const pe = layout(data);
  const modules = [];
  const imports = pe.directory(1);
  if (imports.rva)
    for (let at = pe.offset(imports.rva); ; at += 20) {
      const lookup = data.readUInt32LE(at);
      const name = data.readUInt32LE(at + 12);
      const addresses = data.readUInt32LE(at + 16);
      if (!lookup && !name && !addresses) break;
      modules.push({ dll: pe.string(name), delayed: false, imports: thunks(data, pe, lookup || addresses) });
    }
  const delayed = pe.directory(13);
  if (delayed.rva)
    for (let at = pe.offset(delayed.rva); ; at += 32) {
      const name = data.readUInt32LE(at + 4);
      const lookup = data.readUInt32LE(at + 16);
      if (!name) break;
      modules.push({ dll: pe.string(name), delayed: true, imports: thunks(data, pe, lookup) });
    }
  return modules;
}

/** The names and ordinals a DLL exports. */
export function peExports(data) {
  const pe = layout(data);
  const names = new Set();
  const ordinals = new Set();
  const exports = pe.directory(0);
  if (!exports.rva) return { names, ordinals };
  const at = pe.offset(exports.rva);
  const base = data.readUInt32LE(at + 16);
  const functionCount = data.readUInt32LE(at + 20);
  const nameCount = data.readUInt32LE(at + 24);
  const functions = data.readUInt32LE(at + 28);
  const nameTable = data.readUInt32LE(at + 32);
  for (let i = 0; i < functionCount; i++)
    if (data.readUInt32LE(pe.offset(functions + i * 4))) ordinals.add(base + i);
  for (let i = 0; i < nameCount; i++) names.add(pe.string(data.readUInt32LE(pe.offset(nameTable + i * 4))));
  return { names, ordinals };
}

/** The imports from `dll` (any case) that `exports` cannot satisfy, as "Name" or "#ordinal". */
export function missingImports(modules, dll, exports) {
  const missing = [];
  for (const module of modules) {
    if (module.dll.toLowerCase() !== dll.toLowerCase()) continue;
    for (const entry of module.imports)
      if (entry.name !== undefined ? !exports.names.has(entry.name) : !exports.ordinals.has(entry.ordinal))
        missing.push(entry.name ?? `#${entry.ordinal}`);
  }
  return missing;
}

/**
 * Throws when `executable` imports from comctl32.dll anything the system's
 * version 5.82 (`systemComctl32`, from System32) lacks. Functions only
 * version 6 exports by name, such as GetWindowSubclass (ordinal 411 in both)
 * or TaskDialogIndirect, are imported by ordinal or looked up at run time.
 */
export function verifyComctl32Imports(executable, systemComctl32) {
  const missing = missingImports(peImports(executable), "comctl32.dll", peExports(systemComctl32));
  if (missing.length)
    throw new Error(
      `argus.exe imports ${missing.join(", ")} from comctl32.dll, which the system's comctl32.dll 5.82 does not export: ` +
        "without a Common Controls 6 activation context (as for the installer's argus-version.exe on some machines) it would not load. " +
        "Import by ordinal or resolve at run time.",
    );
}
