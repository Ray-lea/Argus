import assert from "node:assert/strict";
import { mkdtemp, readFile, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import test from "node:test";
import { publishBeta } from "../publish-beta.mjs";

async function fixture(t, request) {
  const dir = await mkdtemp(join(tmpdir(), "argus-beta-publish-"));
  t.after(() => rm(dir, { recursive: true, force: true }));
  const manifest = JSON.stringify({ version: "1.1.0-beta.1", platforms: {} });
  const path = join(dir, "manifest.json");
  await writeFile(path, manifest);
  const uploads = [], messages = [];
  return {
    path, manifest, uploads, messages,
    options: {
      repository: "example/Argus", token: "test-token", request,
      upload: async (tag, path) => uploads.push({ tag, text: await readFile(path, "utf8") }),
      report: message => messages.push(message),
    },
  };
}

test("beta publication uploads the manifest to the latest stable release", async t => {
  const f = await fixture(t, async url => {
    if (url.endsWith("/releases/assets/42")) return Response.json({ version: "1.0.0" });
    assert.equal(url, "https://api.github.com/repos/example/Argus/releases/latest");
    return Response.json({ tag_name: "v1.0.0", draft: false, prerelease: false, assets: [{ name: "beta.json", id: 42 }] });
  });
  assert.equal(await publishBeta(f.path, f.options), true);
  assert.deepEqual(f.uploads, [{ tag: "v1.0.0", text: f.manifest }]);
});

test("out-of-order prereleases preserve the newest beta manifest", async t => {
  let current = "1.0.0";
  const f = await fixture(t, async url => url.endsWith("/releases/latest")
    ? Response.json({ tag_name: "v1.0.0", assets: [{ name: "beta.json", id: 42 }] })
    : Response.json({ version: current }));
  f.options.upload = async (_tag, path) => {
    current = JSON.parse(await readFile(path, "utf8")).version;
    f.uploads.push(current);
  };
  for (const version of ["1.1.0-beta.2", "1.1.0-beta.1", "1.1.0-beta.2"] ) {
    await writeFile(f.path, JSON.stringify({ version }));
    await publishBeta(f.path, f.options);
  }
  assert.equal(current, "1.1.0-beta.2");
  assert.deepEqual(f.uploads, ["1.1.0-beta.2"]);
});

test("channel precedence handles numeric identifiers, stable promotion and build metadata", async t => {
  for (const [candidate, current, expected] of [
    ["1.1.0-beta.10", "1.1.0-beta.9", true],
    ["1.1.0-beta.2", "1.1.0-beta.10", false],
    ["1.1.0-beta.2", "1.1.0", false],
    ["1.1.0-beta.1", "1.2.0-beta.1", false],
    ["1.1.0-beta.1", "1.1.0-beta", true],
    ["1.1.0-1", "1.1.0-alpha", false],
    ["1.1.0-beta.1+new", "1.1.0-beta.1+old", false],
  ]) {
    const f = await fixture(t, async url => url.endsWith("/releases/latest")
      ? Response.json({ tag_name: "v1.0.0", assets: [{ name: "beta.json", id: 42 }] })
      : Response.json({ version: current }));
    await writeFile(f.path, JSON.stringify({ version: candidate }));
    assert.equal(await publishBeta(f.path, f.options), expected, `${candidate} vs ${current}`);
    assert.equal(f.uploads.length, Number(expected));
  }
  const f = await fixture(t, async () => Response.json({ tag_name: "v1.1.0" }));
  assert.equal(await publishBeta(f.path, f.options), false);
  assert.deepEqual(f.uploads, []);
});

test("missing, unreadable or invalid current beta manifests fail without overwriting", async t => {
  for (const assetResponse of [
    null,
    () => new Response("Not Found", { status: 404 }),
    () => new Response("Unavailable", { status: 503 }),
    () => new Response("Invalid JSON"),
    () => Response.json({ version: "1.1.0-beta.01" }),
    () => Response.json({}),
  ]) {
    const f = await fixture(t, async url => url.endsWith("/releases/latest")
      ? Response.json({ tag_name: "v1.0.0", assets: assetResponse ? [{ name: "beta.json", id: 42 }] : [] })
      : assetResponse());
    await assert.rejects(publishBeta(f.path, f.options));
    assert.deepEqual(f.uploads, []);
  }
});

test("a confirmed missing stable release warns without uploading", async t => {
  const f = await fixture(t, async () => new Response("Not Found", { status: 404 }));
  assert.equal(await publishBeta(f.path, f.options), false);
  assert.deepEqual(f.uploads, []);
  assert.match(f.messages[0], /^::warning::/);
});

test("GitHub API, transport and malformed response failures stop beta publication", async t => {
  for (const request of [
    async () => new Response("Forbidden", { status: 403 }),
    async () => new Response("Service Unavailable", { status: 503 }),
    async () => { throw Error("Connection reset"); },
    async () => new Response("Invalid JSON"),
    async () => Response.json({}),
  ]) {
    const f = await fixture(t, request);
    await assert.rejects(publishBeta(f.path, f.options));
    assert.deepEqual(f.uploads, []);
    assert.deepEqual(f.messages, []);
  }
});
