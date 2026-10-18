// When toasts leave: their time, the pointer and keyboard focus holding them.

import assert from "node:assert/strict";
import { beforeEach, mock, test } from "node:test";
import { LINGER_MS, toastClock } from "../src/lib/toast-clock.ts";

let expired: number[] = [];
const clock = () => toastClock((id) => expired.push(id));

beforeEach(() => {
  expired = [];
  mock.timers.reset();
  mock.timers.enable({ apis: ["setTimeout"] });
});

test("a toast nothing holds leaves after its time", () => {
  const c = clock();
  c.start(1, 10_000);
  mock.timers.tick(9_999);
  assert.deepEqual(expired, []);
  mock.timers.tick(1);
  assert.deepEqual(expired, [1]);
});

test("focus on a button keeps a toast past its time", () => {
  const c = clock();
  c.start(1, 10_000);
  mock.timers.tick(4_000);
  c.hold(1, "focus", true);
  mock.timers.tick(60_000);
  assert.deepEqual(expired, []);
  // Focus moving on: it lingers a moment, like after the pointer.
  c.hold(1, "focus", false);
  mock.timers.tick(LINGER_MS - 1);
  assert.deepEqual(expired, []);
  mock.timers.tick(1);
  assert.deepEqual(expired, [1]);
});

test("the pointer leaving does not let go of a toast focus is still in", () => {
  const c = clock();
  c.start(1, 10_000);
  c.hold(1, "pointer", true);
  c.hold(1, "focus", true);
  c.hold(1, "pointer", false);
  mock.timers.tick(60_000);
  assert.deepEqual(expired, []);
  c.hold(1, "focus", false);
  mock.timers.tick(LINGER_MS);
  assert.deepEqual(expired, [1]);
});

test("focus leaving does not let go of a toast the pointer is on", () => {
  const c = clock();
  c.start(1, 10_000);
  c.hold(1, "focus", true);
  c.hold(1, "pointer", true);
  c.hold(1, "focus", false);
  mock.timers.tick(60_000);
  assert.deepEqual(expired, []);
  c.hold(1, "pointer", false);
  mock.timers.tick(LINGER_MS);
  assert.deepEqual(expired, [1]);
});

test("a release without a hold changes nothing", () => {
  const c = clock();
  c.start(1, 10_000);
  // Focus leaving a toast it never entered must not cut its time short.
  c.hold(1, "focus", false);
  mock.timers.tick(LINGER_MS);
  assert.deepEqual(expired, []);
  mock.timers.tick(10_000 - LINGER_MS);
  assert.deepEqual(expired, [1]);
});

test("a repeated hold does not need two releases", () => {
  const c = clock();
  c.start(1, 10_000);
  c.hold(1, "focus", true);
  c.hold(1, "focus", true);
  c.hold(1, "focus", false);
  mock.timers.tick(LINGER_MS);
  assert.deepEqual(expired, [1]);
});

test("a forgotten toast neither expires nor comes back by a hold", () => {
  const c = clock();
  c.start(1, 10_000);
  c.start(2, 10_000);
  c.forget(1);
  // Focus moving out of a toast as it is removed.
  c.hold(1, "focus", true);
  c.hold(1, "focus", false);
  mock.timers.tick(60_000);
  assert.deepEqual(expired, [2]);
});
