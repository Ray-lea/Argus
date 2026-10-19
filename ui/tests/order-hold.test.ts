// What holds the process list's order, and when the status bar names the pointer.

import assert from "node:assert/strict";
import { test } from "node:test";
import { holdsOrder, keepOrder, type OrderHolds, pointerAloneHoldsOrder } from "../src/lib/order-hold.ts";

/** Every combination of the three holds. */
const all: OrderHolds[] = [false, true].flatMap((pointer) =>
  [false, true].flatMap((menu) => [false, true].map((dialog) => ({ pointer, menu, dialog }))),
);

test("a late category change cannot leave a Windows process below the background heading", () => {
  const row = (key: string) => ({ key, kind: key.startsWith("s:") ? "section" : "process" });
  const before = ["s:apps", "app", "s:background", "moving", "bg", "s:windows", "os"].map(row);
  const after = ["s:apps", "app", "s:background", "bg", "s:windows", "moving", "os"].map(row);
  assert.deepEqual(keepOrder(after, before), after);
  assert.deepEqual(
    keepOrder(
      after.filter((r) => !["bg", "s:background"].includes(r.key)),
      before,
    ),
    after.filter((r) => !["bg", "s:background"].includes(r.key)),
  );
});

test("held numeric order and newly expanded members still follow their previous anchors", () => {
  const row = (key: string) => ({ key, kind: "process" });
  const before = ["a", "b", "c"].map(row);
  const fresh = ["c", "b", "a", "child"].map(row);
  assert.deepEqual(
    keepOrder(fresh, before).map((r) => r.key),
    ["a", "child", "b", "c"],
  );
});

test("the pointer, a menu or a dialog each hold the order; nothing lets rows re-sort", () => {
  assert.equal(holdsOrder({ pointer: false, menu: false, dialog: false }), false);
  assert.equal(holdsOrder({ pointer: true, menu: false, dialog: false }), true);
  assert.equal(holdsOrder({ pointer: false, menu: true, dialog: false }), true);
  assert.equal(holdsOrder({ pointer: false, menu: false, dialog: true }), true);
});

test("the status bar names the pointer only when it alone holds the order", () => {
  for (const h of all) {
    const alone = h.pointer && !h.menu && !h.dialog;
    assert.equal(pointerAloneHoldsOrder(h), alone, JSON.stringify(h));
    // Whenever the pointer is named, the order is in fact held.
    if (alone) assert.equal(holdsOrder(h), true);
  }
});

test("a confirmation holds the order without the pointer hint", () => {
  // The onboarding's interval confirmation: the pointer is on the dialog.
  const onDialog = { pointer: false, menu: false, dialog: true };
  assert.equal(holdsOrder(onDialog), true);
  assert.equal(pointerAloneHoldsOrder(onDialog), false);
  // Delete pressed while the pointer rests on the list.
  assert.equal(pointerAloneHoldsOrder({ pointer: true, menu: false, dialog: true }), false);
});

test("an open context menu holds the order without the pointer hint", () => {
  assert.equal(pointerAloneHoldsOrder({ pointer: true, menu: true, dialog: false }), false);
  assert.equal(pointerAloneHoldsOrder({ pointer: false, menu: true, dialog: false }), false);
});
