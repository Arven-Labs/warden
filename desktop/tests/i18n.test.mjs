// P136 — the words of the app: English by default, Portuguese when asked, nothing missing in either, and the language picked by the
// person or by the system. Run with `npm test`.

import assert from "node:assert/strict";
import { afterEach, describe, test } from "node:test";
import { en } from "../src/i18n/en.ts";
import { ptBR } from "../src/i18n/pt-BR.ts";
import { detectLocale, intlTag, locale, plural, setLocale, t } from "../src/i18n/index.ts";

afterEach(() => setLocale("en"));

describe("the catalogs", () => {
  test("Portuguese has every key of English, and no other", () => {
    assert.deepEqual(Object.keys(ptBR).sort(), Object.keys(en).sort());
  });

  test("every text has the same {names} in both languages, so none is lost in a translation", () => {
    const names = (text) => [...text.matchAll(/\{(\w+)\}/g)].map((m) => m[1]).sort();
    for (const key of Object.keys(en)) assert.deepEqual(names(ptBR[key]), names(en[key]), key);
  });

  test("no text is empty", () => {
    for (const [key, text] of [...Object.entries(en), ...Object.entries(ptBR)]) assert.notEqual(text.trim(), "", key);
  });
});

describe("t", () => {
  test("reads in English by default, and in Portuguese once it is picked", () => {
    assert.equal(locale(), "en");
    assert.equal(t("bucket.today"), "Today");
    setLocale("pt-BR");
    assert.equal(t("bucket.today"), "Hoje");
    assert.equal(intlTag(), "pt-BR");
  });

  test("puts each {name} where it belongs, and leaves one it was not given", () => {
    assert.equal(t("work.to", { name: "ana" }), "to ana");
    assert.equal(t("work.to"), "to {name}");
    setLocale("pt-BR");
    assert.equal(t("work.to", { name: "ana" }), "para ana");
  });

  test("a number can fill a {name} too", () => {
    assert.equal(t("work.buttonCount", { count: 3 }), "Notes and runs (3)");
  });

  test("plural picks the text for one or for several", () => {
    assert.equal(plural(1, "replies.one", "replies.many"), "1 reply");
    assert.equal(plural(4, "replies.one", "replies.many"), "4 replies");
    setLocale("pt-BR");
    assert.equal(plural(1, "replies.one", "replies.many"), "1 resposta");
    assert.equal(plural(4, "replies.one", "replies.many"), "4 respostas");
  });
});

describe("the language of the person", () => {
  test("what they picked wins over the system", () => {
    assert.equal(detectLocale("en", ["pt-BR"]), "en");
    assert.equal(detectLocale("pt-BR", ["en-US"]), "pt-BR");
  });

  test("with nothing picked, any Portuguese in the system reads as pt-BR and the rest as English", () => {
    assert.equal(detectLocale(null, ["pt-PT", "en"]), "pt-BR");
    assert.equal(detectLocale(null, ["en-US", "pt-BR"]), "pt-BR");
    assert.equal(detectLocale(null, ["fr-FR"]), "en");
    assert.equal(detectLocale(null, []), "en");
  });

  test("a stored value that is not a language is ignored", () => {
    assert.equal(detectLocale("klingon", ["en"]), "en");
    assert.equal(detectLocale("klingon", ["pt-BR"]), "pt-BR");
  });
});
