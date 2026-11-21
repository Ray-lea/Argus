import assert from "node:assert/strict";
import { test } from "node:test";
import { listHunks, selectPatch, splitPatch } from "../lib/patch-part.mjs";

const DIFF = [
  "diff --git a/src/a.rs b/src/a.rs",
  "index 1111111..2222222 100644",
  "--- a/src/a.rs",
  "+++ b/src/a.rs",
  "@@ -1,4 +1,5 @@",
  " fn first() {",
  "-    old_call();",
  "+    new_call();",
  "+    other_work();",
  " }",
  " ",
  "@@ -20,3 +21,4 @@ fn second() {",
  "     keep();",
  "+    theirs();",
  "     tail();",
  " }",
  "",
].join("\n");

test("a diff splits into its header and hunks", () => {
  const { head, hunks } = splitPatch(DIFF);
  assert.equal(head.length, 4);
  assert.deepEqual(hunks.map((hunk) => hunk.length), [7, 5]);
  const list = listHunks(DIFF, 2);
  assert.equal(list.length, 2);
  assert.match(list[0], /^ {2}1 {2}@@ -1,4 \+1,5 @@\n {5}-    old_call\(\);\n {5}\+    new_call\(\);\n {5}… 1 more changed lines$/);
});

test("chosen hunks are kept whole", () => {
  const { patch, kept } = selectPatch(DIFF, [1]);
  assert.equal(kept, 1);
  assert.equal(patch, `${DIFF.split("\n").slice(0, 11).join("\n")}\n`);
  assert.equal(selectPatch(DIFF, "all").patch, DIFF);
  assert.throws(() => selectPatch(DIFF, [3]), /no hunk 3/);
});

test("left-out lines stay as they are in the index", () => {
  const { patch, kept } = selectPatch(DIFF, "all", ["other_work", "theirs"]);
  assert.equal(kept, 1, "the second hunk has no change left");
  assert.deepEqual(patch.split("\n").slice(4), ["@@ -1,4 +1,5 @@", " fn first() {", "-    old_call();", "+    new_call();", " }", " ", ""]);
  // A removed line that is left out becomes context: it stays in the file.
  assert.deepEqual(selectPatch(DIFF, [1], ["old_call"]).patch.split("\n").slice(5, 9), [" fn first() {", "     old_call();", "+    new_call();", "+    other_work();"]);
  assert.throws(() => selectPatch(DIFF, [1], ["theirs"]), /no changed line/);
  assert.deepEqual(selectPatch(DIFF, [2], ["theirs"]), { patch: "", kept: 0 });
});
