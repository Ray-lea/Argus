import { execFileSync } from "node:child_process";
import { copyFile, readFile } from "node:fs/promises";
import { dirname, join, resolve } from "node:path";
import { pathToFileURL } from "node:url";

function parseVersion(text) {
  const match = /^(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)(?:-([0-9A-Za-z-]+(?:\.[0-9A-Za-z-]+)*))?(?:\+[0-9A-Za-z-]+(?:\.[0-9A-Za-z-]+)*)?$/.exec(text ?? "");
  const pre = match?.[4]?.split(".") ?? [];
  if (!match || pre.some(part => /^0\d+$/.test(part)))
    throw Error(`Invalid channel version: ${text}`);
  return { core: match.slice(1, 4).map(BigInt), pre };
}

// SemVer precedence, including numeric prerelease identifiers and build metadata.
function compareVersions(left, right) {
  const a = parseVersion(left), b = parseVersion(right);
  for (let i = 0; i < 3; i++)
    if (a.core[i] !== b.core[i]) return a.core[i] > b.core[i] ? 1 : -1;
  if (!a.pre.length || !b.pre.length) return b.pre.length - a.pre.length;
  for (let i = 0; i < Math.max(a.pre.length, b.pre.length); i++) {
    const x = a.pre[i], y = b.pre[i];
    if (x === y) continue;
    if (x === undefined) return -1;
    if (y === undefined) return 1;
    const nx = /^\d+$/.test(x), ny = /^\d+$/.test(y);
    if (nx && ny) return BigInt(x) > BigInt(y) ? 1 : -1;
    if (nx !== ny) return nx ? -1 : 1;
    return x > y ? 1 : -1;
  }
  return 0;
}

// Called by release.yml after creating the prerelease. Only an explicit 404
// means that the repository has no stable release; other API failures must fail CI.
// The workflow serializes all build-and-publish jobs around this read/compare/upload.
export async function publishBeta(manifestPath, {
  repository = process.env.GITHUB_REPOSITORY,
  token = process.env.GH_TOKEN,
  request = fetch,
  upload = (tag, path) => execFileSync("gh", [
    "release", "upload", tag, path, "--clobber", "--repo", repository,
  ], { stdio: "inherit", windowsHide: true }),
  report = console.log,
} = {}) {
  if (!/^[\w.-]+\/[\w.-]+$/.test(repository ?? "") || !token)
    throw Error("GITHUB_REPOSITORY and GH_TOKEN are required");
  const manifest = JSON.parse(await readFile(manifestPath, "utf8"));
  if (!parseVersion(manifest.version).pre.length)
    throw Error("The beta publisher requires a prerelease manifest");
  async function api(path, { missing = false, asset = false } = {}) {
    const response = await request(`https://api.github.com/repos/${repository}/${path}`, {
      headers: {
        accept: asset ? "application/octet-stream" : "application/vnd.github+json",
        authorization: `Bearer ${token}`,
        "cache-control": "no-cache",
        "user-agent": "Argus release publisher",
      },
      signal: AbortSignal.timeout(30000),
    });
    if (!response.ok) {
      await response.body?.cancel();
      if (missing && response.status === 404) return null;
      throw Error(`Cannot read ${path}: GitHub API returned ${response.status}`);
    }
    return response.json();
  }
  const release = await api("releases/latest", { missing: true });
  if (release === null) {
    report("::warning::No stable release exists; the prerelease is published but the beta channel cannot be updated yet.");
    return false;
  }
  if (typeof release.tag_name !== "string" || !release.tag_name.startsWith("v") || release.draft || release.prerelease)
    throw Error("Invalid latest stable release response");
  const stableVersion = release.tag_name.slice(1);
  if (parseVersion(stableVersion).pre.length)
    throw Error("Latest stable release has a prerelease tag");
  if (compareVersions(manifest.version, stableVersion) <= 0) {
    report(`Skipped ${manifest.version}: latest stable is ${stableVersion}`);
    return false;
  }
  const beta = release.assets?.find(asset => asset.name === "beta.json");
  if (!Number.isSafeInteger(beta?.id) || beta.id <= 0)
    throw Error("Latest stable release has no beta.json asset");
  // Read the asset API rather than a cached releases/latest/download redirect.
  const current = await api(`releases/assets/${beta.id}`, { asset: true });
  if (compareVersions(manifest.version, current.version) <= 0) {
    report(`Skipped ${manifest.version}: beta channel already offers ${current.version}`);
    return false;
  }
  const betaPath = join(dirname(manifestPath), "beta.json");
  await copyFile(manifestPath, betaPath);
  await upload(release.tag_name, betaPath);
  report(`Updated the beta channel on ${release.tag_name}`);
  return true;
}

if (process.argv[1] && import.meta.url === pathToFileURL(resolve(process.argv[1])).href) {
  if (process.argv.length !== 3) throw Error("Usage: node scripts/publish-beta.mjs <manifest.json>");
  await publishBeta(process.argv[2]);
}
