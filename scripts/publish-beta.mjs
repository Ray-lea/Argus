import { execFileSync } from "node:child_process";
import { copyFile } from "node:fs/promises";
import { dirname, join, resolve } from "node:path";
import { pathToFileURL } from "node:url";

// Called by release.yml after creating the prerelease. Only an explicit 404
// means that the repository has no stable release; other API failures must fail CI.
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
  const response = await request(`https://api.github.com/repos/${repository}/releases/latest`, {
    headers: {
      accept: "application/vnd.github+json",
      authorization: `Bearer ${token}`,
      "user-agent": "Argus release publisher",
    },
    signal: AbortSignal.timeout(30000),
  });
  if (response.status === 404) {
    await response.body?.cancel();
    report("::warning::No stable release exists; the prerelease is published but the beta channel cannot be updated yet.");
    return false;
  }
  if (!response.ok) {
    await response.body?.cancel();
    throw Error(`Cannot find the latest stable release: GitHub API returned ${response.status}`);
  }
  const release = await response.json();
  if (typeof release.tag_name !== "string" || !release.tag_name.startsWith("v") || release.draft || release.prerelease)
    throw Error("Invalid latest stable release response");
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
