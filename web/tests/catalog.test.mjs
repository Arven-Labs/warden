// P136 — the descriptions of the ready-made skills in the gallery: every skill the hub ships has a Portuguese one, and anything the
// catalog does not know keeps the hub's own text. Run with `npm test`.

import assert from "node:assert/strict";
import { readdirSync } from "node:fs";
import { afterEach, describe, test } from "node:test";
import { PT_BR_DESCRIPTIONS, catalogDescription } from "../src/i18n/catalog.ts";
import { setLocale } from "../src/i18n/index.ts";

afterEach(() => setLocale("en"));

const HUB_CATALOG = new URL("../../crates/warden-core/catalog/skills/", import.meta.url);

describe("the gallery descriptions", () => {
  test("every skill the hub ships has a Portuguese description, and none is left over", () => {
    const shipped = readdirSync(HUB_CATALOG)
      .filter((file) => file.endsWith(".md"))
      .map((file) => file.slice(0, -".md".length))
      .sort();
    assert.deepEqual(Object.keys(PT_BR_DESCRIPTIONS).sort(), shipped);
  });

  test("Portuguese shows the translation, English and an unknown skill keep the hub's text", () => {
    setLocale("pt-BR");
    assert.equal(catalogDescription("email-draft", "Draft a clear email."), PT_BR_DESCRIPTIONS["email-draft"]);
    assert.equal(catalogDescription("a-newer-skill", "Does something new."), "Does something new.");
    setLocale("en");
    assert.equal(catalogDescription("email-draft", "Draft a clear email."), "Draft a clear email.");
  });
});
