import assert from "node:assert/strict";
import { beforeEach, mock, test } from "node:test";
import { pollWhileVisible } from "../src/lib/visible-poll.ts";

class Visibility extends EventTarget {
  visibilityState: DocumentVisibilityState = "visible";
  change(state: DocumentVisibilityState) {
    this.visibilityState = state;
    this.dispatchEvent(new Event("visibilitychange"));
  }
}

beforeEach(() => {
  mock.timers.reset();
  mock.timers.enable({ apis: ["setInterval"] });
});

test("page polling sleeps while hidden and refreshes immediately on return", () => {
  const visibility = new Visibility();
  let calls = 0;
  const stop = pollWhileVisible(() => calls++, 2000, visibility);
  assert.equal(calls, 1);
  mock.timers.tick(2000);
  assert.equal(calls, 2);
  visibility.change("hidden");
  mock.timers.tick(60000);
  assert.equal(calls, 2);
  visibility.change("visible");
  assert.equal(calls, 3);
  mock.timers.tick(1999);
  assert.equal(calls, 3);
  mock.timers.tick(1);
  assert.equal(calls, 4);
  stop();
  mock.timers.tick(10000);
  visibility.change("hidden");
  visibility.change("visible");
  assert.equal(calls, 4);
});

test("a page mounted while hidden waits for visibility and cleans up before returning", () => {
  const visibility = new Visibility();
  visibility.change("hidden");
  let calls = 0;
  const stop = pollWhileVisible(() => calls++, 3000, visibility);
  mock.timers.tick(60000);
  assert.equal(calls, 0);
  stop();
  visibility.change("visible");
  mock.timers.tick(60000);
  assert.equal(calls, 0);
});
