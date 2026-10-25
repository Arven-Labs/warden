import assert from "node:assert/strict";
import { test } from "node:test";
import { rememberKeyPath, rememberedKeyPath, shortSigner } from "../src/lib/signingKey.ts";
import { skills } from "../src/i18n/en/skills.ts";
import { skillsPt } from "../src/i18n/pt-BR/skills.ts";

/** A stand-in for `localStorage`. */
function storage(initial = {}) {
  const data = { ...initial };
  return {
    data,
    getItem: (key) => (key in data ? data[key] : null),
    setItem: (key, value) => {
      data[key] = value;
    },
    removeItem: (key) => {
      delete data[key];
    },
  };
}

test("the key file chosen is remembered by path and forgotten on request", () => {
  const store = storage();
  assert.equal(rememberedKeyPath(store), null);
  rememberKeyPath("  /home/ana/warden-signing.key ", store);
  assert.equal(rememberedKeyPath(store), "/home/ana/warden-signing.key");
  assert.deepEqual(Object.values(store.data), ["/home/ana/warden-signing.key"], "only the path is written");
  rememberKeyPath(null, store);
  assert.equal(rememberedKeyPath(store), null);
  rememberKeyPath("   ", store);
  assert.equal(rememberedKeyPath(store), null, "a blank path is not a path");
});

test("storage that is missing or throws never breaks the screen", () => {
  const broken = {
    getItem: () => {
      throw new Error("blocked");
    },
    setItem: () => {
      throw new Error("blocked");
    },
    removeItem: () => {
      throw new Error("blocked");
    },
  };
  assert.equal(rememberedKeyPath(broken), null);
  assert.doesNotThrow(() => rememberKeyPath("/a/key", broken));
  assert.doesNotThrow(() => rememberKeyPath(null, broken));
  assert.equal(rememberedKeyPath(null), null);
  assert.doesNotThrow(() => rememberKeyPath("/a/key", null));
});

test("a signer is shown by its first 16 characters", () => {
  assert.equal(shortSigner("ab".repeat(32)), "ab".repeat(8));
});

test("every line the signing form can show exists in both languages", () => {
  const keys = [
    "skills.signWith", "skills.signHint", "skills.signChooseKey", "skills.signCreateKey", "skills.signAs", "skills.signNeedsKey",
    "skills.signFailed", "skills.signKeyMade", "skills.signCopyPublic", "skills.signedDone",
  ];
  for (const [name, table] of [["en", skills], ["pt-BR", skillsPt]]) {
    for (const key of keys) assert.ok(table[key], `${name}: ${key}`);
  }
  assert.match(skills["skills.signAs"], /\{key\}/);
  assert.match(skillsPt["skills.signFailed"], /\{error\}/);
});
