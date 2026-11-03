import { build as buildAstro } from "astro";
import { copyFile, mkdir, mkdtemp, rename, rm } from "node:fs/promises";
import { dirname, join, resolve } from "node:path";
import { setTimeout as delay } from "node:timers/promises";
import { buildRelease } from "./release.mjs";
import { writeDistribution } from "./distribution.mjs";
import { prepareContent } from "./content.mjs";
import {
  repoRoot as root,
  websiteRoot,
  workDir as work,
  put,
} from "./paths.mjs";

const output = join(work, "dist");
await mkdir(work, { recursive: true });
const stage = await mkdtemp(join(work, "build-"));
const dir = Object.fromEntries(
  ["site", "downloads", "updates", "www", "packages", "input"].map((name) => [
    name,
    join(stage, name),
  ]),
);
await Promise.all(
  Object.values(dir).map((path) => mkdir(path, { recursive: true })),
);
const release = await buildRelease(
  root,
  join(work, "cache"),
  dir.downloads,
  dir.updates,
);
await writeDistribution(root, dir, release);
await prepareContent(dir.input, release);
await put(stage, "release.json", JSON.stringify(release, null, 2) + "\n");

process.env.ARGUS_SITE_INPUT = dir.input;
process.env.ASTRO_TELEMETRY_DISABLED = "1";
await buildAstro({ root: websiteRoot, outDir: dir.site });
await copyFile(
  join(dir.site, "sitemap-index.xml"),
  join(dir.site, "sitemap.xml"),
);

// Only completed builds replace the disposable output beneath this project's target directory.
if (
  output !== resolve(root, "target/official-web/dist") ||
  dirname(stage) !== work
)
  throw Error("Unexpected build path");
const previous = join(work, `previous-${Date.now()}`);
async function moveBuilt(from, to) {
  if (dirname(from) !== work || dirname(to) !== work)
    throw Error("Unexpected build directory");
  for (let attempt = 0; ; attempt++) {
    try {
      await rename(from, to);
      return;
    } catch (error) {
      if (!["EPERM", "EACCES", "EBUSY"].includes(error.code) || attempt === 8)
        throw error;
      await delay(Math.min(1600, 80 * 2 ** attempt));
    }
  }
}
let saved = false;
try {
  await moveBuilt(output, previous);
  saved = true;
} catch (error) {
  if (error.code !== "ENOENT") throw error;
}
try {
  await moveBuilt(stage, output);
} catch (error) {
  if (saved) await moveBuilt(previous, output);
  throw error;
}
if (saved) await rm(previous, { recursive: true, force: true });
console.log(
  `Built the Argus website and both verified update channels for ${release.tag} in ${output}`,
);
