// What failed operations say, and when their notice offers to update the
// privileged service.

import assert from "node:assert/strict";
import { test } from "node:test";
import { actionFailure, endFailure, needsServiceUpdate, telling } from "../src/lib/action-errors.ts";

const denied = { kind: "accessDenied" };
const outdated = { kind: "serviceOutdated" };
const gone = { kind: "notFound" };

test("a denial while the service is outdated has its own message", () => {
  assert.deepEqual(endFailure(outdated), { key: "actions.error.serviceOutdated" });
  assert.deepEqual(actionFailure(outdated), { key: "actions.failed.serviceOutdated" });
  assert.deepEqual(endFailure(denied), { key: "actions.error.accessDenied" });
  assert.deepEqual(actionFailure(denied), { key: "actions.failed.accessDenied" });
});

test("other failures keep their messages", () => {
  assert.deepEqual(endFailure({ kind: "critical" }), { key: "actions.error.critical" });
  assert.deepEqual(actionFailure(gone), { key: "actions.failed.notFound" });
  assert.deepEqual(actionFailure({ kind: "os", message: "boom" }), {
    key: "actions.failed.generic",
    error: "boom",
  });
  // A command that failed before reaching Argus rejects with plain text.
  assert.deepEqual(endFailure("no such command"), { key: "actions.error.generic", error: "no such command" });
});

test("only a denial while the service is outdated offers its update", () => {
  assert.equal(needsServiceUpdate([outdated]), true);
  assert.equal(needsServiceUpdate([gone, outdated]), true);
  assert.equal(needsServiceUpdate([denied, gone]), false);
  assert.equal(needsServiceUpdate(["text", null, undefined]), false);
  assert.equal(needsServiceUpdate([]), false);
});

test("of several failures, the one an update would help is reported", () => {
  assert.equal(telling([gone, outdated, denied]), outdated);
  assert.equal(telling([gone, denied]), gone);
});
