import assert from "node:assert/strict";
import { test } from "node:test";

import { createWriteQueue } from "./writeQueue.ts";

// Deterministic "slow" work: N microtask turns, no wall-clock timers — the
// hazard being tested is ordering, not duration.
async function afterTurns(n: number): Promise<void> {
  for (let i = 0; i < n; i++) await Promise.resolve();
}

test("a save queued in the same tick as an edit runs after it", async () => {
  const order: string[] = [];
  const q = createWriteQueue();
  // Mirrors the Raw pane's ⌘S: `edit` (slow — it awaits applyEdit) and
  // `request-save` posted back to back, with no await in between.
  void q.run(async () => {
    await afterTurns(4);
    order.push("edit");
  });
  await q.run(() => {
    order.push("save");
  });
  assert.deepEqual(order, ["edit", "save"]);
});

test("a failed edit does not wedge the queue", async () => {
  const order: string[] = [];
  const q = createWriteQueue();
  const failed = q.run(async () => {
    await afterTurns(2);
    order.push("edit");
    throw new Error("applyEdit rejected");
  });
  await assert.rejects(failed, /applyEdit rejected/);
  await q.run(() => {
    order.push("save");
  });
  assert.deepEqual(order, ["edit", "save"]);
});

test("queued operations keep their submission order", async () => {
  const order: number[] = [];
  const q = createWriteQueue();
  const runs = [6, 4, 2, 0].map((turns, i) =>
    q.run(async () => {
      await afterTurns(turns);
      order.push(i);
    }),
  );
  await Promise.all(runs);
  assert.deepEqual(order, [0, 1, 2, 3]);
});
