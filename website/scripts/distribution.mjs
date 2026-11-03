import { execFileSync } from "node:child_process";
import { readdir } from "node:fs/promises";
import { join } from "node:path";
import { filenames, origin, repo } from "./release.mjs";
import { put } from "./paths.mjs";

// Keep installer mirrors and update feeds independent of the presentation layer.
export async function writeDistribution(root, dir, release) {
  const { tag, version, files } = release;
  const protect =
    "  X-Content-Type-Options: nosniff\n  Referrer-Policy: strict-origin-when-cross-origin\n";
  await put(
    dir.updates,
    "_headers",
    `/*\n${protect}  Cache-Control: public, max-age=120, must-revalidate\n`,
  );
  await put(
    dir.downloads,
    "_headers",
    `/*\n${protect}  Cache-Control: public, max-age=31536000, immutable\n  Content-Disposition: attachment\n`,
  );
  const redirects = ["/ https://argustask.com/download/ 302"];
  const tags = execFileSync("git", ["tag", "--list", "v*"], {
    cwd: root,
    encoding: "utf8",
  })
    .trim()
    .split(/\r?\n/);
  for (const releaseTag of new Set([...tags, tag, "v" + release.beta])) {
    let names;
    try {
      names = filenames(releaseTag.slice(1));
    } catch {
      continue;
    }
    const actual = await readdir(join(dir.downloads, releaseTag)).catch(
      (error) => {
        if (error.code === "ENOENT") return [];
        throw error;
      },
    );
    for (const name of [...Object.values(names), "SHA256SUMS.txt"])
      if (!actual.includes(name)) {
        redirects.push(
          `/${releaseTag}/${name} ${repo}/releases/download/${releaseTag}/${name} 302`,
        );
      }
  }
  if (redirects.length > 2000)
    throw Error(
      "Static redirect limit reached; preserve versioned paths before deploying",
    );
  await put(dir.downloads, "_redirects", redirects.join("\n") + "\n");
  await put(
    dir.www,
    "index.html",
    '<!doctype html><title>Argus</title><a href="https://argustask.com/">Argus</a>',
  );
  await put(dir.www, "_redirects", "/* https://argustask.com/:splat 301\n");

  // Generate submission drafts outside the hosted assets. Package managers use immutable upstream URLs.
  const id = "Ray-lea.Argus",
    schema = "1.10.0",
    common = `PackageIdentifier: ${id}\nPackageVersion: ${JSON.stringify(version)}\n`;
  const yaml = (type) =>
    `# yaml-language-server: $schema=https://aka.ms/winget-manifest.${type}.${schema}.schema.json\n\n` +
    common;
  const winget = `winget/manifests/r/Ray-lea/Argus/${version}/`;
  await put(
    dir.packages,
    winget + id + ".yaml",
    yaml("version") +
      `DefaultLocale: en-US\nManifestType: version\nManifestVersion: ${schema}\n`,
  );
  await put(
    dir.packages,
    winget + id + ".installer.yaml",
    yaml("installer") +
      `InstallerType: nullsoft\nScope: machine\nInstallerSwitches:\n  Silent: /S\n  SilentWithProgress: /S\nInstallers:\n  - Architecture: x64\n    InstallerUrl: ${files.installer.upstream}\n    InstallerSha256: ${files.installer.sha256.toUpperCase()}\nManifestType: installer\nManifestVersion: ${schema}\n`,
  );
  await put(
    dir.packages,
    winget + id + ".locale.en-US.yaml",
    yaml("defaultLocale") +
      `PackageLocale: en-US\nPublisher: Argus contributors\nPackageName: Argus\nPackageUrl: ${origin}/en/\nLicense: AGPL-3.0-or-later\nLicenseUrl: ${repo}/blob/${tag}/LICENSE\nShortDescription: A Windows task manager for processes, hardware and resource history.\nReleaseNotesUrl: ${repo}/releases/tag/${tag}\nManifestType: defaultLocale\nManifestVersion: ${schema}\n`,
  );
  await put(
    dir.packages,
    "scoop/argus.json",
    JSON.stringify(
      {
        version,
        description:
          "Windows task manager for processes, hardware and resource history",
        homepage: origin,
        license: "AGPL-3.0-or-later",
        architecture: {
          "64bit": {
            url: files.portable.upstream + "#/argus.exe",
            hash: files.portable.sha256,
          },
        },
        bin: "argus.exe",
        shortcuts: [["argus.exe", "Argus"]],
      },
      null,
      2,
    ) + "\n",
  );
}
