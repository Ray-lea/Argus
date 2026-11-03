import { mkdir, writeFile } from "node:fs/promises";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

export const websiteRoot = fileURLToPath(new URL("../", import.meta.url));
export const repoRoot = resolve(websiteRoot, "..");
export const workDir = join(repoRoot, "target", "official-web");
export const siteInput = () =>
  process.env.ARGUS_SITE_INPUT ?? join(workDir, "dist", "input");

export async function put(base, relative, contents) {
  const file = join(base, relative);
  await mkdir(dirname(file), { recursive: true });
  await writeFile(file, contents);
}
