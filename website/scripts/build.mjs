import { spawnSync } from "node:child_process";
import { cp, mkdir, mkdtemp, readFile, rename, rm } from "node:fs/promises";
import { dirname, join, resolve } from "node:path";
import { setTimeout as delay } from "node:timers/promises";
import { buildRelease } from "./release.mjs";
import { writeDistribution } from "./distribution.mjs";
import { prepareContent } from "./content.mjs";
import { secureHtml } from "./csp.mjs";
import {
  contentDir,
  repoRoot as root,
  websiteRoot,
  workDir as work,
  put,
} from "./paths.mjs";
import { origin, repo } from "./release.mjs";

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
await prepareContent(release);
await put(stage, "release.json", JSON.stringify(release, null, 2) + "\n");
await put(dir.input, "site.json", await readFile(join(contentDir, "site.json")));

// Next.js exports the static pages into website/out.
const next = spawnSync(
  process.execPath,
  [join(websiteRoot, "node_modules/next/dist/bin/next"), "build"],
  {
    cwd: websiteRoot,
    stdio: "inherit",
    env: { ...process.env, NEXT_TELEMETRY_DISABLED: "1", ARGUS_NEXT_DIR: "" },
  },
);
if (next.status !== 0) throw Error("next build failed");
await cp(join(websiteRoot, "out"), dir.site, { recursive: true });
await secureHtml(dir.site);
const { contact } = JSON.parse(await readFile(join(contentDir, "site.json"), "utf8"));
const expires = new Date(Date.now() + 350 * 86400000)
  .toISOString()
  .replace(/\.\d{3}Z$/, "Z");
await put(
  dir.site,
  ".well-known/security.txt",
  "Contact: " +
    repo +
    "/security/advisories/new\n" +
    (contact ? "Contact: mailto:security@argustask.com\n" : "") +
    "Expires: " +
    expires +
    "\nCanonical: " +
    origin +
    "/.well-known/security.txt\nPolicy: " +
    origin +
    "/security/\nPreferred-Languages: zh, en\n",
);
await put(dir.site, "robots.txt", "User-agent: *\nAllow: /\nSitemap: " + origin + "/sitemap.xml\n");

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
