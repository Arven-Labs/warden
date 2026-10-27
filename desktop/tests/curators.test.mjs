import assert from "node:assert/strict";
import { test } from "node:test";
import { addCurator, checkCurator, isSource, removeCurator, sameCurators } from "../src/lib/curators.ts";
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
