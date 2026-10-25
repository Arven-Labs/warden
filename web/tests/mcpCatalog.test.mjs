// P140 — the gallery of ready-made MCP integrations in the hub's machine settings: what the four local entries hold, which ones the draft
// already has, and the fields a new one gets. Run with `npm test`.

import assert from "node:assert/strict";
import { afterEach, describe, test } from "node:test";
import { MCP_CATALOG, MCP_NEEDS_TEXT, catalogServer, catalogState } from "../src/components/mcpCatalog.ts";
import { setLocale, t } from "../src/i18n/index.ts";

afterEach(() => setLocale("en"));

const entry = (id) => MCP_CATALOG.find((e) => e.id === id);
const draftOf = (e, name = e.id) => ({ name, kind: "stdio", args: e.args });

describe("the entries", () => {
  test("ids are unique and each one has a command and a token among its arguments", () => {
    assert.equal(new Set(MCP_CATALOG.map((e) => e.id)).size, MCP_CATALOG.length);
    for (const e of MCP_CATALOG) {
      assert.notEqual(e.command.trim(), "", e.id);
      assert.ok(e.args.split("\n").includes(e.token), e.id);
    }
  });

  test("only local ones are here: the page never sends OAuth, so a hosted one that signs in would be saved broken", () => {
    assert.ok(!MCP_CATALOG.some((e) => e.id === "slack"));
    assert.ok(MCP_CATALOG.every((e) => ["npx", "docker"].includes(e.command)));
  });

  test("the secrets ship as names only, and a variable name is never repeated", () => {
    for (const e of MCP_CATALOG) {
      assert.equal(new Set(e.envNames).size, e.envNames.length, e.id);
      assert.ok(e.envNames.every((name) => /^[A-Z][A-Z0-9_]*$/.test(name)), e.id);
      assert.equal(e.needs === "folder", e.envNames.length === 0, `${e.id}: a folder needs no secret, the others do`);
    }
  });

  test("each one says what it asks for, in both languages", () => {
    for (const locale of ["en", "pt-BR"]) {
      setLocale(locale);
      for (const e of MCP_CATALOG) {
        for (const key of [e.label, e.description, MCP_NEEDS_TEXT[e.needs]]) {
          const text = t(key);
          assert.notEqual(text, key, `${e.id} in ${locale}`);
          assert.notEqual(text.trim(), "", `${e.id} in ${locale}`);
        }
      }
    }
  });
});

describe("which ones the draft has", () => {
  test("an empty draft has none", () => {
    assert.ok(MCP_CATALOG.every((e) => catalogState(e, []) === "available"));
  });

  test("a server with the entry's package counts, and so does one the person renamed", () => {
    const notion = entry("notion");
    assert.equal(catalogState(notion, [draftOf(notion)]), "added");
    assert.equal(catalogState(notion, [draftOf(notion, "my-notes")]), "added");
    assert.equal(catalogState(entry("github"), [draftOf(notion)]), "available");
  });

  test("an http server, or a custom one that merely has the same name, is not the entry", () => {
    const notion = entry("notion");
    assert.equal(catalogState(notion, [{ name: "notion", kind: "http", args: notion.args }]), "available");
    assert.equal(catalogState(notion, [{ name: "notion", kind: "stdio", args: "-y\nmy-notion" }]), "available");
  });

  test("the token is read as a whole line, not as part of another", () => {
    const fs = entry("filesystem");
    assert.equal(catalogState(fs, [{ name: "x", kind: "stdio", args: ` ${fs.token} \n/tmp` }]), "added");
    assert.equal(catalogState(fs, [{ name: "x", kind: "stdio", args: `${fs.token}-fork` }]), "available");
  });
});

describe("what adding gives", () => {
  test("a copy of the fields: changing it never changes the catalog", () => {
    const notion = entry("notion");
    const added = catalogServer(notion, []);
    added.envNames.push("EXTRA");
    assert.deepEqual(notion.envNames, ["NOTION_TOKEN"]);
    assert.deepEqual({ command: added.command, args: added.args }, { command: "npx", args: notion.args });
  });

  test("the name never collides with one already in the list, nor with one with stray spaces", () => {
    const notion = entry("notion");
    assert.equal(catalogServer(notion, []).name, "notion");
    assert.equal(catalogServer(notion, [{ name: "notion" }]).name, "notion-2");
    assert.equal(catalogServer(notion, [{ name: "notion" }, { name: " notion-2 " }]).name, "notion-3");
    assert.equal(catalogServer(notion, [{ name: "notion-2" }]).name, "notion");
  });
});
