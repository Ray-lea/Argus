import assert from "node:assert/strict";
import { parse } from "parse5";
import { createHash } from "node:crypto";
import { readFile, readdir, stat } from "node:fs/promises";
import { dirname, join, posix, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { setTimeout as delay } from "node:timers/promises";
import { languages, localized } from "../src/lib/languages.ts";
import {
  assetLimit,
  downloadOrigin,
  feedOrigin,
  filenames,
  origin,
  repo,
} from "./release.mjs";
import { verifyLocalDownloads, verifyOnlineDownloads } from "./verify-downloads.mjs";

const dist = resolve(
  dirname(fileURLToPath(import.meta.url)),
  "../../target/official-web/dist",
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
function inspectPage(source) {
  const ids = new Set(),
    links = [],
    alternates = new Map(),
    schemas = [];
  let canonical,
    policy,
    themeBoot = false;
  const executableHashes = [],
    scriptSources = [];
  function walk(node) {
    const attrs = Object.fromEntries(
      (node.attrs ?? []).map((a) => [a.name, a.value]),
    );
    if (attrs.id) ids.add(attrs.id);
    // next-themes applies the stored theme before the first paint.
    if (
      node.tagName === "script" &&
      !attrs.src &&
      node.childNodes.some((n) => n.value?.includes("argus-theme"))
    )
      themeBoot = true;
    if (node.tagName === "script" && attrs.src) scriptSources.push(attrs.src);
    for (const key of ["href", "src"])
      if (attrs[key]) links.push(attrs[key]);
    if (attrs.srcset && !attrs.srcset.startsWith("data:")) {
      for (const item of attrs.srcset.split(","))
        links.push(item.trim().split(/\s+/)[0]);
    }
    if (node.tagName === "link" && attrs.rel === "canonical")
      canonical = attrs.href;
    if (
      node.tagName === "meta" &&
      attrs["http-equiv"] === "content-security-policy"
    )
      policy = attrs.content;
    if (
      node.tagName === "script" &&
      !attrs.src &&
      (!attrs.type ||
        ["module", "text/javascript", "application/javascript"].includes(
          attrs.type,
        ))
    ) {
      const source = node.childNodes.map((n) => n.value ?? "").join("");
      executableHashes.push(
        "sha256-" + createHash("sha256").update(source).digest("base64"),
      );
    }
    if (node.tagName === "link" && attrs.hreflang)
      alternates.set(attrs.hreflang, attrs.href);
    if (node.tagName === "script" && attrs.type === "application/ld+json")
      schemas.push(
        JSON.parse(node.childNodes.map((n) => n.value ?? "").join("")),
      );
    for (const child of node.childNodes ?? []) walk(child);
  }
  walk(parse(source));
  return {
    ids,
    links,
    alternates,
    schemas,
    canonical,
    policy,
    executableHashes,
    scriptSources,
    themeBoot,
  };
}
function checkPageShell(page, url) {
  assert(page.themeBoot, url + ": missing theme script");
  const policy = page.policy
    ?.split(";")
    .find((d) => d.trim().startsWith("script-src "));
  assert(
    policy && !policy.includes("'unsafe-inline'"),
    url + ": invalid script policy",
  );
  for (const hash of page.executableHashes)
    assert(
      policy.includes("'" + hash + "'"),
      url + ": inline script blocked by CSP: " + hash,
    );
  for (const src of page.scriptSources)
    assert.equal(
      new URL(src, url).origin,
      origin,
      url + ": unexpected external script",
    );
}
async function local() {
  const dir = join(dist, "site");
  const siteFiles = new Set(await files(dir)),
    pages = new Map();
  for (const file of siteFiles) {
    if (file.endsWith(".html"))
      pages.set(file, inspectPage(await readFile(join(dir, file), "utf8")));
  }
  let links = 0;
  for (const [file, page] of pages) {
    const from = origin + "/" + file.replace(/index\.html$/, "");
    checkPageShell(page, from);
    for (const href of page.links) {
      const url = new URL(href, from);
      if (url.origin !== origin) continue;
      const target =
        decodeURIComponent(url.pathname).slice(1) +
        (url.pathname.endsWith("/") ? "index.html" : "");
      assert(siteFiles.has(target), file + ": missing " + href);
      if (url.hash && pages.has(target))
        assert(
          pages.get(target).ids.has(decodeURIComponent(url.hash.slice(1))),
          file + ": missing anchor " + href,
        );
      links++;
    }
  }
  for (const path of [
    "",
    "download/",
    "changelog/",
    "security/",
    "privacy/",
    "signing/",
  ]) {
    for (const { id } of languages) {
      const prefix = id === "zh" ? "" : id + "/";
      const page = pages.get(prefix + path + "index.html");
      assert(page, "Missing localized page " + prefix + path);
      assert.equal(page.canonical, origin + "/" + prefix + path);
      for (const alternate of languages)
        assert.equal(page.alternates.get(alternate.tag), origin + localized(alternate.id, "/" + path));
      if (path === "" || path === "download/") {
        const app = page.schemas.find(
          (s) => s["@type"] === "SoftwareApplication",
        );
        assert.equal(app?.softwareVersion, release.version);
        assert.equal(app.downloadUrl, release.files.installer.url);
        assert.equal(app.offers.price, "0");
      }
      if (path === "")
        assert(
          page.schemas.find((s) => s["@type"] === "FAQPage")?.mainEntity
            .length,
          prefix + ": missing FAQ structured data",
        );
    }
  }
  const { editions } = JSON.parse(
    await readFile(join(dist, "input/site.json"), "utf8"),
  );
  assert.deepEqual(
    editions.map((edition) => edition.base),
    ["/docs/", "/docs/dev/"],
  );
  const sitemap = await readFile(join(dir, "sitemap.xml"), "utf8");
  for (const edition of editions) {
    const index = pages.get(edition.base.slice(1) + "index.html");
    for (const [file, url] of Object.entries(edition.routes)) {
      const page = pages.get(url.slice(1) + "index.html");
      assert(page, "Missing document " + url);
      // Development pages that the stable edition also has defer to it.
      const canonical =
        (edition.id === "dev" && editions[0].routes[file]) || url;
      assert.equal(page.canonical, origin + canonical);
      assert.equal(
        sitemap.includes("<loc>" + origin + url + "</loc>"),
        canonical === url,
        "Sitemap entry for " + url,
      );
      assert(
        index.links.includes(url),
        "Document missing from navigation: " + url,
      );
      assert(page.schemas.some((s) => s["@type"] === "BreadcrumbList"), url);
      // "Copy Markdown" and "View as Markdown" read this file.
      assert(siteFiles.has("llms.mdx" + url + "content.md"), "Missing Markdown for " + url);
    }
  }
  const moved = await readFile(join(dir, "_redirects"), "utf8");
  for (const from of ["/questions/", "/en/questions/", "/search/"])
    assert(
      moved.split("\n").some((line) => line.startsWith(from + " ")),
      "Missing redirect for " + from,
    );
  // The search dialog filters by edition; both must be in the exported index.
  const search = await readFile(join(dir, "docs-search.json"), "utf8");
  assert(
    search.includes('"stable"') && search.includes('"dev"'),
    "The search index is missing an edition",
  );
  const llms = await readFile(join(dir, "llms.txt"), "utf8");
  assert(
    llms.startsWith("# Argus\n\n> ") && llms.includes(release.files.installer.url),
    "Invalid llms.txt",
  );
  for (const [, url] of llms.matchAll(/\]\((https:[^)]+)\)/g)) {
    const { pathname } = new URL(url);
    if (url.startsWith(origin))
      assert(
        siteFiles.has(
          decodeURIComponent(pathname).slice(1) +
            (pathname.endsWith("/") ? "index.html" : ""),
        ),
        "llms.txt: missing " + url,
      );
  }
  assert(siteFiles.has("llms-full.txt"), "Missing llms-full.txt");
  const security = await readFile(join(dir, ".well-known/security.txt"), "utf8");
  assert(
    security.includes("Expires:") &&
      security.includes(`Canonical: ${origin}/.well-known/security.txt`) &&
      security.includes(`${repo}/security/advisories/new`),
    "Invalid security.txt",
  );
  for (const service of ["site", "downloads", "updates", "www"]) {
    const items = await files(join(dist, service));
    assert(items.length <= 20000);
    for (const item of items)
      assert(
        (await stat(join(dist, service, item))).size <= assetLimit,
        `${service}/${item} exceeds Free asset size`,
      );
  }
  await verifyLocalDownloads(dist, release);
  for (const channel of ["latest", "beta"]) {
    const m = JSON.parse(
      await readFile(join(dist, "updates", channel + ".json"), "utf8"),
    );
    const expected = `${downloadOrigin}/v${m.version}/${filenames(m.version).installer}`;
    for (const key of ["windows-x86_64", "windows-x86_64-nsis"])
      assert.equal(m.platforms[key].url, expected);
  }
  console.log(
    `Validated ${pages.size} HTML files, ${links} internal links and anchors, verified downloads and Free limits.`,
  );
}

async function online(service) {
  if (service === "downloads") return verifyOnlineDownloads(release);
  const targets =
    service === "updates"
      ? ["latest", "beta"].map((c) => [`${feedOrigin}/${c}.json`, null])
      : service === "www"
        ? [["https://www.argustask.com/download/", null]]
        : [
            ...languages.flatMap(({ id }) => ["/", "/download/", "/changelog/"].map(path => [origin + localized(id, path), null])),
            [origin + "/docs/guide/", null],
            [origin + "/docs/dev/", null],
            [origin + "/docs-search.json", null],
            [origin + "/.well-known/security.txt", null],
          ];
  for (const [url] of targets) {
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
          if (service === "updates") {
            const name = new URL(url).pathname.slice(1);
            assert.deepEqual(
              await res.json(),
              JSON.parse(await readFile(join(dist, "updates", name), "utf8")),
            );
          } else {
            if (url.endsWith("/"))
              assert(
                res.headers.get("cache-control")?.includes("no-transform"),
                url + ": missing HTML cache policy",
              );
            // Exactly the files local() checked: an older deployment or an edge
            // rewrite would also break the CSP hashes of the inline scripts.
            const { pathname } = new URL(url);
            const built = await readFile(
              join(
                dist,
                "site",
                decodeURIComponent(pathname) +
                  (pathname.endsWith("/") ? "index.html" : ""),
              ),
            );
            const body = Buffer.from(await res.arrayBuffer());
            assert(
              body.equals(built),
              `${url}: serves ${body.length} bytes that differ from the ${built.length}-byte build`,
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
