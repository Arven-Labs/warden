// P131 — the pure half of the "Chamadas diretas" editor (`src/hub/rawAccess.ts`). Run with `npm test`.

import assert from "node:assert/strict";
import { describe, test } from "node:test";
import { canEditRaw, farIds, hasAll, missingIds, rawOptions, rawSummary, toggleRaw } from "../src/hub/rawAccess.ts";

const providers = [
  { id: "main", kind: "openai" },
  { id: "casa", kind: "node" },
  { id: "fabio", kind: "cloud" },
  { id: "local", kind: "openai_compatible" },
];
const combos = [
  { id: "fast", providers: ["main", "local"] },
  { id: "mixed", providers: ["main", "casa"] },
  { id: "empty", providers: [] },
];

describe("the ids that '*' does not cover", () => {
  test("are a node's model, another hub, and any combo with one of them", () => {
    assert.deepEqual([...farIds(providers, combos)].sort(), ["casa", "fabio", "mixed"]);
  });

  test("are none when everything is local", () => {
    assert.equal(farIds(providers.slice(0, 1), combos.slice(0, 1)).size, 0);
  });
});

describe("the options of the editor", () => {
  test("list providers first, then combos, each marked when it reaches another machine", () => {
    assert.deepEqual(
      rawOptions(providers, combos).map((o) => [o.id, o.kind, o.far]),
      [
        ["main", "provider", false],
        ["casa", "provider", true],
        ["fabio", "provider", true],
        ["local", "provider", false],
        ["fast", "combo", false],
        ["mixed", "combo", true],
        ["empty", "combo", false],
      ],
    );
  });
});

describe("ticking", () => {
  test("adds an id the first time and removes it the second, without touching the rest", () => {
    const once = toggleRaw(["main"], "fast");
    assert.deepEqual(once, ["main", "fast"]);
    assert.deepEqual(toggleRaw(once, "main"), ["fast"]);
  });

  test("never changes the list it was given", () => {
    const raw = ["main"];
    toggleRaw(raw, "fast");
    assert.deepEqual(raw, ["main"]);
  });

  test("treats '*' like any other id and can tell it is there", () => {
    const all = toggleRaw([], "*");
    assert.equal(hasAll(all), true);
    assert.equal(hasAll(toggleRaw(all, "*")), false);
    assert.equal(hasAll(["main"]), false);
  });
});

describe("the summary of a key", () => {
  test("says none for no list, in any shape the hub may send", () => {
    assert.equal(rawSummary(undefined), "None");
    assert.equal(rawSummary([]), "None");
  });

  test("names the ids, or all the local ones, or both", () => {
    assert.equal(rawSummary(["main", "fast"]), "main, fast");
    assert.equal(rawSummary(["*"]), "All local models");
    assert.equal(rawSummary(["*", "casa"]), "All local models and casa");
  });
});

describe("which keys the editor is for", () => {
  const key = (extra) => ({ id: "a", name: "n", shown: "wdn_1", createdAtMs: 1, ...extra });

  test("a general key of the owner, and nothing else", () => {
    assert.equal(canEditRaw(key({}), false), true);
    assert.equal(canEditRaw(key({ agentId: "poet" }), false), false);
    assert.equal(canEditRaw(key({ user: "ana" }), false), false);
    assert.equal(canEditRaw(key({}), true), false, "a member's own screen never offers it");
  });
});

describe("the ids the hub no longer has", () => {
  test("are the listed ones that are neither '*' nor an option", () => {
    const options = rawOptions(providers, combos);
    assert.deepEqual(missingIds(["main", "gone", "*", "fast", "old"], options), ["gone", "old"]);
    assert.deepEqual(missingIds(undefined, options), []);
  });
});
