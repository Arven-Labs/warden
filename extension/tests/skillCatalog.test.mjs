// P140 — the pure half of the gallery in the skills tab (`src/sidepanel/lib/skillCatalog.ts`) and how the extension reads the hub's
// gallery messages (`src/protocol/messages.ts`). Run with `npm test`.

import assert from "node:assert/strict";
import { readdirSync } from "node:fs";
import { describe, test } from "node:test";
import { decode } from "../src/protocol/messages.ts";
import { PT_BR_DESCRIPTIONS, catalogAction, catalogDescription } from "../src/sidepanel/lib/skillCatalog.ts";

const HUB_CATALOG = new URL("../../crates/warden-core/catalog/skills/", import.meta.url);

describe("what a gallery row offers", () => {
  test("an installed entry offers nothing, a missing one offers to install", () => {
    assert.equal(catalogAction("installed", false), "installed");
    assert.equal(catalogAction("notInstalled", false), "install");
  });

  test("a skill of the same name that differs says so first, and only a second tap replaces it", () => {
    assert.equal(catalogAction("modified", false), "conflict");
    assert.equal(catalogAction("modified", true), "confirm");
  });

  test("an untouched older copy of the gallery's text offers a one-tap update, with nothing to confirm", () => {
    assert.equal(catalogAction("outdated", false), "update");
    assert.equal(catalogAction("outdated", true), "update");
  });

  test("asking to replace does not matter for an entry that is not in conflict", () => {
    assert.equal(catalogAction("installed", true), "installed");
    assert.equal(catalogAction("notInstalled", true), "install");
  });
});

describe("the gallery descriptions", () => {
  test("every skill the hub ships has a Portuguese description, and none is left over", () => {
    const shipped = readdirSync(HUB_CATALOG)
      .filter((file) => file.endsWith(".md"))
      .map((file) => file.slice(0, -".md".length))
      .sort();
    assert.deepEqual(Object.keys(PT_BR_DESCRIPTIONS).sort(), shipped);
  });

  test("Portuguese shows the translation, English and an unknown skill keep the hub's text", () => {
    assert.equal(catalogDescription("pt-BR", "email-draft", "Draft a clear email."), PT_BR_DESCRIPTIONS["email-draft"]);
    assert.equal(catalogDescription("pt-BR", "a-newer-skill", "Does something new."), "Does something new.");
    assert.equal(catalogDescription("en", "email-draft", "Draft a clear email."), "Draft a clear email.");
  });
});

describe("decoding the hub's gallery", () => {
  test("reads the entries with their state", () => {
    const entries = [
      { name: "email-draft", description: "Draft an email", body: "Be polite.", status: "notInstalled" },
      { name: "commit-message", description: "Write a commit", body: "Be brief.", status: "modified" },
    ];
    const reply = decode(JSON.stringify({ type: "skillCatalog", requestId: 7, entries }));
    assert.deepEqual(reply, { type: "skillCatalog", requestId: 7, entries });
  });
});
