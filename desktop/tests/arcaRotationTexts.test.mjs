// P-49 — what the rotation screen says when a rotation call fails. A RESUME (the person typed the 24 words of a key) may follow an
// attempt that already published the new manifest, so its refusals must not say "nothing was changed" or "the key was discarded": they
// say that nothing was written by THIS attempt, and that the 24 words stay valid. But the same screen also gets the errors of the
// phrase itself (not 24 valid words), which say nothing about an earlier attempt, and the "newer epoch" that proves the phrase is NOT
// the published one. Run with `npm test`.

import assert from "node:assert/strict";
import { describe, test } from "node:test";
import { en } from "../src/i18n/en.ts";
import { ptBR } from "../src/i18n/pt-BR.ts";
import { failureOf, failureView } from "../src/lib/arcaRotation.ts";

const refused = { partial: false, message: "this attempt wrote nothing: x", newerEpoch: false };
const newer = { partial: false, message: "a newer key epoch is already stored", newerEpoch: true };
const stopped = { partial: true, message: "the key was not rotated: disk full", newerEpoch: false };

describe("a resume that was refused", () => {
  test("says that nothing was written by this attempt and keeps the 24 words valid, in English", () => {
    const text = en["arca.rotationResumeNotWritten"];
    assert.ok(text, "the key exists");
    assert.match(text, /this attempt/);
    assert.match(text, /earlier attempt/);
    assert.match(text, /24 words/);
    assert.match(text, /\{message\}/);
    assert.doesNotMatch(text, /discarded/);
  });

  test("says it in Portuguese too", () => {
    const text = ptBR["arca.rotationResumeNotWritten"];
    assert.ok(text, "the key exists");
    assert.match(text, /nesta tentativa/);
    assert.match(text, /tentativa anterior/);
    assert.match(text, /24 palavras/);
    assert.match(text, /\{message\}/);
    assert.doesNotMatch(text, /descartada/);
  });

  test("a newer epoch on a resume says the phrase is NOT the one of the change that stopped, never that an earlier attempt published it", () => {
    const text = en["arca.rotationResumeNewerEpoch"];
    assert.ok(text, "the key exists");
    assert.match(text, /not those of the change that stopped/);
    assert.match(text, /24 words/);
    assert.match(text, /\{message\}/);
    assert.doesNotMatch(text, /earlier attempt/);
    const textPt = ptBR["arca.rotationResumeNewerEpoch"];
    assert.ok(textPt, "the key exists");
    assert.match(textPt, /não são as da troca que parou/);
    assert.match(textPt, /24 palavras/);
    assert.doesNotMatch(textPt, /tentativa anterior/);
  });
});

describe("failureView: which text and which step after a failure", () => {
  test("a stop that may have written something goes to the resume with the same 24 words, wherever it came from", () => {
    for (const step of ["shown", "resume"]) {
      assert.deepEqual(failureView(step, "confirm", stopped), { next: "resume", key: "arca.rotationPartial" }, step);
    }
  });

  test("a refusal of a key that was just made drops it and starts again, with the plain text", () => {
    assert.deepEqual(failureView("shown", "confirm", refused), { next: "reset", key: "arca.rotationFailed" });
    assert.deepEqual(failureView("shown", "confirm", newer), { next: "reset", key: "arca.rotationFailed" });
  });

  test("a refusal of the confirm of a resume keeps the words and talks about THIS attempt", () => {
    assert.deepEqual(failureView("resume", "confirm", refused), { next: "keep", key: "arca.rotationResumeNotWritten" });
  });

  test("a newer epoch on a resume has its own text", () => {
    assert.deepEqual(failureView("resume", "confirm", newer), { next: "keep", key: "arca.rotationResumeNewerEpoch" });
  });

  test("the errors of the phrase itself (not 24 valid words, not in arca mode) are not about an earlier attempt", () => {
    // `arca_rotation_resume` rejects with plain strings: nothing was published by anyone, so the neutral text is the right one.
    const phrase = failureOf("that is not a valid 24-word phrase");
    assert.deepEqual(failureView("resume", "resume", phrase), { next: "keep", key: "arca.rotationFailed" });
    assert.deepEqual(failureView("resume", "resume", refused), { next: "keep", key: "arca.rotationFailed" });
  });

  test("every text it can point to exists in both languages with the {message} it fills", () => {
    const keys = new Set();
    for (const step of ["shown", "resume"]) {
      for (const phase of ["resume", "confirm"]) {
        for (const failure of [refused, newer, stopped]) keys.add(failureView(step, phase, failure).key);
      }
    }
    for (const key of keys) {
      assert.ok(en[key] && ptBR[key], key);
      assert.match(en[key], /\{message\}/, key);
      assert.match(ptBR[key], /\{message\}/, key);
    }
  });
});

describe("failureOf", () => {
  test("reads what arca_rotation_confirm rejects with", () => {
    assert.deepEqual(failureOf({ partial: true, message: "m", newerEpoch: false }), { partial: true, message: "m", newerEpoch: false });
    assert.deepEqual(failureOf({ partial: false, message: "m", newerEpoch: true }), { partial: false, message: "m", newerEpoch: true });
  });

  test("a plain string (the other commands) is nothing written, and nothing special", () => {
    assert.deepEqual(failureOf("no rotation was started"), { partial: false, message: "no rotation was started", newerEpoch: false });
  });

  test("anything that is not literally true is false, so a malformed answer never opens the resume", () => {
    assert.deepEqual(failureOf({ partial: "true", message: "m", newerEpoch: 1 }), { partial: false, message: "m", newerEpoch: false });
  });
});
