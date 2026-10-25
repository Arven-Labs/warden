// P140 — the gallery of ready-made MCP integrations in the hub's machine settings: what the local and the hosted entries hold, which ones
// the draft already has, and the fields a new one gets. Run with `npm test`.

import assert from "node:assert/strict";
import { afterEach, describe, test } from "node:test";
import { MCP_CATALOG, MCP_NEEDS_TEXT, catalogServer, catalogState } from "../src/components/mcpCatalog.ts";
import { setLocale, t } from "../src/i18n/index.ts";

afterEach(() => setLocale("en"));

const entry = (id) => MCP_CATALOG.find((e) => e.id === id);
const stdio = (e, name = e.id) => ({ name, kind: "stdio", args: e.args, url: "" });
const http = (e, name = e.id) => ({ name, kind: "http", args: "", url: e.url });

describe("the entries", () => {
  test("ids are unique, a local one has a command with its token among the arguments, a hosted one is https at its token", () => {
    assert.equal(new Set(MCP_CATALOG.map((e) => e.id)).size, MCP_CATALOG.length);
    for (const e of MCP_CATALOG) {
      if (e.kind === "stdio") {
        assert.notEqual(e.command.trim(), "", e.id);
        assert.ok(e.args.split("\n").includes(e.token), e.id);
      } else {
        assert.ok(e.url.startsWith("https://"), e.id);
        assert.equal(e.url, e.token, e.id);
      }
    }
  });

  test("nothing that signs in with OAuth is here: the page never sends it, so such a server would be saved broken", () => {
    assert.ok(!MCP_CATALOG.some((e) => ["slack", "notion_remote"].includes(e.id)));
    assert.ok(MCP_CATALOG.filter((e) => e.kind === "stdio").every((e) => ["npx", "docker"].includes(e.command)));
    // Every hosted one gets in through a key in the Authorization header, so it has a header to fill in.
    for (const e of MCP_CATALOG.filter((x) => x.kind === "http")) {
      assert.deepEqual(e.headerNames, ["Authorization"], e.id);
      assert.equal(e.needs, "header", e.id);
    }
  });

  test("the hosted ones checked in Session 304 are at the addresses the vendors' docs give, and Sentry waits", () => {
    assert.equal(entry("linear").url, "https://mcp.linear.app/mcp");
    assert.equal(entry("github_remote").url, "https://api.githubcopilot.com/mcp/");
    assert.ok(!MCP_CATALOG.some((e) => e.id === "sentry"), "Sentry's own pages disagree on how it signs in");
  });

  test("the secrets ship as names only, and a variable name is never repeated", () => {
    for (const e of MCP_CATALOG.filter((x) => x.kind === "stdio")) {
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

  test("a local server with the entry's package counts, and so does one the person renamed", () => {
    const notion = entry("notion");
    assert.equal(catalogState(notion, [stdio(notion)]), "added");
    assert.equal(catalogState(notion, [stdio(notion, "my-notes")]), "added");
    assert.equal(catalogState(entry("github"), [stdio(notion)]), "available");
  });

  test("a hosted server counts by its address, renamed or not, and the local GitHub is not the hosted one", () => {
    const linear = entry("linear");
    assert.equal(catalogState(linear, [http(linear)]), "added");
    assert.equal(catalogState(linear, [http(linear, "work")]), "added");
    assert.equal(catalogState(linear, [{ name: "linear", kind: "http", args: "", url: "https://example.com/mcp" }]), "available");
    assert.equal(catalogState(entry("github_remote"), [stdio(entry("github"))]), "available");
    assert.equal(catalogState(entry("github"), [http(entry("github_remote"))]), "available");
  });

  test("the wrong kind, or a custom one that merely has the same name, is not the entry", () => {
    const notion = entry("notion");
    assert.equal(catalogState(notion, [{ name: "notion", kind: "http", args: notion.args, url: "" }]), "available");
    assert.equal(catalogState(notion, [{ name: "notion", kind: "stdio", args: "-y\nmy-notion", url: "" }]), "available");
    const linear = entry("linear");
    assert.equal(catalogState(linear, [{ name: "linear", kind: "stdio", args: linear.url, url: "" }]), "available");
  });

  test("the token is read as a whole line, not as part of another", () => {
    const fs = entry("filesystem");
    assert.equal(catalogState(fs, [{ name: "x", kind: "stdio", args: ` ${fs.token} \n/tmp`, url: "" }]), "added");
    assert.equal(catalogState(fs, [{ name: "x", kind: "stdio", args: `${fs.token}-fork`, url: "" }]), "available");
  });
});

describe("what adding gives", () => {
  test("a copy of the fields: changing it never changes the catalog", () => {
    const notion = entry("notion");
    const added = catalogServer(notion, []);
    assert.equal(added.kind, "stdio");
    added.envNames.push("EXTRA");
    assert.deepEqual(notion.envNames, ["NOTION_TOKEN"]);
    assert.deepEqual({ command: added.command, args: added.args }, { command: "npx", args: notion.args });

    const linear = entry("linear");
    const hosted = catalogServer(linear, []);
    assert.deepEqual({ kind: hosted.kind, url: hosted.url, headerNames: hosted.headerNames }, { kind: "http", url: linear.url, headerNames: ["Authorization"] });
    hosted.headerNames.push("X-Extra");
    assert.deepEqual(linear.headerNames, ["Authorization"]);
  });

  test("the name never collides with one already in the list, nor with one with stray spaces", () => {
    const notion = entry("notion");
    assert.equal(catalogServer(notion, []).name, "notion");
    assert.equal(catalogServer(notion, [{ name: "notion" }]).name, "notion-2");
    assert.equal(catalogServer(notion, [{ name: "notion" }, { name: " notion-2 " }]).name, "notion-3");
    assert.equal(catalogServer(notion, [{ name: "notion-2" }]).name, "notion");
    assert.equal(catalogServer(entry("linear"), [{ name: "linear" }]).name, "linear-2");
  });
});
