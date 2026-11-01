// The rules of `browser_type_text` that decide, before anything is written, whether a field may be typed into (`src/protocol/domGuards.ts`),
// and how its arguments are read. The model chooses both the field and the text, and a page the model has read can tell it what to choose, so
// these are the lines that keep a hostile page from having a password, a card number or a one-time code filled for it. Run with `npm test`.

import assert from "node:assert/strict";
import { describe, test } from "node:test";
import { MAX_TYPED_CHARS, parseTypeTextArgs, refusalFor } from "../src/protocol/domGuards.ts";

/** The facts the page reports about a field (a plain visible text input unless a test says otherwise). */
const field = (over = {}) => ({ tag: "input", type: "text", autocomplete: "", name: "q", id: "", label: "", readOnly: false, disabled: false, visible: true, ...over });

describe("the fields that may be typed into", () => {
  test("a visible, editable text field, a search box, an email, a url, a phone and a number are fine", () => {
    for (const type of ["text", "search", "email", "url", "tel", "number", ""]) {
      assert.equal(refusalFor(field({ type })), null, `type "${type}"`);
    }
    assert.equal(refusalFor(field({ tag: "textarea", type: "" })), null);
  });

  test("the type is read without regard to case or stray spaces", () => {
    assert.equal(refusalFor(field({ type: " TEXT " })), null);
    assert.match(refusalFor(field({ type: " Password " })) ?? "", /password/i);
  });
});

describe("what is refused, and why", () => {
  test("a password field, however it is written", () => {
    assert.match(refusalFor(field({ type: "password" })) ?? "", /password/i);
    assert.match(refusalFor(field({ type: "PASSWORD" })) ?? "", /password/i);
  });

  test("a field that says by its autocomplete that it holds a secret: password, one-time code, card data", () => {
    for (const autocomplete of ["current-password", "new-password", "one-time-code", "cc-number", "cc-csc", "cc-exp", "cc-exp-month", "cc-name", "CC-NUMBER"]) {
      assert.match(refusalFor(field({ autocomplete })) ?? "", /credential|secret|card/i, autocomplete);
    }
  });

  test("a secret token among several in the autocomplete is found; an ordinary one is not", () => {
    assert.notEqual(refusalFor(field({ autocomplete: "section-pay billing cc-number" })), null);
    for (const autocomplete of ["email", "username", "name", "street-address", "off", "on"]) {
      assert.equal(refusalFor(field({ autocomplete })), null, autocomplete);
    }
  });

  test("a text field named like a secret, since some sites mask a password with a plain text input", () => {
    for (const probe of [{ name: "user_password" }, { id: "passwd" }, { label: "Senha" }, { name: "cvv" }, { id: "card-cvc" }, { label: "One-time code" }, { name: "otp" }]) {
      assert.notEqual(refusalFor(field(probe)), null, JSON.stringify(probe));
    }
    for (const probe of [{ name: "passport_country" }, { label: "Search" }, { name: "compass" }]) {
      assert.equal(refusalFor(field(probe)), null, JSON.stringify(probe));
    }
  });

  test("anything that is not a text box: buttons, checkboxes, files, hidden fields, a div, a contenteditable region", () => {
    for (const type of ["hidden", "file", "checkbox", "radio", "button", "submit", "reset", "image", "range", "color", "date"]) {
      assert.match(refusalFor(field({ type })) ?? "", /text field|not a/i, type);
    }
    assert.match(refusalFor(field({ tag: "div", type: "" })) ?? "", /text field|not a/i);
    assert.match(refusalFor(field({ tag: "select", type: "" })) ?? "", /text field|not a/i);
  });

  test("a field that is disabled, read-only or not visible", () => {
    assert.match(refusalFor(field({ disabled: true })) ?? "", /disabled/i);
    assert.match(refusalFor(field({ readOnly: true })) ?? "", /read-only/i);
    assert.match(refusalFor(field({ visible: false })) ?? "", /visible|hidden/i);
  });
});

describe("the arguments of browser_type_text", () => {
  test("a selector and a text are required; the tab and append are optional", () => {
    assert.deepEqual(parseTypeTextArgs({ selector: "#q", text: "olá" }), { selector: "#q", text: "olá", append: false });
    assert.deepEqual(parseTypeTextArgs({ selector: "#q", text: "x", append: true, tabId: 4 }), { selector: "#q", text: "x", append: true, tabId: 4 });
  });

  test("an empty text is allowed (it clears the field, and still asks)", () => {
    assert.equal(parseTypeTextArgs({ selector: "#q", text: "" }).text, "");
  });

  test("a missing, blank or non-text selector is refused", () => {
    for (const selector of [undefined, "", "   ", 5, null]) {
      assert.throws(() => parseTypeTextArgs({ selector, text: "x" }), /selector/, String(selector));
    }
  });

  test("a missing or non-text text is refused, and so is one past the ceiling", () => {
    for (const text of [undefined, 5, null, ["a"]]) {
      assert.throws(() => parseTypeTextArgs({ selector: "#q", text }), /text/, String(text));
    }
    assert.equal(parseTypeTextArgs({ selector: "#q", text: "a".repeat(MAX_TYPED_CHARS) }).text.length, MAX_TYPED_CHARS);
    assert.throws(() => parseTypeTextArgs({ selector: "#q", text: "a".repeat(MAX_TYPED_CHARS + 1) }), /text/);
  });

  test("append and tabId must have the right type when given", () => {
    assert.throws(() => parseTypeTextArgs({ selector: "#q", text: "x", append: "yes" }), /append/);
    assert.throws(() => parseTypeTextArgs({ selector: "#q", text: "x", tabId: "4" }), /tabId/);
  });

  test("the arguments must be an object", () => {
    for (const args of [null, undefined, "x", 5, ["x"]]) {
      assert.throws(() => parseTypeTextArgs(args), /arguments/, String(args));
    }
  });

  test("there is no way to ask for the form to be sent: Enter, submit and the like are not arguments, and any such key is not carried through", () => {
    const parsed = parseTypeTextArgs({ selector: "#q", text: "x", submit: true, pressEnter: true, enter: true });
    assert.deepEqual(Object.keys(parsed).sort(), ["append", "selector", "text"]);
  });
});
