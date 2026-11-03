import { createHash } from "node:crypto";
import { existsSync, rmSync, watch } from "node:fs";
import {
  copyFile,
  cp,
  mkdir,
  mkdtemp,
  readFile,
  readdir,
  rename,
  rm,
  writeFile,
} from "node:fs/promises";
import { basename, dirname, join, relative, resolve } from "node:path";
import {
  documentPage,
  prepareContent,
  prepareDevelopmentEdition,
  workspaceDocuments,
} from "./content.mjs";
import { put, repoRoot, websiteRoot, workDir } from "./paths.mjs";
import { buildRelease } from "./release.mjs";

const devDir = join(workDir, "dev");
const prefix = "session-" + process.pid + "-";
const owns = (dir) =>
  dirname(dir) === devDir && basename(dir).startsWith(prefix);
const manifest = "site.json";

async function releaseData(session) {
  for (const path of [
    join(workDir, "dist/release.json"),
    join(devDir, "release.json"),
  ]) {
    try {
      return JSON.parse(await readFile(path, "utf8"));
    } catch (error) {
      if (error.code !== "ENOENT") throw error;
    }
  }
  console.log(
    "[website] Fetching and verifying release data for the first preview…",
  );
  const feeds = join(session, "feeds");
  await mkdir(feeds);
  const release = await buildRelease(
    repoRoot,
    join(workDir, "cache"),
    join(session, "downloads"),
    feeds,
  );
  const temp = join(session, "release.json");
  await writeFile(temp, JSON.stringify(release));
  await rename(temp, join(devDir, "release.json"));
  return release;
}

export async function prepareDevelopmentInput() {
  const existing = process.env.ARGUS_SITE_INPUT;
  if (existing && basename(existing) === "input" && owns(dirname(existing)))
    return existing;
  await mkdir(devDir, { recursive: true });
  // Windows may terminate a console process before exit handlers run.
  for (const entry of await readdir(devDir, { withFileTypes: true })) {
    const id = /^session-(\d+)-[a-zA-Z0-9]+$/.exec(entry.name)?.[1];
    const path = resolve(devDir, entry.name);
    if (!id || !entry.isDirectory() || dirname(path) !== devDir) continue;
    try {
      process.kill(Number(id), 0);
    } catch (error) {
      if (error.code === "ESRCH")
        await rm(path, { recursive: true, force: true });
    }
  }
  const session = await mkdtemp(join(devDir, prefix));
  if (!owns(session)) throw Error("Unexpected website development directory");
  process.once("exit", () => {
    try {
      rmSync(session, { recursive: true, force: true });
    } catch {
      /* Files may be locked on exit. */
    }
  });
  const input = join(session, "input");
  const release = await releaseData(session);
  console.log(
    "[website] Preparing " + release.tag + " and workspace documentation…",
  );
  const assets = await publicFiles();
  await openSnapshot(input, release, assets);
  startWatching(input, assets);
  process.env.ARGUS_SITE_INPUT = input;
  return input;
}

const localPath = (path) =>
  relative(repoRoot, resolve(path)).replaceAll("\\", "/");
const siteSources = new Set([
  "README.md",
  "README.en.md",
  "PRIVACY.md",
  "resources/logo.svg",
  "website/public-settings.json",
  "docs/images/screenshot-dark.png",
  "docs/images/screenshot-en-dark.png",
  "docs/images/screenshot-light.png",
  "docs/images/screenshot-en-light.png",
]);

async function publicFiles() {
  const entries = await readdir(join(websiteRoot, "public"), {
    recursive: true,
    withFileTypes: true,
  });
  return new Set(
    entries
      .filter((entry) => entry.isFile())
      .map((entry) => localPath(join(entry.parentPath, entry.name))),
  );
}

// Reuse the tagged handbook and product images. Always read development Markdown from the working tree.
async function openSnapshot(input, release, assets) {
  const hash = createHash("sha256").update(JSON.stringify(release));
  const paths = [
    ...siteSources,
    "website/scripts/content.mjs",
    "pnpm-lock.yaml",
    "website/src/components/magicui/LICENSE",
    ...assets,
  ].sort();
  for (const file of paths)
    hash
      .update(file)
      .update("\0")
      .update(await readFile(join(repoRoot, file)));
  const cache = join(devDir, "snapshots", hash.digest("hex"));
  if (existsSync(join(cache, manifest))) {
    await cp(cache, input, { recursive: true });
    const data = JSON.parse(await readFile(join(input, manifest), "utf8"));
    data.editions[1] = await prepareDevelopmentEdition(input, true);
    await put(input, manifest, JSON.stringify(data));
    return;
  }
  await prepareContent(input, release, { worktree: true });
  const seed = join(dirname(input), "seed");
  await cp(input, seed, {
    recursive: true,
    filter: (path) => path !== join(input, "content/docs/dev"),
  });
  await mkdir(dirname(cache), { recursive: true });
  try {
    await rename(seed, cache);
  } catch (error) {
    if (!existsSync(join(cache, manifest))) throw error;
  }
}

const isDocument = (file) =>
  file.startsWith("docs/") &&
  file.endsWith(".md") &&
  !file.endsWith("/AGENTS.md");

// Reconcile the whole list on rename: Windows can report the new name before the old name.
async function update(input, files, assets) {
  const data = JSON.parse(await readFile(join(input, manifest), "utf8"));
  const edition = data.editions.find((item) => item.base === "/docs/dev/");
  let changedRoutes = false;
  if (files.some((file) => isDocument(file) || file === "docs/")) {
    const current = new Set(await workspaceDocuments());
    for (const [file, route] of Object.entries(edition.routes)) {
      if (current.has(file)) continue;
      await rm(join(input, "content" + route + "index.md"), { force: true });
      delete edition.routes[file];
      changedRoutes = true;
    }
    for (const file of current) {
      if (
        edition.routes[file] &&
        !files.includes(file) &&
        !files.includes("docs/")
      )
        continue;
      const page = documentPage(
        file,
        await readFile(join(repoRoot, file), "utf8"),
        edition.base,
        edition.label,
      );
      await put(input, "content" + page.path + "index.md", page.markdown);
      if (!edition.routes[file]) {
        edition.routes[file] = page.path;
        changedRoutes = true;
      }
    }
  }
  const changedAssets = files.filter((file) =>
    file.startsWith("website/public/"),
  );
  if (changedAssets.length) {
    const current = await publicFiles();
    const output = (file) =>
      join(input, "public", file.slice("website/public/".length));
    for (const file of assets) {
      if (!current.has(file)) {
        await rm(output(file), { force: true });
        assets.delete(file);
      }
    }
    for (const file of current) {
      if (
        assets.has(file) &&
        !changedAssets.some(
          (changed) =>
            file === changed ||
            file.startsWith(changed.endsWith("/") ? changed : changed + "/"),
        )
      )
        continue;
      await mkdir(dirname(output(file)), { recursive: true });
      await copyFile(join(repoRoot, file), output(file));
      assets.add(file);
    }
  }
  if (files.some((file) => siteSources.has(file))) {
    await prepareContent(input, data.release, { worktree: true });
  } else if (changedRoutes) {
    edition.routes = Object.fromEntries(
      Object.entries(edition.routes).sort(([a], [b]) =>
        a < b ? -1 : a > b ? 1 : 0,
      ),
    );
    const path = join(input, manifest);
    await writeFile(path + ".tmp", JSON.stringify(data));
    await rename(path + ".tmp", path);
  }
}

// Keep watching the source tree while Astro restarts to load a changed sidebar.
function startWatching(input, assets) {
  const pending = new Set();
  let writes = Promise.resolve(),
    timer;
  function enqueue(file) {
    pending.add(file);
    clearTimeout(timer);
    timer = setTimeout(() => {
      const files = [...pending];
      pending.clear();
      writes = writes
        .then(() => update(input, files, assets))
        .catch((error) => console.error("[website]", error.message));
    }, 100);
  }
  for (const dir of ["docs", "website/public", "website", "resources", "."]) {
    const root = join(repoRoot, dir);
    watch(
      root,
      {
        recursive: dir === "docs" || dir === "website/public",
        persistent: false,
      },
      (event, name) => {
        if (!name) {
          if (dir === "docs" || dir === "website/public") enqueue(dir + "/");
          return;
        }
        const file = localPath(join(root, name));
        if (
          siteSources.has(file) ||
          isDocument(file) ||
          file.startsWith("website/public/")
        )
          enqueue(file);
        else if (
          dir === "docs" &&
          event === "rename" &&
          !basename(name).includes(".")
        )
          enqueue("docs/");
      },
    );
  }
}

export function watchDevelopmentInput(input) {
  const path = join(input, manifest);
  return {
    name: "argus-workspace-docs",
    hooks: {
      "astro:config:setup": ({ addWatchFile, updateConfig }) => {
        addWatchFile(path);
        addWatchFile(join(websiteRoot, "astro.config.mjs"));
        updateConfig({
          vite: {
            plugins: [
              {
                name: "argus-edition-sidebar",
                configureServer: (server) => {
                  server.watcher.add(path);
                },
              },
            ],
          },
        });
      },
    },
  };
}
