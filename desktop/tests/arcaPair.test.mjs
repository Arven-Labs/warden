// P182 — what the Sync screen draws while a pairing runs: the events of `arca_pair_cmds` (Rust) become one view.
// Run with `npm test`.

import assert from "node:assert/strict";
import { describe, test } from "node:test";
import { emptyView, reduce, fieldIsTyped } from "../src/lib/arcaPair.ts";

describe("what the screen shows", () => {
  test("a code the device computed is shown as text to read out, with no question open", () => {
    const view = reduce(emptyView, { kind: "show", what: "offerCode", value: "7f3a-91c2-0b4d-e8a5" });
    assert.deepEqual(view, { shown: { what: "offerCode", value: "7f3a-91c2-0b4d-e8a5" }, question: null, devices: [] });
  });

  test("a question opens an empty field and clears what was shown, so nothing computed sits next to the field", () => {
    const shown = reduce(emptyView, { kind: "show", what: "offerCode", value: "7f3a-91c2-0b4d-e8a5" });
    const asked = reduce(shown, { kind: "ask", ask: "fingerprintShownByTheOldDevice" });
    assert.deepEqual(asked, { shown: null, question: "fingerprint", devices: [] });
  });

  test("the question for the code on the giving side opens the code field", () => {
    assert.equal(reduce(emptyView, { kind: "ask", ask: "codeShownByTheNewDevice" }).question, "code");
  });

  test("the waiting devices are listed by id for the person to choose", () => {
    const view = reduce(emptyView, { kind: "choose", devices: ["dev-b", "dev-c"] });
    assert.deepEqual(view, { shown: null, question: "choice", devices: ["dev-b", "dev-c"] });
  });

  test("the fingerprint A computes after granting is shown, and closes the question", () => {
    const asked = reduce(emptyView, { kind: "ask", ask: "codeShownByTheNewDevice" });
    const done = reduce(asked, { kind: "show", what: "keyFingerprint", value: "aaaa-bbbb-cccc-dddd" });
    assert.deepEqual(done, { shown: { what: "keyFingerprint", value: "aaaa-bbbb-cccc-dddd" }, question: null, devices: [] });
  });

  test("an event the screen does not know changes nothing", () => {
    const view = reduce(emptyView, { kind: "show", what: "offerCode", value: "x" });
    assert.equal(reduce(view, { kind: "surprise" }), view);
    assert.equal(reduce(view, null), view);
  });
});

describe("what the person may type into a field", () => {
  test("typing is allowed and pasting, dropping and autofill are not (they defeat the check)", () => {
    assert.equal(fieldIsTyped({ inputType: "insertText" }), true);
    assert.equal(fieldIsTyped({ inputType: "deleteContentBackward" }), true);
    assert.equal(fieldIsTyped({ inputType: "insertFromPaste" }), false);
    assert.equal(fieldIsTyped({ inputType: "insertFromDrop" }), false);
    assert.equal(fieldIsTyped({ inputType: "insertReplacementText" }), false);
    assert.equal(fieldIsTyped({ inputType: "insertFromYank" }), false);
  });
});
