import assert from "node:assert/strict";
import { test } from "node:test";
import { aggregateGpu } from "../src/lib/gpu-usage.ts";

const engine = (id: string, util: number, name = id) => ({ id, util, name });
const row = (...gpuEngines: ReturnType<typeof engine>[]) => ({
  gpu: Math.max(0, ...gpuEngines.map((e) => e.util)),
  gpuEngines,
});

test("independent engines and adapters are not added into a false 75% group", () => {
  const result = aggregateGpu([row(engine("gpu0:0:0", 40)), row(engine("gpu1:0:6", 35))]);
  assert.equal(result.gpu, 40);
  assert.equal(result.gpuEngine, "gpu0:0:0");
});

test("contributions on the same physical node add and saturate at 100%", () => {
  assert.equal(aggregateGpu([row(engine("gpu0:0:0", 40)), row(engine("gpu0:0:0", 35))]).gpu, 75);
  assert.equal(aggregateGpu([row(engine("gpu0:0:0", 80)), row(engine("gpu0:0:0", 35))]).gpu, 100);
});

test("a process's secondary engine can become the busiest engine of its group", () => {
  const result = aggregateGpu([row(engine("3d", 80), engine("copy", 70)), row(engine("copy", 40))]);
  assert.equal(result.gpu, 100);
  assert.equal(result.gpuEngine, "copy");
});

test("identical engine names do not merge different nodes or physical GPUs", () => {
  const result = aggregateGpu([
    row(engine("a:0:1", 40, "Copy")),
    row(engine("a:0:2", 35, "Copy")),
    row(engine("a:1:1", 20, "Copy")),
  ]);
  assert.equal(result.gpu, 40);
  assert.equal(result.gpuEngines.length, 3);
});

test("section aggregation preserves the same result with folded application groups", () => {
  const first = row(engine("3d", 20), engine("copy", 30));
  const second = row(engine("3d", 30));
  const third = row(engine("copy", 40));
  const folded = aggregateGpu([aggregateGpu([first, second]), third]);
  assert.equal(folded.gpu, aggregateGpu([first, second, third]).gpu);
  assert.equal(folded.gpu, 70);
});

test("unknown and invalid engine readings do not create invented totals", () => {
  assert.equal(aggregateGpu([{ gpu: 40 }, { gpu: 35 }]).gpu, 40);
  assert.equal(
    aggregateGpu([row(engine("a", Number.NaN)), row(engine("a", Number.POSITIVE_INFINITY)), { gpu: -5 }]).gpu,
    0,
  );
});
