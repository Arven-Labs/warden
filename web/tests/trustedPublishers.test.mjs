import assert from "node:assert/strict";
import { test } from "node:test";
import { MAX_NAME_CHARS, MAX_PUBLISHERS, addPublisher, checkDraft, normalizeKey, removePublisher, samePublishers, shortKey, trustKey } from "../src/hub/trustedPublishers.ts";
import { skills } from "../src/i18n/en/skills.ts";
import { skillsPt } from "../src/i18n/pt-BR/skills.ts";

const KEY = "ab".repeat(32);
const OTHER = "cd".repeat(32);

test("a key is 64 hex characters, compared in lowercase without the spaces around it", () => {
  assert.equal(normalizeKey(`  ${KEY.toUpperCase()} \n`), KEY);
  for (const bad of ["", "abcd", "zz".repeat(32), "ab".repeat(33), `${KEY} ${KEY}`, "0x" + "ab".repeat(31)]) {
    assert.equal(normalizeKey(bad), null, bad);
  }
});

test("a key or name that cannot go in the list says why, in the order the server checks", () => {
  const list = [{ key: KEY, name: "Fabio" }];
  assert.equal(checkDraft(list, OTHER, "Ana"), null);
  assert.equal(checkDraft(list, OTHER, ""), null, "a name is optional");
  assert.equal(checkDraft(list, "nope", "Ana"), "badKey");
  assert.equal(checkDraft(list, KEY.toUpperCase(), "Again"), "duplicate");
  assert.equal(checkDraft(list, OTHER, "two\nlines"), "nameLines");
  assert.equal(checkDraft(list, OTHER, "x".repeat(MAX_NAME_CHARS + 1)), "nameLong");
  assert.equal(checkDraft(list, OTHER, "x".repeat(MAX_NAME_CHARS)), null);
  const full = Array.from({ length: MAX_PUBLISHERS }, (_, n) => ({ key: n.toString(16).padStart(64, "0"), name: "" }));
  assert.equal(checkDraft(full, OTHER, ""), "tooMany");
});

test("adding keeps what was there and tidies the new entry; a refused one changes nothing", () => {
  const list = [{ key: KEY, name: "Fabio" }];
  assert.deepEqual(addPublisher(list, ` ${OTHER.toUpperCase()} `, "  Ana  "), [{ key: KEY, name: "Fabio" }, { key: OTHER, name: "Ana" }]);
  assert.deepEqual(addPublisher(list, "nope", "x"), list);
  assert.notEqual(addPublisher(list, "nope", "x"), list, "a new array either way");
  assert.deepEqual(list, [{ key: KEY, name: "Fabio" }], "the list it was given is not touched");
});

test("trusting the key from the preview gives the list to save, or why there is nothing to save", () => {
  const list = [{ key: KEY, name: "Fabio" }];
  assert.deepEqual(trustKey(list, OTHER, " Ana "), { list: [{ key: KEY, name: "Fabio" }, { key: OTHER, name: "Ana" }] });
  assert.deepEqual(trustKey([], OTHER, ""), { list: [{ key: OTHER, name: "" }] });
  assert.deepEqual(trustKey(list, KEY, "Again"), { problem: "duplicate" });
  assert.deepEqual(trustKey(list, "nope", ""), { problem: "badKey" });
  assert.deepEqual(trustKey(list, OTHER, "x".repeat(MAX_NAME_CHARS + 1)), { problem: "nameLong" });
  const full = Array.from({ length: MAX_PUBLISHERS }, (_, n) => ({ key: n.toString(16).padStart(64, "0"), name: "" }));
  assert.deepEqual(trustKey(full, OTHER, ""), { problem: "tooMany" });
  assert.deepEqual(list, [{ key: KEY, name: "Fabio" }], "the list it was given is not touched");
});

test("removing, comparing and shortening", () => {
  const list = [{ key: KEY, name: "Fabio" }, { key: OTHER, name: "" }];
  assert.deepEqual(removePublisher(list, KEY), [{ key: OTHER, name: "" }]);
  assert.deepEqual(removePublisher(list, "missing"), list);
  assert.equal(samePublishers(list, [...list]), true);
  assert.equal(samePublishers(list, [list[1], list[0]]), false, "order counts");
  assert.equal(samePublishers(list, [{ key: KEY, name: "Other name" }, list[1]]), false);
  assert.equal(shortKey(KEY), KEY.slice(0, 16));
});

test("every line the list screen can show exists in both languages", () => {
  const keys = [
    "skills.trustTitle", "skills.trustHint", "skills.trustNone", "skills.trustKeyLabel", "skills.trustNameLabel", "skills.trustAdd",
    "skills.trustRemove", "skills.trustSave", "skills.trustSaving", "skills.trustSaved", "skills.trustUnsaved", "skills.trustNoName",
    "skills.trustLoadFailed", "skills.trustSaveFailed", "skills.trustKeyPrompt", "skills.copyKey",
    "skills.trustThisKey", "skills.trustThisKeyName", "skills.trustThisKeyConfirm",
    "skills.trustProblem.badKey", "skills.trustProblem.duplicate", "skills.trustProblem.nameLong", "skills.trustProblem.nameLines", "skills.trustProblem.tooMany",
  ];
  for (const [name, table] of [["en", skills], ["pt-BR", skillsPt]]) {
    for (const key of keys) assert.ok(table[key], `${name}: ${key}`);
  }
  assert.match(skills["skills.trustLoadFailed"], /\{error\}/);
  assert.match(skillsPt["skills.trustSaveFailed"], /\{error\}/);
});
