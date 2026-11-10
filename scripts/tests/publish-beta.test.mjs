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
    assert.equal(url, "https://api.github.com/repos/example/Argus/releases/latest");
    return Response.json({ tag_name: "v1.0.0", draft: false, prerelease: false });
  });
  assert.equal(await publishBeta(f.path, f.options), true);
  assert.deepEqual(f.uploads, [{ tag: "v1.0.0", text: f.manifest }]);
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
