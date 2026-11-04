import { existsSync, readFileSync } from "node:fs";
import { mkdir, readFile, readdir, rename, rm, rmdir, writeFile } from "node:fs/promises";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

export const websiteRoot = fileURLToPath(new URL("../", import.meta.url));
export const repoRoot = resolve(websiteRoot, "..");
export const workDir = join(repoRoot, "target", "official-web");
// The pages read these generated folders; both are ignored by Git.
export const contentDir = join(websiteRoot, "content");
export const assetsDir = join(websiteRoot, "public", "assets");

// Raise when the shape of site.json changes, so content prepared by older scripts is rebuilt.
export const siteFormat = 6;

export async function put(base, relative, contents) {
  const file = join(base, relative);
  await mkdir(dirname(file), { recursive: true });
  await writeFile(file, contents);
}

// A running development server reloads every file that is written, so unchanged files are left alone.
export async function putIfChanged(base, relative, contents) {
  const file = join(base, relative);
  const bytes = Buffer.isBuffer(contents) ? contents : Buffer.from(contents);
  try {
    if (bytes.equals(await readFile(file))) return false;
  } catch (error) {
    if (error.code !== "ENOENT") throw error;
  }
  await mkdir(dirname(file), { recursive: true });
  // Readers never see a half-written file: write beside it, then rename over it.
  const temporary = file + "." + process.pid + ".tmp";
  await writeFile(temporary, bytes);
  await rename(temporary, file);
  return true;
}
export const putAtomic = putIfChanged;

// Remove files below `dir` that the last preparation did not write, then empty folders.
export async function prune(dir, keep) {
  if (!existsSync(dir)) return;
  for (const entry of await readdir(dir, { withFileTypes: true, recursive: true })) {
    const path = join(entry.parentPath, entry.name);
    if (entry.isFile() && !keep.has(path)) await rm(path, { force: true });
  }
  const folders = (await readdir(dir, { withFileTypes: true, recursive: true }))
    .filter((entry) => entry.isDirectory())
    .map((entry) => join(entry.parentPath, entry.name))
    .sort((a, b) => b.length - a.length);
  for (const folder of folders) await rmdir(folder).catch(() => {});
}

export function readSite(dir = contentDir) {
  const file = join(dir, "site.json");
  const data = JSON.parse(readFileSync(file, "utf8"));
  if (data.format !== siteFormat || !Array.isArray(data.editions))
    throw Error(
      `${file} was prepared by an older version of the website scripts. ` +
        "Restart start-website.bat, or run start-website.bat build to prepare it again.",
    );
  return data;
}
