// Windows target architecture resolution and PE machine verification shared by
// build, installer packaging and SignPath staging scripts.

export const WINDOWS_TARGETS = {
  x64: {
    arch: "x64",
    rustTarget: "x86_64-pc-windows-msvc",
    peMachine: 0x8664,
  },
  arm64: {
    arch: "arm64",
    rustTarget: "aarch64-pc-windows-msvc",
    peMachine: 0xaa64,
  },
};

export function hostWindowsArch(nodeArch = process.arch) {
  if (nodeArch === "x64" || nodeArch === "arm64") return nodeArch;
  throw new Error(`Unsupported Windows host architecture: ${nodeArch}`);
}

export function parseArchFlag(argv) {
  const rest = [];
  let arch = null;
  for (let i = 0; i < argv.length; i++) {
    const arg = argv[i];
    if (arg === "--arch") {
      arch = argv[++i] ?? "";
    } else if (arg.startsWith("--arch=")) {
      arch = arg.slice("--arch=".length);
    } else {
      rest.push(arg);
    }
  }
  return { arch, rest };
}

export function resolveWindowsTarget(requestedArch = null, nodeArch = process.arch) {
  const host = hostWindowsArch(nodeArch);
  const arch = requestedArch ?? host;
  const spec = WINDOWS_TARGETS[arch];
  if (!spec) throw new Error(`Unsupported Windows target architecture: ${arch} (expected x64 or arm64)`);
  return {
    ...spec,
    hostArch: host,
    cross: arch !== host,
  };
}

export function peMachine(data) {
  if (data.length < 0x40 || data.readUInt16LE(0) !== 0x5a4d) throw new Error("Not a PE file");
  const pe = data.readUInt32LE(0x3c);
  if (pe + 6 > data.length || data.readUInt32LE(pe) !== 0x4550) throw new Error("Invalid PE header");
  return data.readUInt16LE(pe + 4);
}

export function verifyPeArchitecture(data, expectedArch) {
  const spec = WINDOWS_TARGETS[expectedArch];
  if (!spec) throw new Error(`Unknown Windows architecture: ${expectedArch}`);
  const actual = peMachine(data);
  if (actual !== spec.peMachine) {
    throw new Error(
      `Executable PE machine 0x${actual.toString(16)} does not match expected ${expectedArch} (0x${spec.peMachine.toString(16)})`,
    );
  }
}
