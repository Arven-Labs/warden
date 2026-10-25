// P140 — the gallery of ready-made MCP integrations in Settings: what the five entries hold, which ones the draft already has, and the
// server that adding one puts in it. Run with `npm test`.

import assert from "node:assert/strict";
import { afterEach, describe, test } from "node:test";
import { setLocale, t } from "../src/i18n/index.ts";
import { MCP_CATALOG, MCP_NEEDS_TEXT, catalogServer, catalogState } from "../src/lib/mcpCatalog.ts";

afterEach(() => setLocale("en"));

const entry = (id) => MCP_CATALOG.find((e) => e.id === id);

describe("the entries", () => {
  test("ids and server names are unique and each entry has a name to give its server", () => {
    assert.equal(new Set(MCP_CATALOG.map((e) => e.id)).size, MCP_CATALOG.length);
    assert.equal(new Set(MCP_CATALOG.map((e) => e.server.name)).size, MCP_CATALOG.length);
    assert.ok(MCP_CATALOG.every((e) => e.server.name.trim() !== ""));
  });

  test("a hosted entry is told apart by its address and a local one by a package or image in its arguments", () => {
    for (const e of MCP_CATALOG) {
      if ("url" in e.server) assert.equal(e.server.url, e.token, e.id);
      else assert.ok(e.server.args.includes(e.token), e.id);
    }
  });

  test("a hosted entry is https and takes exactly one way in: the browser sign-in, or a key in the Authorization header left blank", () => {
    for (const e of MCP_CATALOG) {
      if ("url" in e.server) {
        assert.ok(e.server.url.startsWith("https://"), e.id);
        if (e.server.oauth) {
          assert.deepEqual(e.server.headers, {}, e.id);
          assert.equal(e.needs, "oauth", e.id);
        } else {
          assert.deepEqual(e.server.headers, { Authorization: "" }, `${e.id} ships no key`);
          assert.equal(e.needs, "header", e.id);
        }
      } else {
        assert.ok(Object.values(e.server.env).every((v) => v === ""), `${e.id} leaves its secrets blank`);
      }
    }
  });

  test("the hosted ones checked in Session 304 are there, at the addresses the vendors' docs give", () => {
    const urls = Object.fromEntries(MCP_CATALOG.filter((e) => "url" in e.server).map((e) => [e.id, e.server.url]));
    assert.equal(urls.linear, "https://mcp.linear.app/mcp");
    assert.equal(urls.github_remote, "https://api.githubcopilot.com/mcp/");
    assert.equal(urls.notion_remote, "https://mcp.notion.com/mcp");
    assert.ok(!MCP_CATALOG.some((e) => e.id === "sentry"), "Sentry's own pages disagree on how it signs in, so it waits");
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

  test("a server added from the entry counts, and so does one the person renamed", () => {
    const notion = entry("notion");
    const added = catalogServer(notion, []);
    assert.equal(catalogState(notion, [added]), "added");
    assert.equal(catalogState(notion, [{ ...added, name: "my-notes" }]), "added");
    assert.equal(catalogState(entry("github"), [added]), "available");
  });

  test("a hosted one is told apart by its address", () => {
    const slack = entry("slack");
    assert.equal(catalogState(slack, [catalogServer(slack, [])]), "added");
    assert.equal(catalogState(slack, [{ name: "slack", url: "https://example.com/mcp", headers: {}, oauth: false }]), "available");
  });

  test("a custom server that merely has the same name is not the entry", () => {
    assert.equal(catalogState(entry("notion"), [{ name: "notion", command: "my-notion", args: [], env: {} }]), "available");
  });
});

describe("what adding puts in the draft", () => {
  test("a copy: editing the new server never changes the catalog", () => {
    const github = entry("github");
    const added = catalogServer(github, []);
    added.args.push("--extra");
    added.env.GITHUB_PERSONAL_ACCESS_TOKEN = "secret";
    assert.ok(!github.server.args.includes("--extra"));
    assert.equal(github.server.env.GITHUB_PERSONAL_ACCESS_TOKEN, "");

    const slack = entry("slack");
    const hosted = catalogServer(slack, []);
    hosted.headers.Authorization = "Bearer x";
    assert.deepEqual(slack.server.headers, {});

    // A hosted one that takes a key: filling in the copy's header leaves the catalog's blank.
    const linear = entry("linear");
    const keyed = catalogServer(linear, []);
    keyed.headers.Authorization = "Bearer secret";
    assert.deepEqual(linear.server.headers, { Authorization: "" });
    assert.equal(catalogState(linear, [keyed]), "added");
  });

  test("the name never collides with one already in the list", () => {
    const notion = entry("notion");
    const one = catalogServer(notion, []);
    assert.equal(one.name, "notion");
    const two = catalogServer(notion, [one]);
    assert.equal(two.name, "notion-2");
    const three = catalogServer(notion, [one, two]);
    assert.equal(three.name, "notion-3");
    assert.equal(catalogServer(notion, [{ ...one, name: "notion-2" }]).name, "notion");
  });
});
