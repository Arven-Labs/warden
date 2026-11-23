// P-49 — what the rotation screen says when a RESUME (the person typed the 24 words of a key) is refused. A resume may follow an
// attempt that already published the new manifest, so the text must not say "nothing was changed" or "the key was discarded": it
// says that nothing was written by THIS attempt, and that the 24 words stay valid. Run with `npm test`.

import assert from "node:assert/strict";
import { describe, test } from "node:test";
import { en } from "../src/i18n/en.ts";
import { ptBR } from "../src/i18n/pt-BR.ts";

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
});
