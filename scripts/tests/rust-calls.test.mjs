import assert from "node:assert/strict";
import { test } from "node:test";
import { macroCalls, topLevelArguments } from "../lib/rust-calls.mjs";

test("a call nested in another's arguments is read on its own", () => {
  const source = `let exact = tr!(
        "performance.withAlt",
        value = fmt(bytes),
        alt = tr!("performance.disk.bytes", count = grouped(&details["capacityBytes"]))
    );`;
  assert.deepEqual(
    macroCalls(source, "tr").map(({ key, names }) => ({ key, names })),
    [
      { key: "performance.withAlt", names: ["value", "alt"] },
      { key: "performance.disk.bytes", names: ["count"] },
    ],
  );
});

test("commas and brackets inside strings and closures do not split arguments", () => {
  assert.deepEqual(topLevelArguments(`"k", a = format!("{}, {}", x, y), b = v.iter().map(|(i, j)| i + j).sum()`), [
    `"k"`,
    `a = format!("{}, {}", x, y)`,
    `b = v.iter().map(|(i, j)| i + j).sum()`,
  ]);
  // A comparison is a value, not a name.
  assert.deepEqual(macroCalls(`tr!("a.b", n = x == 1, m = (y == 2))`, "tr")[0].names, ["n", "m"]);
});

test("calls without a literal key are skipped", () => {
  assert.deepEqual(macroCalls(`tr!(key, n = 1); tr!("x.y")`, "tr").map((call) => call.key), ["x.y"]);
});
