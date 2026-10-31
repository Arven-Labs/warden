// P72 — the pure half of a skill's attached files in the skills tab (`src/sidepanel/lib/skillFiles.ts`): the checks done before a
// file is sent, which mirror the hub's (the hub is the real barrier) so a bad file fails fast. Run with `npm test`.

import assert from "node:assert/strict";
import { describe, test } from "node:test";
import { MAX_FILES_PER_SKILL, MAX_FILE_BYTES, canAttach, decodeAttachment, validateFileName } from "../src/sidepanel/lib/skillFiles.ts";

const bytes = (text) => new TextEncoder().encode(text);

describe("an attachment's name", () => {
  test("letters, digits, dot, dash and underscore are fine", () => {
    for (const ok of ["notes.md", "a", "My_File-2.txt", "x".repeat(64)]) assert.equal(validateFileName(ok), null, ok);
  });

  test("the problem says which rule it broke", () => {
    assert.equal(validateFileName(""), "length");
    assert.equal(validateFileName("x".repeat(65)), "length");
    assert.equal(validateFileName("a/b"), "chars");
    assert.equal(validateFileName(".hidden"), "dot");
  });

  test("anything that could leave the skill's folder or hide is refused", () => {
    for (const bad of ["../x.md", "a/b.md", "a\\b.md", ".hidden", "..", "", "nul\0.md", "sp ace.md", "é.md", "x".repeat(65)]) {
      assert.notEqual(validateFileName(bad), null, JSON.stringify(bad));
    }
  });
});

describe("an attachment's text", () => {
  test("the limit is in bytes, not characters", () => {
    assert.equal(decodeAttachment(bytes("a".repeat(MAX_FILE_BYTES))).ok, true);
    assert.equal(decodeAttachment(bytes("a".repeat(MAX_FILE_BYTES + 1))).ok, false);
    // 'é' is 2 bytes: 32 769 of them are 32 769 characters but 65 538 bytes.
    assert.equal(decodeAttachment(bytes("é".repeat(MAX_FILE_BYTES / 2 + 1))).ok, false);
    assert.equal(decodeAttachment(bytes("é".repeat(MAX_FILE_BYTES / 2))).ok, true);
  });

  test("returns the decoded text", () => {
    assert.deepEqual(decodeAttachment(bytes("héllo")), { ok: true, text: "héllo" });
  });

  test("bytes that are not valid UTF-8, and a NUL, are not text", () => {
    assert.equal(decodeAttachment(new Uint8Array([0xff, 0xfe, 0x41])).ok, false);
    assert.equal(decodeAttachment(bytes("a\0b")).ok, false);
  });

  test("an empty file is text", () => {
    assert.deepEqual(decodeAttachment(new Uint8Array()), { ok: true, text: "" });
  });
});

describe("how many attachments fit", () => {
  const twenty = Array.from({ length: MAX_FILES_PER_SKILL }, (_, i) => ({ name: `f${i}.md`, size: 1 }));

  test("a new name fits until there are twenty", () => {
    assert.equal(canAttach(twenty.slice(1), "new.md"), true);
    assert.equal(canAttach(twenty, "new.md"), false);
  });

  test("replacing one of the twenty is still fine", () => {
    assert.equal(canAttach(twenty, "f3.md"), true);
  });

  test("a skill with no attachments yet (files absent) takes the first", () => {
    assert.equal(canAttach(undefined, "a.md"), true);
  });
});
