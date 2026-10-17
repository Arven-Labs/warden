// P157 — the small readings behind the chat's copy buttons (`src/lib/clipboard.ts`). Run with `npm test`.

import assert from "node:assert/strict";
import { afterEach, describe, test } from "node:test";
import { codeLanguage, copyText, hastText } from "../src/lib/clipboard.ts";

describe("the language of a fenced block", () => {
  test("is what follows language- in the classes of its code", () => {
    assert.equal(codeLanguage(["hljs", "language-ts"]), "ts");
    assert.equal(codeLanguage("language-rust"), "rust");
    assert.equal(codeLanguage(["language-c++", "hljs"]), "c++");
  });

  test("is nothing for a block that names none", () => {
    assert.equal(codeLanguage(undefined), null);
    assert.equal(codeLanguage([]), null);
    assert.equal(codeLanguage(["hljs"]), null);
    assert.equal(codeLanguage(["language-"]), null);
  });
});

describe("the text of a highlighted block", () => {
  test("joins the words the highlighter wrapped, in order, without the wrapping", () => {
    const tree = {
      type: "element",
      children: [
        { type: "element", children: [{ type: "text", value: "const" }] },
        { type: "text", value: " x = " },
        { type: "element", children: [{ type: "text", value: "1" }] },
        { type: "text", value: ";\n" },
      ],
    };
    assert.equal(hastText(tree), "const x = 1;\n");
  });

  test("is empty for no tree, an empty block, or a node with no words", () => {
    assert.equal(hastText(undefined), "");
    assert.equal(hastText({ type: "element", children: [] }), "");
    assert.equal(hastText({ type: "comment", value: "not text" }), "");
  });
});

describe("copyText", () => {
  const saved = { navigator: Object.getOwnPropertyDescriptor(globalThis, "navigator"), document: Object.getOwnPropertyDescriptor(globalThis, "document") };

  const set = (name, value) => Object.defineProperty(globalThis, name, { value, configurable: true, writable: true });
  const restore = (name) => {
    if (saved[name]) Object.defineProperty(globalThis, name, saved[name]);
    else delete globalThis[name];
  };

  /** A page's `document` that records what the old way did: the text put in the hidden box and whether it was removed. */
  const fakeDocument = (copies) => {
    const area = { value: "", style: {}, setAttribute() {}, select() {}, removed: false, remove() { this.removed = true; } };
    return {
      area,
      createElement: () => area,
      body: { appendChild() {} },
      execCommand: (command) => {
        assert.equal(command, "copy");
        return copies;
      },
    };
  };

  afterEach(() => {
    restore("navigator");
    restore("document");
  });

  test("uses the clipboard API when the page has one", async () => {
    let written = null;
    set("navigator", { clipboard: { writeText: async (text) => { written = text; } } });
    assert.equal(await copyText("hello"), true);
    assert.equal(written, "hello");
  });

  test("falls back to a hidden box where there is no clipboard API, and cleans it up", async () => {
    set("navigator", {});
    const document = fakeDocument(true);
    set("document", document);
    assert.equal(await copyText("plain http"), true);
    assert.equal(document.area.value, "plain http");
    assert.equal(document.area.removed, true);
  });

  test("falls back too when the API refuses", async () => {
    set("navigator", { clipboard: { writeText: async () => { throw new Error("denied"); } } });
    const document = fakeDocument(true);
    set("document", document);
    assert.equal(await copyText("x"), true);
    assert.equal(document.area.value, "x");
  });

  test("says false when nothing let it copy", async () => {
    set("navigator", {});
    set("document", fakeDocument(false));
    assert.equal(await copyText("x"), false);
    delete globalThis.document;
    assert.equal(await copyText("x"), false, "and with no page at all");
  });
});
