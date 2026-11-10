import assert from "node:assert/strict";
import test from "node:test";
import { deployWithRetry, transientFailure } from "../../website/scripts/deploy.mjs";

test("the observed Cloudflare HTML 504 failure retries the same deployment", async () => {
  const results = [{ code: 1, output: "Received a malformed response from the API\n<!DOCTYPE html>\nPOST /workers/scripts/argustask-downloads/versions -> 504 Gateway Timeout" }, { code: 0, output: "Deployed" }];
  const waits = [];
  let calls = 0;
  await deployWithRetry(async () => results[calls++], { wait: async ms => waits.push(ms), report() {} });
  assert.equal(calls, 2);
  assert.deepEqual(waits, [5000]);
});
test("authentication and configuration failures stop immediately", async () => {
  for (const output of ["Authentication error [code: 10000]", "HTTP 403 Forbidden", "Invalid configuration", "Authentication error after a 504 Gateway Timeout"]) {
    assert.equal(transientFailure(output), false);
    let calls = 0;
    await assert.rejects(deployWithRetry(async () => { calls++; return { code: 1, output }; }, { wait: async () => assert.fail("must not wait") }), /failed/);
    assert.equal(calls, 1);
  }
});
test("continued transport failures stop after the retry budget", async () => {
  let calls = 0;
  const waits = [];
  await assert.rejects(deployWithRetry(async () => { calls++; return { code: 1, output: "ECONNRESET" }; }, { wait: async ms => waits.push(ms), report() {} }), /attempt 3\/3/);
  assert.equal(calls, 3);
  assert.deepEqual(waits, [5000, 15000]);
});
