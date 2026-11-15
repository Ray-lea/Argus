import assert from "node:assert/strict";
import { createHash, generateKeyPairSync, sign } from "node:crypto";
import { copyFile, mkdir, mkdtemp, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { basename, dirname, join, resolve } from "node:path";
import test from "node:test";
import { assetLimit, buildRelease, downloadOrigin, filenames, repo } from "../../website/scripts/release.mjs";
import { writeDistribution } from "../../website/scripts/distribution.mjs";
import { verifyLocalDownloads, verifyOnlineDownloads } from "../../website/scripts/verify-downloads.mjs";

const hash = body => createHash("sha256").update(body).digest("hex");
async function temporary(t) {
  const path = resolve(await mkdtemp(join(tmpdir(), "argus-downloads-test-")));
  t.after(async () => {
    assert.equal(dirname(path), resolve(tmpdir()));
    assert(basename(path).startsWith("argus-downloads-test-"));
    await rm(path, { recursive: true, force: true });
  });
  return path;
}
function fixture(beta = "1.1.0-beta.1") {
  const bodies = new Map();
  const downloads = [...new Set(["1.0.0", beta])].map(version => ({
    version,
    files: Object.fromEntries(Object.entries(filenames(version)).map(([kind, name]) => {
      const body = Buffer.from(`Fixture ${version} ${kind}`);
      const url = `${downloadOrigin}/v${version}/${name}`;
      bodies.set(url, body);
      return [kind, { name, size: body.length, sha256: hash(body), url, upstream: `${repo}/releases/download/v${version}/${name}` }];
    })),
  }));
  return { bodies, release: { ...downloads[0], tag: "v1.0.0", beta, downloads } };
}
function response(body, url, status = 200) {
  const result = new Response(body, { status });
  Object.defineProperty(result, "url", { value: url });
  return result;
}
const quiet = { wait: async () => {}, report() {} };

test("release building retains both channels through serialization and verifies each unique version", async t => {
  const root = await temporary(t);
  await mkdir(join(root, "scripts/lib"), { recursive: true });
  await mkdir(join(root, "resources"));
  const repoRoot = resolve(import.meta.dirname, "../..");
  for (const file of ["scripts/release.mjs", "scripts/lib/release-notes.ts", "resources/changelog.json"])
    await copyFile(join(repoRoot, file), join(root, file));
  // A disposable minisign key exercises the real signature verifier without
  // reading a release secret or changing the installed application's public key.
  const { publicKey, privateKey } = generateKeyPairSync("ed25519");
  const keyId = Buffer.alloc(8, 7);
  const prefix = Buffer.concat([Buffer.from("Ed"), keyId]);
  const publicPacket = Buffer.concat([prefix, Buffer.from(publicKey.export({ format: "jwk" }).x, "base64url")]);
  await writeFile(join(root, "resources/app.json"), JSON.stringify({
    updater: { pubkey: Buffer.from(`untrusted comment: fixture\n${publicPacket.toString("base64")}\n`).toString("base64") },
  }));
  function signature(body, version) {
    const signed = sign(null, body, privateKey);
    const comment = `timestamp:0\tversion:${version}`;
    return Buffer.from(`untrusted comment: fixture\n${Buffer.concat([prefix, signed]).toString("base64")}\ntrusted comment: ${comment}\n${sign(null, Buffer.concat([signed, Buffer.from(comment)]), privateKey).toString("base64")}\n`).toString("base64");
  }
  for (const beta of ["1.1.0-beta.1", "1.0.0"]) {
    const f = fixture(beta), upstream = new Map();
    upstream.set("https://api.github.com/repos/Ray-lea/Argus/releases/latest", JSON.stringify({ tag_name: "v1.0.0", published_at: "2026-01-01T00:00:00Z" }));
    for (const { version, files } of f.release.downloads) {
      for (const file of Object.values(files)) upstream.set(file.upstream, f.bodies.get(file.url));
      upstream.set(`${repo}/releases/download/v${version}/SHA256SUMS.txt`, Object.values(files).map(file => `${file.sha256}  ${file.name}\n`).join(""));
    }
    for (const [channel, version] of [["latest", "1.0.0"], ["beta", beta]]) {
      const file = f.release.downloads.find(item => item.version === version).files.installer;
      const platform = { url: file.upstream, signature: signature(f.bodies.get(file.url), version) };
      upstream.set(`${repo}/releases/download/v1.0.0/${channel}.json`, JSON.stringify({ version, platforms: { "windows-x86_64": platform, "windows-x86_64-nsis": platform } }));
    }
    const upstreamCalls = [];
    t.mock.method(globalThis, "fetch", async url => {
      upstreamCalls.push(url);
      assert(upstream.has(url), `Unexpected upstream request: ${url}`);
      return response(upstream.get(url), url);
    });
    const dist = join(root, beta);
    const dir = Object.fromEntries(["downloads", "updates", "www", "packages"].map(name => [name, join(dist, name)]));
    await mkdir(dir.updates, { recursive: true });
    const built = JSON.parse(JSON.stringify(await buildRelease(root, join(dist, "cache"), dir.downloads, dir.updates)));
    assert.deepEqual(built.downloads.map(item => item.version), f.release.downloads.map(item => item.version));
    assert.equal(built.files.installer.url, f.release.files.installer.url);
    assert.equal(upstreamCalls.filter(url => url.endsWith(".exe")).length, f.bodies.size);
    await writeDistribution(repoRoot, dir, built);
    await verifyLocalDownloads(dist, built);
    const checked = [];
    await verifyOnlineDownloads(built, { ...quiet, request: async url => {
      checked.push(url);
      assert(f.bodies.has(url));
      return response(f.bodies.get(url), url);
    } });
    assert.deepEqual(new Set(checked), new Set(f.bodies.keys()));
    assert.equal(checked.length, f.bodies.size);
  }
});

test("healthy stable downloads do not hide beta 404 or checksum failures", async () => {
  for (const failure of ["missing", "corrupt"]) {
    const f = fixture(), target = f.release.downloads[1].files.portable;
    await assert.rejects(verifyOnlineDownloads(f.release, { ...quiet, request: async url => {
      if (url === target.url)
        return response(failure === "missing" ? "Not Found" : Buffer.alloc(target.size, 0), url, failure === "missing" ? 404 : 200);
      return response(f.bodies.get(url), url);
    } }), error => {
      assert(error.message.includes(target.url));
      assert.match(error.cause.message, failure === "missing" ? /404/ : /checksum mismatch/);
      return true;
    });
  }
});

test("large beta assets follow upstream redirects and still require matching bytes", async () => {
  const f = fixture(), file = f.release.downloads[1].files.portable;
  const body = Buffer.alloc(assetLimit + 1, 1);
  Object.assign(file, { size: body.length, sha256: hash(body) });
  f.bodies.set(file.url, body);
  await verifyOnlineDownloads(f.release, { ...quiet, request: async url => response(f.bodies.get(url), url === file.url ? file.upstream : url) });
  await assert.rejects(verifyOnlineDownloads(f.release, { ...quiet, request: async url => response(url === file.url ? Buffer.from("corrupt") : f.bodies.get(url), url === file.url ? file.upstream : url) }), error => /size mismatch/.test(error.cause.message));
});

test("small beta downloads must remain on the mirror origin", async () => {
  const f = fixture(), file = f.release.downloads[1].files.installer;
  await assert.rejects(verifyOnlineDownloads(f.release, { ...quiet, request: async url => response(f.bodies.get(url), url === file.url ? file.upstream : url) }), error => /expected a mirrored asset/.test(error.cause.message));
});

test("local verification rejects a missing or corrupt beta asset and a missing large-file redirect", async t => {
  const dist = await temporary(t), f = fixture();
  for (const { version, files } of f.release.downloads) {
    const dir = join(dist, "downloads", "v" + version);
    await mkdir(dir, { recursive: true });
    await writeFile(join(dir, "SHA256SUMS.txt"), Object.values(files).map(file => `${file.sha256}  ${file.name}\n`).join(""));
    for (const file of Object.values(files)) await writeFile(join(dir, file.name), f.bodies.get(file.url));
  }
  await writeFile(join(dist, "downloads/_redirects"), "");
  await verifyLocalDownloads(dist, f.release);
  const file = f.release.downloads[1].files.portable;
  const path = join(dist, "downloads", "v" + f.release.beta, file.name);
  await rm(path);
  await assert.rejects(verifyLocalDownloads(dist, f.release), { code: "ENOENT" });
  await writeFile(path, Buffer.alloc(file.size, 0));
  await assert.rejects(verifyLocalDownloads(dist, f.release), /checksum mismatch/);
  await rm(path);
  file.size = assetLimit + 1;
  await assert.rejects(verifyLocalDownloads(dist, f.release), /missing upstream redirect/);
  await writeFile(join(dist, "downloads/_redirects"), `/v${f.release.beta}/${file.name} ${file.upstream} 302\n`);
  await verifyLocalDownloads(dist, f.release);
});

test("old or incomplete build metadata cannot silently skip the beta channel", async () => {
  const f = fixture();
  for (const downloads of [undefined, f.release.downloads.slice(0, 1)])
    await assert.rejects(verifyOnlineDownloads({ ...f.release, downloads }, { ...quiet, request: async () => assert.fail("must not fetch incomplete metadata") }), /Missing/);
});

test("partial beta file metadata fails before checking any download", async t => {
  const dist = await temporary(t);
  await mkdir(join(dist, "downloads"));
  await writeFile(join(dist, "downloads/_redirects"), "");
  for (const corrupt of [
    files => { delete files.installer; },
    files => { delete files.portable; },
    files => { delete files.installer; delete files.portable; },
    files => { files.portable.size = -1; },
    files => { files.portable.sha256 = "invalid"; },
    files => { files.portable.url = files.installer.url; },
    files => { files.portable.upstream = files.installer.upstream; },
  ]) {
    const f = fixture();
    corrupt(f.release.downloads[1].files);
    await assert.rejects(verifyOnlineDownloads(f.release, { ...quiet, request: async () => assert.fail("must validate all metadata before fetching") }), /metadata/);
    await assert.rejects(verifyLocalDownloads(dist, f.release), /metadata/);
  }
});
