// Prepares the website content for the development server, then keeps the
// development edition in step with the working tree. scripts/serve.mjs runs
// this as a child process and restarts it when a pipeline script changes.
import { watch } from "node:fs";
import { mkdir, mkdtemp, readFile, rename, rm, writeFile } from "node:fs/promises";
import { join, relative, resolve } from "node:path";
import { folders, prepareContent, prepareDevelopmentEdition } from "./content.mjs";
import { contentDir, prune, putIfChanged, readSite, repoRoot, workDir } from "./paths.mjs";
import { buildRelease } from "./release.mjs";

const devDir = join(workDir, "dev");

async function releaseData() {
  for (const path of [join(workDir, "dist/release.json"), join(devDir, "release.json")]) {
    try {
      return JSON.parse(await readFile(path, "utf8"));
    } catch (error) {
      if (error.code !== "ENOENT") throw error;
    }
  }
  console.log("[website] Fetching and verifying release data for the first preview…");
  await mkdir(devDir, { recursive: true });
  const session = await mkdtemp(join(devDir, "release-"));
  try {
    const release = await buildRelease(
      repoRoot,
      join(workDir, "cache"),
      join(session, "downloads"),
      join(session, "feeds"),
    );
    await writeFile(join(session, "release.json"), JSON.stringify(release));
    await rename(join(session, "release.json"), join(devDir, "release.json"));
    return release;
  } finally {
    await rm(session, { recursive: true, force: true });
  }
}

const localPath = (path) => relative(repoRoot, resolve(path)).replaceAll("\\", "/");
// Inputs of site.json other than the documents themselves.
const siteSources = new Set([
  "README.md",
  "README.en.md",
  "PRIVACY.md",
  "resources/logo.svg",
  "website/public-settings.json",
  "docs/images/screenshot-dark.png",
  "docs/images/screenshot-en-dark.png",
]);
const isDocument = (file) =>
  file.startsWith("docs/") && file.endsWith(".md") && !file.endsWith("/AGENTS.md");

async function refreshDocuments() {
  const written = new Set();
  const edition = await prepareDevelopmentEdition(written, true);
  await prune(join(contentDir, "docs", folders.dev), written);
  const site = readSite();
  site.editions[1] = edition;
  await putIfChanged(contentDir, "site.json", JSON.stringify(site, null, 2));
}

const release = await releaseData();
console.log("[website] Preparing " + release.tag + " with the workspace documentation…");
await prepareContent(release, { worktree: true });
process.send?.("ready");

let queue = Promise.resolve(),
  timer,
  full = false;
function schedule(everything) {
  full ||= everything;
  clearTimeout(timer);
  timer = setTimeout(() => {
    const all = full;
    full = false;
    queue = queue
      .then(() => (all ? prepareContent(release, { worktree: true }) : refreshDocuments()))
      .catch((error) => console.error("[website]", error.message));
  }, 120);
}
for (const dir of ["docs", "website", "resources", "."]) {
  const root = join(repoRoot, dir);
  watch(root, { recursive: dir === "docs", persistent: true }, (event, name) => {
    // Windows reports a renamed folder without its files; reread the whole edition.
    if (!name) return dir === "docs" && schedule(false);
    const file = localPath(join(root, name));
    if (siteSources.has(file)) schedule(true);
    else if (isDocument(file) || (dir === "docs" && !name.includes("."))) schedule(false);
  });
}
