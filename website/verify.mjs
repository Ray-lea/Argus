import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { readFile, readdir, stat } from "node:fs/promises";
import { dirname, join, posix, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { setTimeout as delay } from "node:timers/promises";
import {
  assetLimit,
  downloadOrigin,
  feedOrigin,
  filenames,
  origin,
} from "./release.mjs";

const dist = resolve(
  dirname(fileURLToPath(import.meta.url)),
  "../target/official-web/dist",
);
const release = JSON.parse(await readFile(join(dist, "release.json"), "utf8"));
async function files(dir, prefix = "") {
  const all = [];
  for (const entry of await readdir(join(dir, prefix), {
    withFileTypes: true,
  })) {
    const path = posix.join(prefix, entry.name);
    if (entry.isDirectory()) all.push(...(await files(dir, path)));
    else if (entry.isFile()) all.push(path);
  }
  return all;
}
function attrs(html, key) {
  return [...html.matchAll(new RegExp(`\\b${key}="([^"]+)"`, "g"))].map((m) =>
    m[1].replaceAll("&amp;", "&"),
  );
}
async function local() {
  const dir = join(dist, "site"),
    siteFiles = new Set(await files(dir));
  const anchors = new Map(),
    html = new Map();
  for (const file of siteFiles)
    if (file.endsWith(".html")) {
      const page = await readFile(join(dir, file), "utf8");
      html.set(file, page);
      anchors.set(file, new Set(attrs(page, "id")));
    }
  let links = 0;
  for (const [file, page] of html) {
    for (const link of [
      ...attrs(page, "href"),
      ...attrs(page, "src"),
      ...attrs(page, "srcset"),
    ]) {
      const url = new URL(
        link,
        origin + "/" + file.replace(/index\.html$/, ""),
      );
      if (url.origin !== origin) continue;
      const target =
        decodeURIComponent(url.pathname).slice(1) +
        (url.pathname.endsWith("/") ? "index.html" : "");
      assert(siteFiles.has(target), `${file}: missing ${link}`);
      if (url.hash && anchors.has(target))
        assert(
          anchors.get(target).has(decodeURIComponent(url.hash.slice(1))),
          `${file}: missing anchor ${link}`,
        );
      links++;
    }
  }
  for (const service of ["site", "downloads", "updates", "www"]) {
    const items = await files(join(dist, service));
    assert(items.length <= 20000);
    for (const item of items)
      assert(
        (await stat(join(dist, service, item))).size <= assetLimit,
        `${service}/${item} exceeds Free asset size`,
      );
  }
  const sums = await readFile(
    join(dist, "downloads", release.tag, "SHA256SUMS.txt"),
    "utf8",
  );
  const redirects = await readFile(
    join(dist, "downloads", "_redirects"),
    "utf8",
  );
  for (const f of Object.values(release.files)) {
    assert(sums.includes(f.sha256));
    if (f.size <= assetLimit)
      assert.equal(
        createHash("sha256")
          .update(await readFile(join(dist, "downloads", release.tag, f.name)))
          .digest("hex"),
        f.sha256,
      );
    else
      assert(redirects.includes(`/${release.tag}/${f.name} ${f.upstream} 302`));
  }
  for (const channel of ["latest", "beta"]) {
    const m = JSON.parse(
      await readFile(join(dist, "updates", channel + ".json"), "utf8"),
    );
    const expected = `${downloadOrigin}/v${m.version}/${filenames(m.version).installer}`;
    for (const key of ["windows-x86_64", "windows-x86_64-nsis"])
      assert.equal(m.platforms[key].url, expected);
  }
  console.log(
    `Validated ${html.size} HTML files, ${links} internal links and anchors, verified downloads and Free limits.`,
  );
}

async function online(service) {
  const targets =
    service === "downloads"
      ? Object.values(release.files).map((f) => [f.url, f.sha256, f.size <= assetLimit])
      : service === "updates"
        ? ["latest", "beta"].map((c) => [`${feedOrigin}/${c}.json`, null])
        : service === "www"
          ? [["https://www.argustask.com/download/", null]]
          : [
              [origin + "/", null],
              [origin + "/en/download/", null],
              [origin + "/docs/guide/", null],
              [origin + "/.well-known/security.txt", null],
            ];
  for (const [url, hash, mirrored] of targets) {
    let last;
    for (let attempt = 0; attempt < 6; attempt++) {
      try {
        const res = await fetch(url, {
          redirect: service === "www" ? "manual" : "follow",
          signal: AbortSignal.timeout(20000),
        });
        if (service === "www") {
          assert([301, 308].includes(res.status));
          assert.equal(res.headers.get("location"), origin + "/download/");
        } else {
          assert.equal(res.status, 200, url);
          if (hash) {
            if (mirrored) assert.equal(new URL(res.url).origin, downloadOrigin);
            const digest = createHash("sha256");
            for await (const block of res.body) digest.update(block);
            assert.equal(digest.digest("hex"), hash);
          } else if (service === "updates") {
            const name = new URL(url).pathname.slice(1);
            assert.deepEqual(
              await res.json(),
              JSON.parse(await readFile(join(dist, "updates", name), "utf8")),
            );
          } else {
            const body = await res.text();
            assert(
              url.endsWith("security.txt")
                ? body.includes("Expires:") && body.includes(`Canonical: ${origin}/.well-known/security.txt`) && body.includes("https://github.com/Ray-lea/Argus/security/advisories/new")
                : body.includes("Argus") && body.includes("<main"),
            );
          }
        }
        console.log(`Verified ${url}`);
        last = null;
        break;
      } catch (error) {
        last = error;
        if (attempt < 5) await delay(Math.min(10000, 1200 * 2 ** attempt));
      }
    }
    if (last)
      throw Error(`Public verification failed: ${url}`, { cause: last });
  }
}
const service = process.argv[2];
if (service && !["site", "downloads", "updates", "www"].includes(service))
  throw Error("Unknown verification target");
if (service) await online(service);
else await local();
