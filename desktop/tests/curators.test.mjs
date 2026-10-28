import assert from "node:assert/strict";
import { test } from "node:test";
import { readFileSync } from "node:fs";
import { FABIO_SOURCE, addCurator, checkCurator, hasFabioSource, isSource, packageChange, removeCurator, sameCurators } from "../src/lib/curators.ts";
import { skills } from "../src/i18n/en/skills.ts";
import { skillsPt } from "../src/i18n/pt-BR/skills.ts";

const KEY = "ab".repeat(32);
const ana = { name: "Ana", key: KEY, url: "https://example.org/index.json" };
const ID = "a".repeat(43);

test("an index can be read from https, from arweave, or over http from this machine only", () => {
  for (const ok of ["https://example.org/index.json", `ar://${ID}`, "http://127.0.0.1:8080/i.json", "http://localhost/i.json", "http://[::1]:9/i", "  https://example.org/a  "]) {
    assert.equal(isSource(ok), true, ok);
  }
  for (const bad of ["http://example.org/i.json", "http://127.0.0.1.evil.org/a", "http://localhost@evil.org/a", "http://10.0.0.1/a", "ftp://example.org/a", "file:///etc/passwd", "https://", "https:///a", "ar://short", `ar://${ID}x`, "https://exa mple.org/a", "javascript:alert(1)", ""]) {
    assert.equal(isSource(bad), false, bad);
  }
});

test("an entry is checked the way the hub and this computer check it, one problem at a time", () => {
  assert.equal(checkCurator([], ana), null);
  assert.equal(checkCurator([], { ...ana, name: "  " }), "noName");
  assert.equal(checkCurator([], { ...ana, name: "a\nb" }), "nameLines");
  assert.equal(checkCurator([], { ...ana, name: "x".repeat(81) }), "nameLong");
  assert.equal(checkCurator([], { ...ana, key: "abcd" }), "badKey");
  assert.equal(checkCurator([], { ...ana, url: "http://example.org/i" }), "badUrl");
  assert.equal(checkCurator([ana], { ...ana, name: "Bia" }), "duplicateKey");
  assert.equal(checkCurator([ana], { name: "ANA", key: "cd".repeat(32), url: ana.url }), "duplicateName");
  assert.equal(checkCurator(Array.from({ length: 20 }, (_, i) => ({ name: `c${i}`, key: String(i).padStart(2, "0").repeat(32), url: ana.url })), { name: "last", key: "ff".repeat(32), url: ana.url }), "tooMany");
});

test("adding, removing and comparing keep the key in lowercase and the list as it was when an entry is refused", () => {
  const list = addCurator([], { name: " Ana ", key: KEY.toUpperCase(), url: " https://example.org/index.json " });
  assert.deepEqual(list, [ana]);
  assert.deepEqual(addCurator(list, { ...ana, key: "nope" }), list);
  assert.equal(sameCurators(list, [{ ...ana }]), true);
  assert.equal(sameCurators(list, []), false);
  assert.deepEqual(removeCurator(list, "Ana"), []);
  assert.deepEqual(removeCurator(list, "Nobody"), list);
});

test("every line the sources screen can show exists in both languages", () => {
  const problems = ["noName", "nameLong", "nameLines", "badKey", "badUrl", "duplicateKey", "duplicateName", "tooMany"];
  for (const [name, table] of [["en", skills], ["pt-BR", skillsPt]]) {
    for (const key of ["skills.curatorsTitle", "skills.curatorsHint", "skills.curatorsNone", "skills.curatorOpen", "skills.curatorInstall", "skills.curatorRevoked", "skills.curatorRevokedWhy", ...problems.map((p) => `skills.curatorProblem.${p}`)]) {
      assert.ok(table[key], `${name}: ${key}`);
    }
  }
  assert.match(skills["skills.curatorRevokedWhy"], /\{reason\}/);
  assert.match(skillsPt["skills.curatorEntries"], /\{name\}/);
});

test("the first source is the one community/ is signed with, and adds like any entry", () => {
  const published = readFileSync(new URL("../../community/PUBLIC_KEY", import.meta.url), "utf8").trim();
  assert.equal(FABIO_SOURCE.key, published, "the button and the repository name the same key");
  assert.equal(FABIO_SOURCE.url, "https://raw.githubusercontent.com/Arven-Labs/warden/main/community/index.json");
  assert.equal(checkCurator([], FABIO_SOURCE), null);
  assert.equal(hasFabioSource([]), false);
  const list = addCurator([], FABIO_SOURCE);
  assert.deepEqual(list, [FABIO_SOURCE]);
  assert.equal(hasFabioSource(list), true);
  assert.equal(hasFabioSource([{ ...FABIO_SOURCE, name: "Renamed" }]), true, "found by key, not by name");
  assert.equal(checkCurator([{ ...FABIO_SOURCE, key: "cd".repeat(32) }], FABIO_SOURCE), "duplicateName", "someone else already called Fabio");
  for (const table of [skills, skillsPt]) {
    assert.ok(table["skills.curatorAddFabio"] && table["skills.curatorAddFabioHint"]);
  }
});

const mine = { name: "greet", description: "Say hello.", body: "Line one.\nLine two.\nLine three." };

test("a package for a skill the person does not have shows no change", () => {
  assert.equal(packageChange(undefined, mine), null);
});

test("a package with the same text as the skill the person has shows no change", () => {
  assert.equal(packageChange(mine, { ...mine }), null);
});

test("a package that edits the body shows the lines that go and the lines that come", () => {
  const change = packageChange(mine, { ...mine, body: "Line one.\nLine 2.\nLine three.\nLine four." });
  assert.ok(change);
  assert.equal(change.description, null);
  const ops = change.body.filter((l) => l.op !== "same").map((l) => `${l.op}:${l.text}`);
  assert.deepEqual(ops, ["del:Line two.", "add:Line 2.", "add:Line four."]);
});

test("a package that only rewrites the description says so and has no body lines to show", () => {
  const change = packageChange(mine, { ...mine, description: "Say hello nicely." });
  assert.ok(change);
  assert.deepEqual(change.description, { before: "Say hello.", after: "Say hello nicely." });
  assert.equal(change.body.some((l) => l.op === "add" || l.op === "del"), false);
});

test("the words for a change exist in both languages", () => {
  for (const table of [skills, skillsPt]) {
    for (const key of ["skills.previewChange", "skills.previewDescription", "skills.curatorHaveName"]) {
      assert.ok(table[key], key);
    }
  }
  assert.match(skills["skills.previewDescription"], /\{before\}.*\{after\}/);
  assert.match(skillsPt["skills.previewDescription"], /\{before\}.*\{after\}/);
});
