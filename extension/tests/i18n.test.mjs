import assert from "node:assert/strict";
import { describe, test } from "node:test";
import { en } from "../src/i18n/en.ts";
import { ptBR } from "../src/i18n/pt-BR.ts";
import { detectLocale, plural, setLocale, t } from "../src/i18n/index.ts";

const placeholders = (text) => [...text.matchAll(/\{(\w+)\}/g)].map((m) => m[1]).sort();

describe("the catalogs", () => {
  test("pt-BR has every key of en, and only those", () => {
    assert.deepEqual(Object.keys(ptBR).sort(), Object.keys(en).sort());
  });

  test("a text takes the same {placeholders} in both languages", () => {
    for (const key of Object.keys(en)) {
      assert.deepEqual(placeholders(ptBR[key]), placeholders(en[key]), key);
    }
  });
});

describe("t()", () => {
  test("English by default, and the {placeholder} left alone when not given", () => {
    assert.equal(t("ago.minutes", { count: 4 }), "4 min ago");
    assert.equal(t("ago.minutes", {}), "{count} min ago");
  });

  test("follows the language that is set", () => {
    setLocale("pt-BR");
    assert.equal(t("ago.minutes", { count: 4 }), "há 4 min");
    setLocale("en");
    assert.equal(t("ago.minutes", { count: 4 }), "4 min ago");
  });

  test("plural picks by the number", () => {
    assert.equal(plural(1, "replies.one", "replies.many"), "1 reply");
    assert.equal(plural(2, "replies.one", "replies.many"), "2 replies");
  });
});

describe("detectLocale", () => {
  test("a stored choice wins, then any pt browser language, then English", () => {
    assert.equal(detectLocale("en", ["pt-BR"]), "en");
    assert.equal(detectLocale(null, ["pt-PT"]), "pt-BR");
    assert.equal(detectLocale(null, ["fr", "en-US"]), "en");
    assert.equal(detectLocale(null, []), "en");
  });
});
