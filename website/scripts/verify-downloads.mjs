import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { readFile } from "node:fs/promises";
import { join } from "node:path";
import { setTimeout as delay } from "node:timers/promises";
import { assetLimit, downloadOrigin, filenames, readChecksums, repo } from "./release.mjs";

function channelDownloads(release) {
  assert(Array.isArray(release.downloads), "Missing channel download metadata; rebuild the website");
  const versions = new Map(release.downloads.map(item => [item.version, item]));
  return [...new Set([release.version, release.beta])].map(version => {
    assert(versions.has(version), `Missing downloads for channel version ${version}`);
    const item = versions.get(version);
    const expected = filenames(version);
    assert.equal(Object.keys(item.files ?? {}).length, Object.keys(expected).length, `Incomplete download metadata for ${version}`);
    for (const [kind, name] of Object.entries(expected)) {
      const file = item.files?.[kind];
      assert(file && file.name === name, `Missing ${kind} download metadata for ${version}`);
      assert(Number.isSafeInteger(file.size) && file.size > 0, `Invalid size metadata for ${version}/${kind}`);
      assert(/^[0-9a-f]{64}$/.test(file.sha256 ?? ""), `Invalid checksum metadata for ${version}/${kind}`);
      assert.equal(file.url, `${downloadOrigin}/v${version}/${name}`, `Invalid mirror metadata for ${version}/${kind}`);
      assert.equal(file.upstream, `${repo}/releases/download/v${version}/${name}`, `Invalid upstream metadata for ${version}/${kind}`);
    }
    return item;
  });
}

export async function verifyLocalDownloads(dist, release) {
  const redirects = await readFile(join(dist, "downloads", "_redirects"), "utf8");
  for (const { version, files } of channelDownloads(release)) {
    const tag = "v" + version;
    const sums = readChecksums(
      await readFile(join(dist, "downloads", tag, "SHA256SUMS.txt"), "utf8"),
      Object.values(files).map(file => file.name),
    );
    for (const file of Object.values(files)) {
      assert.equal(sums.get(file.name), file.sha256, `${tag}/${file.name}: checksum metadata mismatch`);
      if (file.size <= assetLimit) {
        const body = await readFile(join(dist, "downloads", tag, file.name));
        assert.equal(body.length, file.size, `${tag}/${file.name}: size mismatch`);
        assert.equal(createHash("sha256").update(body).digest("hex"), file.sha256, `${tag}/${file.name}: checksum mismatch`);
      } else {
        assert(redirects.split(/\r?\n/).includes(`/${tag}/${file.name} ${file.upstream} 302`), `${tag}/${file.name}: missing upstream redirect`);
      }
    }
  }
}

export async function verifyOnlineDownloads(release, {
  request = fetch, wait = delay, report = console.log,
} = {}) {
  for (const { files } of channelDownloads(release)) {
    for (const file of Object.values(files)) {
      let last;
      for (let attempt = 0; attempt < 6; attempt++) {
        let response;
        try {
          response = await request(file.url, {
            redirect: "follow", signal: AbortSignal.timeout(20000),
          });
          if (response.status !== 200) {
            await response.body?.cancel();
            throw Error(`${response.status} fetching ${file.url}`);
          }
          if (file.size <= assetLimit)
            assert.equal(new URL(response.url).origin, downloadOrigin, `${file.url}: expected a mirrored asset`);
          const digest = createHash("sha256");
          let size = 0;
          for await (const block of response.body) {
            size += block.length;
            assert(size <= file.size, `${file.url}: size mismatch`);
            digest.update(block);
          }
          assert.equal(size, file.size, `${file.url}: size mismatch`);
          assert.equal(digest.digest("hex"), file.sha256, `${file.url}: checksum mismatch`);
          report(`Verified ${file.url}`);
          last = null;
          break;
        } catch (error) {
          await response?.body?.cancel().catch(() => {});
          last = error;
          if (attempt < 5) await wait(Math.min(10000, 1200 * 2 ** attempt));
        }
      }
      if (last) throw Error(`Public verification failed: ${file.url}`, { cause: last });
    }
  }
}
