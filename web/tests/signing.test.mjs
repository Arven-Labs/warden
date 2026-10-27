import assert from "node:assert/strict";
import { createHash, createPublicKey, generateKeyPairSync, verify } from "node:crypto";
import { test } from "node:test";
import { SigningError, createKeyFile, publicKeyOf, readKeyFile, signPackageText } from "../src/hub/signing.ts";
import { skills } from "../src/i18n/en/skills.ts";
import { skillsPt } from "../src/i18n/pt-BR/skills.ts";

/** A key file as `warden-server package keygen` writes it: the PKCS#8 v2 that `ring` makes, in hex. Built here from a Node key. */
function keyFile() {
  const { privateKey, publicKey } = generateKeyPairSync("ed25519");
  const seed = privateKey.export({ format: "der", type: "pkcs8" }).subarray(16);
  const raw = publicKey.export({ format: "der", type: "spki" }).subarray(12);
  const text = "3053020101300506032b657004220420" + seed.toString("hex") + "a123032100" + raw.toString("hex");
  return { text: text + "\n", publicKey, publicHex: raw.toString("hex") };
}

/** A package as the hub exports it, hashed the way `warden-core` does (the fields in a fixed order, compact JSON). */
function exported() {
  const content = { name: "greet", description: "Greets.", body: "Say hello." };
  const hashed = JSON.stringify({ kind: "skill", id: "greet", version: "1.0.0", author: "Ana", license: "MIT", content });
  const hash = "sha256:" + createHash("sha256").update(hashed).digest("hex");
  return JSON.stringify({ format: "warden-package", formatVersion: 1, kind: "skill", id: "greet", version: "1.0.0", author: "Ana", license: "MIT", content, hash }, null, 2);
}

test("a key file gives its public key, and anything else is not a key", () => {
  const key = keyFile();
  assert.equal(publicKeyOf(key.text), key.publicHex);
  assert.equal(readKeyFile(key.text)?.seed.length, 32);
  for (const bad of ["", "hello", "ab".repeat(85), key.text.slice(0, -10), key.text.replace("a123032100", "a123032101"), "30".repeat(5000)]) {
    assert.equal(readKeyFile(bad), null, bad.slice(0, 20));
  }
  assert.throws(() => publicKeyOf("hello"), SigningError);
});

test("a package is signed over its hash, in the form the server reads", async () => {
  const key = keyFile();
  const content = exported();
  const signed = await signPackageText(content, key.text);
  assert.equal(signed.signer, key.publicHex);
  const envelope = JSON.parse(signed.content);
  const match = /^ed25519:([0-9a-f]{64}):([0-9a-f]{128})$/.exec(envelope.signature);
  assert.ok(match, envelope.signature);
  assert.equal(match[1], key.publicHex);
  assert.equal(verify(null, Buffer.from(envelope.hash), key.publicKey, Buffer.from(match[2], "hex")), true, "the signature is over the hash");
  // Everything but the signature is as the hub exported it.
  const { signature, ...rest } = envelope;
  assert.deepEqual(rest, JSON.parse(content));
  // Signing a signed package replaces the signature.
  assert.equal(JSON.parse((await signPackageText(signed.content, key.text)).content).signature, signature);
});

test("what is not a package, or is signed with what is not a key, is refused before anything is signed", async () => {
  const key = keyFile();
  await assert.rejects(signPackageText("not json", key.text), /not a Warden package/);
  await assert.rejects(signPackageText(JSON.stringify({ format: "other", hash: "sha256:00" }), key.text), /not a Warden package/);
  await assert.rejects(signPackageText(JSON.stringify({ format: "warden-package" }), key.text), /not a Warden package/);
  await assert.rejects(signPackageText(exported(), "hello"), /not a Warden signing key/);
});

test("a key file whose public half is not the seed's does not sign", async () => {
  const key = keyFile();
  const other = keyFile();
  const mixed = key.text.trim().slice(0, -64) + other.publicHex;
  await assert.rejects(signPackageText(exported(), mixed), SigningError);
});

test("a key made here is a key file the reader, the signer and the server's format all accept", async () => {
  const made = await createKeyFile();
  assert.match(made.publicKey, /^[0-9a-f]{64}$/);
  assert.match(made.text, /^[0-9a-f]{170}\n$/, "85 bytes in hex on one line, as `ring` writes it");
  assert.equal(publicKeyOf(made.text), made.publicKey);
  const signed = await signPackageText(exported(), made.text);
  assert.equal(signed.signer, made.publicKey);
  const match = /^ed25519:([0-9a-f]{64}):([0-9a-f]{128})$/.exec(JSON.parse(signed.content).signature);
  assert.ok(match);
  const raw = Buffer.from("302a300506032b6570032100" + made.publicKey, "hex");
  assert.equal(verify(null, Buffer.from(JSON.parse(signed.content).hash), createPublicKey({ key: raw, format: "der", type: "spki" }), Buffer.from(match[2], "hex")), true);
  assert.notEqual((await createKeyFile()).publicKey, made.publicKey, "each call makes a different key");
});

test("every line the signing form can show exists in both languages", () => {
  const keys = ["skills.signWith", "skills.signKeyFile", "skills.signAs", "skills.signNeedsKey", "skills.signFailed", "skills.signNotAccepted", "skills.signedDone", "skills.signCreate", "skills.signCreated", "skills.signCreatedTrusted"];
  for (const [name, table] of [["en", skills], ["pt-BR", skillsPt]]) {
    for (const key of keys) assert.ok(table[key], `${name}: ${key}`);
  }
  assert.match(skills["skills.signAs"], /\{key\}/);
  assert.match(skillsPt["skills.signFailed"], /\{error\}/);
});
