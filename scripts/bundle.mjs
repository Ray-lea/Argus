// Render and compile the native NSIS installer. No application is launched here.
import { spawnSync } from "node:child_process";
import { copyFileSync, existsSync, mkdirSync, readFileSync, statSync, writeFileSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { pathToFileURL } from "node:url";
import { installerStrings } from "./lib/installer-locales.mjs";
import { cleanupTestProfile } from "./lib/owned-test.mjs";
const root = resolve(import.meta.dirname, "..");
export function validateBuildInfo(info, version) {
  if (info?.version !== version || !["stable", "test"].includes(info.buildKind))
    throw Error("Installer requires a release-profile executable with the expected package version");
  const suffix = typeof info.build === "string" && info.build.startsWith(version + "+") ? info.build.slice(version.length + 1) : null;
  if (info.build !== version && (suffix === null || !suffix.split(".").every(part => /^[0-9A-Za-z-]+$/.test(part))))
    throw Error("Invalid executable display version");
  if (info.buildKind !== (info.build.includes("+") || version.includes("-") ? "test" : "stable"))
    throw Error("Executable build type does not match its version");
  const parts = typeof info.fileVersion === "string" ? info.fileVersion.split(".") : [];
  if (parts.length !== 4 || parts.some(part => !/^\d+$/.test(part) || Number(part) > 65535)
      || parts.slice(0, 3).join(".") !== version.split(/[-+]/)[0])
    throw Error("Invalid executable file version");
  return info;
}
function readBuildInfo(binary, version) {
  const identifier = `dev.argus.taskmanager.test.bundle-info-${process.pid}`;
  try {
    // An older executable that does not recognize the query stays hidden in
    // an isolated profile until the timeout; it cannot activate the installed app.
    const result = spawnSync(binary, ["--argus-build-info", "--hidden", "--test", "--identifier", identifier], { encoding: "utf8", windowsHide: true, timeout: 10000 });
    if (result.error) throw result.error;
    if (result.status !== 0) throw Error(`Executable build information failed (${result.status})`);
    return validateBuildInfo(JSON.parse(result.stdout), version);
  } finally {
    cleanupTestProfile(identifier);
  }
}
export function findNsis() {
  const candidates = [
    process.env.ARGUS_NSIS,
    process.env.NSISDIR && join(process.env.NSISDIR, "makensis.exe"),
    join(process.env["ProgramFiles(x86)"] ?? "C:/Program Files (x86)", "NSIS/makensis.exe"),
    join(process.env.ProgramFiles ?? "C:/Program Files", "NSIS/makensis.exe"),
  ].filter(Boolean);
  const compiler = candidates.find(existsSync);
  if (!compiler) throw Error("NSIS 3.13 is required; install NSIS or set ARGUS_NSIS to makensis.exe.");
  return compiler;
}
const escape = (value) =>
  String(value)
    .replaceAll("$", () => "$$")
    .replaceAll('"', () => '$\\"');
export function renderInstaller(metadata, paths) {
  const values = {
    compression: "lzma",
    manufacturer: metadata.publisher,
    product_name: metadata.productName,
    version: metadata.version,
    display_version: paths.build?.build ?? metadata.version,
    version_with_build: paths.build?.fileVersion ?? metadata.version.split(/[-+]/)[0] + ".0",
    homepage: metadata.homepage,
    install_mode: "perMachine",
    installer_icon: paths.icon,
    uninstaller_icon: paths.icon,
    sidebar_image: "",
    header_image: "",
    uninstaller_header_image: "",
    main_binary_name: "argus",
    main_binary_path: paths.binary,
    bundle_id: metadata.identifier,
    copyright: metadata.copyright,
    out_file: paths.output,
    arch: "x64",
    allow_downgrades: "false",
    display_language_selector: "false",
    uninstaller_sign_cmd: "",
    estimated_size: paths.sizeKiB,
    start_menu_folder: "",
    installer_hooks: paths.hooks,
  };
  return readFileSync(join(root, "packaging/windows/installer-template.nsi"), "utf8").replace(
    /\{\{([^}]+)\}\}/g,
    (_match, key) => {
      if (!(key in values)) throw Error(`Unknown installer parameter: ${key}`);
      return escape(values[key]);
    },
  );
}
export function bundle() {
  if (process.platform !== "win32" || process.arch !== "x64")
    throw Error("Native installer packaging currently supports Windows x64.");
  const metadata = JSON.parse(readFileSync(join(root, "resources/app.json"), "utf8"));
  const version = JSON.parse(readFileSync(join(root, "package.json"), "utf8")).version;
  if (version !== metadata.version) throw Error("Application version metadata differs");
  const binary = join(root, "target/release/argus.exe");
  const build = readBuildInfo(binary, version);
  const output = join(root, `target/release/bundle/nsis/${metadata.productName}_${build.build}_x64-setup.exe`);
  const directory = join(root, "target/release/nsis/x64");
  mkdirSync(directory, { recursive: true });
  mkdirSync(dirname(output), { recursive: true });
  for (const name of ["utils.nsh"])
    copyFileSync(join(root, "packaging/windows", name), join(directory, name));
  writeFileSync(join(directory, "installer-locales.nsh"), "\uFEFF" + installerStrings(root));
  const script = join(directory, "installer.nsi");
  writeFileSync(
    script,
    "\uFEFF" +
      renderInstaller(metadata, {
        build,
        binary,
        output,
        icon: join(root, "resources/icons/icon.ico"),
        hooks: join(root, "packaging/windows/installer-hooks.nsh"),
        sizeKiB: Math.ceil(statSync(binary).size / 1024),
      }),
  );
  const result = spawnSync(findNsis(), ["/V2", script], {
    cwd: directory,
    stdio: "inherit",
    windowsHide: true,
  });
  if (result.error) throw result.error;
  if (result.status !== 0) throw Error(`NSIS failed (${result.status})`);
  console.log(output);
}
if (process.argv[1] && import.meta.url === pathToFileURL(resolve(process.argv[1])).href) bundle();
