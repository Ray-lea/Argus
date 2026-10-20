// Stages only Argus-owned PE files. Signing itself runs in the official
// SignPath action; this script never reads an API token or a private key.
import { createHash } from "node:crypto";
import { execFileSync } from "node:child_process";
import { copyFileSync, existsSync, mkdirSync, readFileSync, readdirSync, writeFileSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { pathToFileURL } from "node:url";

const root = resolve(import.meta.dirname, "..");
const stage = join(root, "target", "signpath");
const release = join(root, "target", "release");
const helpers = ["argus-taskmgr-launcher.exe", "argus-taskmgr-recover.exe"];
const version = JSON.parse(readFileSync(join(root, "package.json"), "utf8")).version;
const installer = `Argus_${version}_x64-setup.exe`;
const hash = (bytes) => createHash("sha256").update(bytes).digest("hex");
const bytes = (file) => readFileSync(file);
const directory = (name) => {
  const path = join(stage, name);
  mkdirSync(path, { recursive: true });
  return path;
};
const copy = (from, to) => {
  mkdirSync(dirname(to), { recursive: true });
  copyFileSync(from, to);
};
const json = (name, value) => writeFileSync(join(stage, name), `${JSON.stringify(value, null, 2)}\n`);
const readJson = (name) => JSON.parse(readFileSync(join(stage, name), "utf8"));

export function peHeaders(data) {
  if (data.length < 0x40 || data.readUInt16LE(0) !== 0x5a4d) throw new Error("Not a PE file");
  const pe = data.readUInt32LE(0x3c);
  if (pe + 24 > data.length || data.readUInt32LE(pe) !== 0x4550) throw new Error("Invalid PE header");
  const optional = pe + 24;
  const magic = data.readUInt16LE(optional);
  const directories = magic === 0x20b ? optional + 112 : magic === 0x10b ? optional + 96 : 0;
  if (!directories || directories + 40 > data.length) throw new Error("Invalid optional PE header");
  const security = directories + 32;
  return {
    checksum: optional + 64,
    security,
    certificate: data.readUInt32LE(security),
    certificateSize: data.readUInt32LE(security + 4),
  };
}

// Signing may change only the checksum, certificate directory, alignment
// padding and appended certificate. Every payload byte must stay intact.
export function verifySignedPayload(original, signed) {
  const before = peHeaders(original);
  const after = peHeaders(signed);
  if (before.certificateSize !== 0) throw new Error("The input is already signed");
  if (before.security !== after.security || after.certificateSize < 8
      || after.certificate % 8 !== 0 || after.certificate < original.length
      || after.certificate - original.length > 7
      || after.certificate + after.certificateSize !== signed.length) {
    throw new Error("Invalid signed PE layout");
  }
  const left = Buffer.from(original);
  const right = Buffer.from(signed.subarray(0, original.length));
  for (const data of [left, right]) {
    data.fill(0, before.checksum, before.checksum + 4);
    data.fill(0, before.security, before.security + 8);
  }
  if (!left.equals(right) || signed.subarray(original.length, after.certificate).some((v) => v !== 0)) {
    throw new Error("Signing changed executable content");
  }
}

function applySigned(folder, names) {
  for (const name of names) {
    const original = join(stage, `${folder}-unsigned`, name);
    const signed = join(stage, `${folder}-signed`, name);
    verifySignedPayload(bytes(original), bytes(signed));
  }
}

function embeddedHelpers(main) {
  for (const name of helpers) {
    if (!main.includes(bytes(join(stage, "helpers-signed", name)))) {
      throw new Error(`The executable does not embed the signed ${name}`);
    }
  }
}

function once(text, pattern, replacement) {
  const matches = text.match(new RegExp(pattern.source, `${pattern.flags.replace("g", "")}g`));
  if (matches?.length !== 1) throw new Error(`Unexpected NSIS template near ${pattern}`);
  return text.replace(pattern, replacement);
}

function nsis(mode, output) {
  const rendered = join(release, "nsis", "x64", "installer.nsi");
  let text = readFileSync(rendered, "utf8");
  if (!text.includes('!define UNINSTALLERSIGNCOMMAND ""')) throw new Error("Unexpected NSIS signing command");
  const command = `"${process.execPath}" "${join(root, "scripts", "signpath.mjs")}" uninstaller "%1"`;
  if (command.includes("'") || output.includes('"')) throw new Error("Unsupported build path quoting");
  text = once(text, /^!define OUTFILE ".*"$/m, `!define OUTFILE "${output.replaceAll("$", "$$")}"`);
  text = once(text, /; Uninstaller signing command/,
    `!uninstfinalize '${command.replaceAll("$", "$$")}' = 0\n\n; Uninstaller signing command`);
  const script = join(dirname(rendered), "signpath-installer.nsi");
  writeFileSync(script, text.replaceAll("\r\n", "\n"));
  const compiler = [
    join(process.env.LOCALAPPDATA ?? "", "tauri", "NSIS", "makensis.exe"),
    join(root, "target", ".tauri", "NSIS", "makensis.exe"),
  ].find(existsSync);
  if (!compiler) throw new Error("Tauri's NSIS compiler is missing");
  execFileSync(compiler, ["/V2", script], {
    cwd: dirname(rendered), windowsHide: true, stdio: "inherit",
    env: { ...process.env, ARGUS_SIGNPATH_UNINSTALLER_MODE: mode },
  });
}

function uninstaller(file) {
  if (!file || !existsSync(file)) throw new Error("NSIS did not supply an uninstaller");
  const raw = bytes(file);
  peHeaders(raw);
  const original = join(directory("application-unsigned"), "uninstall.exe");
  if (process.env.ARGUS_SIGNPATH_UNINSTALLER_MODE === "capture") {
    writeFileSync(original, raw);
  } else if (process.env.ARGUS_SIGNPATH_UNINSTALLER_MODE === "apply") {
    if (!raw.equals(bytes(original))) throw new Error("NSIS regenerated a different uninstaller");
    const signed = join(stage, "application-signed", "uninstall.exe");
    verifySignedPayload(raw, bytes(signed));
    copyFileSync(signed, file);
    json("uninstaller-applied.json", { sha256: hash(bytes(signed)) });
  } else {
    throw new Error("An NSIS signing phase is required");
  }
}

function prepare() {
  directory("");
  json("build-config.json", {
    build: { beforeBuildCommand: "pnpm build" },
    bundle: { createUpdaterArtifacts: false },
  });
  for (const name of helpers) {
    const file = join(release, name);
    if (peHeaders(bytes(file)).certificateSize !== 0) throw new Error(`Expected unsigned ${name}`);
    copy(file, join(directory("helpers-unsigned"), name));
  }
}

function applyHelpers() {
  applySigned("helpers", helpers);
  for (const name of helpers) copy(join(stage, "helpers-signed", name), join(release, name));
}

function prepareApplication() {
  const main = bytes(join(release, "argus.exe"));
  embeddedHelpers(main);
  if (peHeaders(main).certificateSize !== 0) throw new Error("Expected an unsigned main executable");
  copy(join(release, "argus.exe"), join(directory("application-unsigned"), "argus.exe"));
  nsis("capture", join(stage, "staging-setup.exe"));
  if (!existsSync(join(stage, "application-unsigned", "uninstall.exe"))) {
    throw new Error("NSIS did not produce an uninstaller");
  }
}

function buildInstaller() {
  applySigned("application", ["argus.exe", "uninstall.exe"]);
  embeddedHelpers(bytes(join(stage, "application-signed", "argus.exe")));
  copy(join(stage, "application-signed", "argus.exe"), join(release, "argus.exe"));
  writeFileSync(join(stage, "uninstaller-applied.json"), "");
  // Reuse Tauri's rendered script directly: another bundle pass would patch
  // argus.exe and invalidate its signature.
  nsis("apply", join(directory("installer-unsigned"), installer));
  if (readJson("uninstaller-applied.json").sha256 !== hash(bytes(join(stage, "application-signed", "uninstall.exe")))) {
    throw new Error("The signed uninstaller was not embedded");
  }
  if (!bytes(join(release, "argus.exe")).equals(bytes(join(stage, "application-signed", "argus.exe")))) {
    throw new Error("Bundling changed the signed executable");
  }
}

function collect() {
  applySigned("installer", [installer]);
  const out = directory("artifacts");
  const commit = process.env.GITHUB_SHA;
  if (!/^[a-f0-9]{40}$/.test(commit ?? "")) throw new Error("A complete GitHub commit ID is required");
  const prefix = `Argus_${version}_signpath-test_${commit.slice(0, 7)}_x64`;
  copy(join(stage, "application-signed", "argus.exe"), join(out, `${prefix}.exe`));
  copy(join(stage, "installer-signed", installer), join(out, `${prefix}-setup.exe`));
  const sums = readdirSync(out).filter((name) => name.endsWith(".exe"))
    .sort().map((name) => `${hash(bytes(join(out, name)))}  ${name}`).join("\n");
  writeFileSync(join(out, "SHA256SUMS.txt"), `${sums}\n`);
  writeFileSync(join(out, "TEST-SIGNING.txt"),
    `Argus test signing build\nCommit: ${commit}\nPolicy: test-signing\n` +
    "The SignPath Foundation self-signed test certificate is not publicly trusted.\n" +
    "This build is not a published release or an application update.\n");
}

if (process.argv[1] && import.meta.url === pathToFileURL(resolve(process.argv[1])).href) {
  const commands = {
    prepare, "apply-helpers": applyHelpers, "prepare-application": prepareApplication,
    "build-installer": buildInstaller, collect, uninstaller,
  };
  const command = commands[process.argv[2]];
  if (!command) throw new Error("Unknown SignPath build phase");
  command(process.argv[3]);
}
