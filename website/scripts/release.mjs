// Build from published GitHub assets and the public key used by installed Argus copies.
import { createHash, randomUUID } from "node:crypto";
import { execFileSync } from "node:child_process";
import { createReadStream, createWriteStream } from "node:fs";
import { copyFile, mkdir, rename, rm, stat, writeFile } from "node:fs/promises";
import { join } from "node:path";
import { Readable, Transform } from "node:stream";
import { pipeline } from "node:stream/promises";
import { setTimeout as delay } from "node:timers/promises";

export const repo = "https://github.com/Ray-lea/Argus";
export const origin = "https://argustask.com";
export const downloadOrigin = "https://dl.argustask.com";
export const feedOrigin = "https://update.argustask.com";
export const assetLimit = 25 * 1024 * 1024;
const transferLimit = 256 * 1024 * 1024;
const versionPattern =
  /^(?:0|[1-9]\d*)\.(?:0|[1-9]\d*)\.(?:0|[1-9]\d*)(?:-[0-9A-Za-z]+(?:[.-][0-9A-Za-z]+)*)?$/;

export function filenames(version) {
  if (!versionPattern.test(version))
    throw Error(`Invalid release version: ${version}`);
  return {
    installer: `Argus_${version}_x64-setup.exe`,
    portable: `Argus_${version}_x64.exe`,
  };
}

export function readChecksums(text, names) {
  const checksums = new Map();
  for (const line of text.trim().split(/\r?\n/)) {
    const match = /^([a-f0-9]{64}) [ *]([^/\\]+)$/i.exec(line);
    if (!match || checksums.has(match[2]))
      throw Error("Invalid or duplicate release checksum");
    checksums.set(match[2], match[1].toLowerCase());
  }
  for (const name of names)
    if (!checksums.has(name)) throw Error(`Missing checksum for ${name}`);
  return checksums;
}

async function response(url) {
  for (let attempt = 0; attempt < 3; attempt++) {
    let res;
    try {
      res = await fetch(url, {
        headers: { "user-agent": "argustask.com release builder", ...(new URL(url).origin === 'https://api.github.com' && process.env.GITHUB_TOKEN ? {authorization:`Bearer ${process.env.GITHUB_TOKEN}`} : {}) },
        signal: AbortSignal.timeout(120000),
      });
    } catch (error) {
      if (attempt === 2)
        throw Error(`Network error fetching ${url}`, { cause: error });
    }
    if (res?.ok) return res;
    if (res) {
      await res.body?.cancel();
      if (attempt === 2 || (res.status !== 429 && res.status < 500))
        throw Error(`${res.status} fetching ${url}`);
    }
    await delay(500 * 2 ** attempt);
  }
}

async function smallText(url) {
  const res = await response(url);
  const parts = [];
  let length = 0;
  for await (const part of res.body) {
    length += part.length;
    if (length > 2 * 1024 * 1024) throw Error(`Metadata exceeds 2 MiB: ${url}`);
    parts.push(part);
  }
  return Buffer.concat(parts).toString("utf8");
}

async function artifact(url, path, expected) {
  try {
    const cached = await stat(path);
    if (cached.isFile() && cached.size <= transferLimit) {
      const hash = createHash("sha256");
      for await (const chunk of createReadStream(path)) hash.update(chunk);
      if (hash.digest("hex") === expected) return cached.size;
    }
  } catch (error) {
    if (error.code !== "ENOENT") throw error;
  }
  const res = await response(url);
  const hash = createHash("sha256");
  let size = 0;
  const temporary = `${path}.${randomUUID()}.part`;
  try {
    await pipeline(
      Readable.fromWeb(res.body),
      new Transform({
        transform(chunk, _, cb) {
          size += chunk.length;
          if (size > transferLimit)
            return cb(Error("Release file exceeds build transfer limit"));
          hash.update(chunk);
          cb(null, chunk);
        },
      }),
      createWriteStream(temporary, { flags: "wx" }),
    );
    if (hash.digest("hex") !== expected)
      throw Error(`Checksum mismatch: ${url}`);
    await rename(temporary, path);
    return size;
  } finally {
    await rm(temporary, { force: true });
  }
}

function verify(root, version, path, signature) {
  execFileSync(
    process.execPath,
    [
      join(root, "scripts/release.mjs"),
      "check-signature",
      version,
      path,
      signature,
    ],
    { cwd: root, stdio: ["ignore", "pipe", "inherit"] },
  );
}

// Public figures of the repository for the home page: stars, forks, published
// releases and how often their executables were downloaded from GitHub.
export async function repositoryStats() {
  const [info, releases] = await Promise.all(
    ["", "/releases?per_page=100"].map(async (path) =>
      JSON.parse(await smallText("https://api.github.com/repos/Ray-lea/Argus" + path)),
    ),
  );
  const published = releases.filter((r) => !r.draft);
  const stats = {
    stars: info.stargazers_count,
    forks: info.forks_count,
    releases: published.filter((r) => !r.prerelease).length,
    downloads: published
      .flatMap((r) => r.assets)
      .filter((a) => a.name.endsWith(".exe"))
      .reduce((sum, a) => sum + a.download_count, 0),
    checked: new Date().toISOString(),
  };
  if (Object.values(stats).some((v) => v === undefined || Number.isNaN(v)))
    throw Error("Unexpected GitHub repository data");
  return stats;
}

export async function buildRelease(root, cache, downloads, feeds) {
  const release = JSON.parse(
    await smallText(
      "https://api.github.com/repos/Ray-lea/Argus/releases/latest",
    ),
  );
  const tag = release.tag_name;
  const stableVersion = tag?.slice(1);
  if (
    !tag?.startsWith("v") ||
    !versionPattern.test(stableVersion) ||
    tag.includes("-") ||
    release.draft ||
    release.prerelease
  )
    throw Error("Invalid latest stable release");
  const manifests = Object.fromEntries(
    await Promise.all(
      ["latest", "beta"].map(async (channel) => [
        channel,
        JSON.parse(
          await smallText(`${repo}/releases/download/${tag}/${channel}.json`),
        ),
      ]),
    ),
  );
  if (manifests.latest.version !== stableVersion)
    throw Error("Stable release and manifest disagree");
  const versions = new Map();
  for (const [channel, manifest] of Object.entries(manifests)) {
    const version = manifest.version;
    const names = filenames(version);
    const upstream = `${repo}/releases/download/v${version}`;
    const windows = ["windows-x86_64", "windows-x86_64-nsis"].map(
      (key) => manifest.platforms?.[key],
    );
    if (
      windows.some(
        (p) => p?.url !== `${upstream}/${names.installer}` || !p.signature,
      )
    )
      throw Error(`Invalid installer in ${channel} channel`);
    const versionCache = join(cache, `v${version}`);
    const output = join(downloads, `v${version}`);
    await Promise.all([
      mkdir(versionCache, { recursive: true }),
      mkdir(output, { recursive: true }),
    ]);
    if (!versions.has(version)) {
      const sums = await smallText(`${upstream}/SHA256SUMS.txt`);
      const checksums = readChecksums(sums, Object.values(names));
      const files = Object.fromEntries(
        await Promise.all(
          Object.entries(names).map(async ([kind, name]) => {
            const path = join(versionCache, name);
            const sha256 = checksums.get(name);
            const size = await artifact(`${upstream}/${name}`, path, sha256);
            // Larger assets and older versions resolve to the original, versioned release.
            if (size <= assetLimit) await copyFile(path, join(output, name));
            return [
              kind,
              {
                name,
                size,
                sha256,
                url: `${downloadOrigin}/v${version}/${name}`,
                upstream: `${upstream}/${name}`,
              },
            ];
          }),
        ),
      );
      await writeFile(join(output, "SHA256SUMS.txt"), sums);
      versions.set(version, { version, files });
    }
    const sigPath = join(versionCache, `${channel}.sig`);
    for (const sig of new Set(windows.map((p) => p.signature))) {
      await writeFile(sigPath, sig);
      verify(root, version, join(versionCache, names.installer), sigPath);
    }
    if (manifest.argus) {
      const { releaseNotes, signature } = manifest.argus;
      if (
        typeof releaseNotes !== "string" ||
        typeof signature !== "string" ||
        JSON.parse(releaseNotes).version !== version
      )
        throw Error("Invalid signed release notes");
      const notesPath = join(versionCache, `${channel}-notes.json`);
      await Promise.all([
        writeFile(notesPath, releaseNotes),
        writeFile(sigPath, signature),
      ]);
      verify(root, version, notesPath, sigPath);
    }
    for (const p of windows) p.url = versions.get(version).files.installer.url;
    await writeFile(
      join(feeds, `${channel}.json`),
      JSON.stringify(manifest) + "\n",
    );
  }
  return {
    ...versions.get(stableVersion),
    tag,
    published: release.published_at,
    beta: manifests.beta.version,
  };
}
