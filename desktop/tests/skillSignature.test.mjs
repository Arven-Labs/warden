import assert from "node:assert/strict";
import { test } from "node:test";
import { signatureView } from "../src/lib/skillSignature.ts";
import { skills } from "../src/i18n/en/skills.ts";
import { skillsPt } from "../src/i18n/pt-BR/skills.ts";

const KEY = "ab".repeat(32);

test("a key the person trusts is shown by its name, and only the exact word trusted counts", () => {
  assert.deepEqual(signatureView({ signature: "trusted", signer: KEY, signerName: "Fabio" }), { kind: "trusted", name: "Fabio", key: KEY.slice(0, 16), fullKey: KEY });
  // A listed key with no name falls back to its first characters.
  assert.deepEqual(signatureView({ signature: "trusted", signer: KEY, signerName: "" }), { kind: "trusted", name: undefined, key: KEY.slice(0, 16), fullKey: KEY });
});

test("a signature by a key that is not listed is unknown and carries the key, whole for copying", () => {
  assert.deepEqual(signatureView({ signature: "unknown", signer: KEY }), { kind: "unknown", key: KEY.slice(0, 16), fullKey: KEY });
});

test("no signature, an older build, a state from the future or an unknown signer with no key make no claim", () => {
  for (const preview of [
    { signature: "unsigned" },
    {},
    { signature: "verified-by-the-moon", signer: KEY },
    { signature: "unknown" },
    { signature: "TRUSTED", signer: KEY, signerName: "Fabio" },
  ]) {
    assert.deepEqual(signatureView(preview), { kind: "unsigned" }, JSON.stringify(preview));
  }
});

test("every line the signature can show exists in both languages, with its holes", () => {
  for (const [name, table] of [["en", skills], ["pt-BR", skillsPt]]) {
    assert.match(table["skills.signedTrusted"], /\{name\}/, name);
    assert.match(table["skills.signedTrustedNoName"], /\{key\}/, name);
    assert.match(table["skills.signedUnknown"], /\{key\}/, name);
    assert.ok(table["skills.signedNone"], name);
  }
});
