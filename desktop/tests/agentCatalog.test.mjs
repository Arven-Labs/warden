// P140 — the gallery of ready-made agents in the organization view: what a row offers, the edit it sends, and the Portuguese texts (every agent
// the program ships has one, and anything the table does not know keeps the original text). Run with `npm test`.

import assert from "node:assert/strict";
import { readdirSync } from "node:fs";
import { afterEach, describe, test } from "node:test";
import { PT_BR_AGENTS, catalogAgentText } from "../src/i18n/agentCatalog.ts";
import { setLocale } from "../src/i18n/index.ts";
import { catalogAgentAction, installAgentEdit } from "../src/lib/org.ts";

afterEach(() => setLocale("en"));

const CATALOG = new URL("../../crates/warden-bootstrap/catalog/agents/", import.meta.url);

describe("what a gallery row offers", () => {
  test("an added agent offers nothing and a missing one offers to be added", () => {
    assert.equal(catalogAgentAction("installed", false), "installed");
    assert.equal(catalogAgentAction("notInstalled", false), "install");
  });

  test("an agent of the same id that differs says so first, and only a second tap replaces its text", () => {
    assert.equal(catalogAgentAction("modified", false), "conflict");
    assert.equal(catalogAgentAction("modified", true), "confirm");
    assert.equal(catalogAgentAction("installed", true), "installed");
    assert.equal(catalogAgentAction("notInstalled", true), "install");
  });

  test("the install is an edit of the organization that the hub and this computer read", () => {
    assert.deepEqual(installAgentEdit("writer", false), { kind: "installCatalogAgent", id: "writer", overwrite: false });
    assert.deepEqual(installAgentEdit("writer", true), { kind: "installCatalogAgent", id: "writer", overwrite: true });
  });
});

describe("the gallery texts", () => {
  test("every agent the program ships has a Portuguese role and description, and none is left over", () => {
    const shipped = readdirSync(CATALOG)
      .filter((file) => file.endsWith(".md"))
      .map((file) => file.slice(0, -".md".length))
      .sort();
    assert.deepEqual(Object.keys(PT_BR_AGENTS).sort(), shipped);
    for (const [id, text] of Object.entries(PT_BR_AGENTS)) {
      assert.ok(text.role.trim() !== "" && text.description.trim() !== "", id);
    }
  });

  test("Portuguese shows the translation, English and an unknown agent keep the original text", () => {
    const original = { role: "Writer", description: "Drafts emails." };
    setLocale("pt-BR");
    assert.deepEqual(catalogAgentText("writer", original), PT_BR_AGENTS.writer);
    assert.deepEqual(catalogAgentText("a-newer-agent", original), original);
    setLocale("en");
    assert.deepEqual(catalogAgentText("writer", original), original);
  });
});
