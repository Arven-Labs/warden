// P126 — the stamp the window saves with an answer (`src/lib/answeredBy.ts`). Run with `npm test`.

import assert from "node:assert/strict";
import { describe, test } from "node:test";
import { answeredByOf } from "../src/lib/answeredBy.ts";

const slice = (providerId, n, agentId) => ({ ...(agentId ? { agentId } : {}), providerId, usage: { promptTokens: n, completionTokens: 0, totalTokens: n } });

describe("the answeredBy stamp of a reply", () => {
  test("a turn that says nothing leaves the stamp to the conversation", () => {
    assert.deepEqual(answeredByOf({}), {});
    assert.deepEqual(answeredByOf({ usageSlices: [slice("main", 5)] }), {});
  });

  test("the provider of the last call is the stamp, as before", () => {
    assert.deepEqual(answeredByOf({ providerId: "main" }), { answeredBy: { providerId: "main" } });
    assert.deepEqual(answeredByOf({ providerId: "main", usageSlices: [slice("main", 5)] }), { answeredBy: { providerId: "main" } }, "one slice is what the stamp already says");
  });

  test("a turn through a combo is stamped with it, with or without the provider", () => {
    assert.deepEqual(answeredByOf({ providerId: "spare", comboId: "fast" }), { answeredBy: { providerId: "spare", comboId: "fast" } });
    assert.deepEqual(answeredByOf({ comboId: "fast" }), { answeredBy: { comboId: "fast" } });
    assert.deepEqual(answeredByOf({ providerId: "main" }), { answeredBy: { providerId: "main" } }, "a provider picked by itself has no combo");
    const slices = [{ ...slice("main", 5, "a"), comboId: "fast" }, slice("cheap", 11, "helper")];
    assert.deepEqual(answeredByOf({ providerId: "main", comboId: "fast", usageSlices: slices }), { answeredBy: { providerId: "main", comboId: "fast", slices } });
  });

  test("a turn two providers answered carries its usage told per provider", () => {
    const slices = [slice("main", 5, "a"), slice("spare", 11, "a")];
    assert.deepEqual(answeredByOf({ providerId: "spare", usageSlices: slices }), { answeredBy: { providerId: "spare", slices } });
    assert.deepEqual(answeredByOf({ usageSlices: slices }), { answeredBy: { slices } }, "even for a model that carries no provider id");
  });
});
