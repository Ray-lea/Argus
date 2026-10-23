// Render and compile the native NSIS installer. No application is launched here.
import { spawnSync } from "node:child_process";
import { copyFileSync, existsSync, mkdirSync, readFileSync, statSync, writeFileSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { pathToFileURL } from "node:url";
const root = resolve(import.meta.dirname, "..");
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
    version_with_build: metadata.version.split("-")[0] + ".0",
    homepage: metadata.homepage,
    install_mode: "perMachine",
    license: "",
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
    additional_plugins_path: "",
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
  const output = join(root, `target/release/bundle/nsis/${metadata.productName}_${version}_x64-setup.exe`);
  const directory = join(root, "target/release/nsis/x64");
  mkdirSync(directory, { recursive: true });
  mkdirSync(dirname(output), { recursive: true });
  for (const name of ["utils.nsh", "FileAssociation.nsh", "English.nsh", "SimpChinese.nsh"])
    copyFileSync(join(root, "packaging/windows", name), join(directory, name));
  const script = join(directory, "installer.nsi");
  writeFileSync(
    script,
    "\uFEFF" +
      renderInstaller(metadata, {
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
