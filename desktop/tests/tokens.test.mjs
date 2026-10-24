import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { test } from "node:test";

const read = (path) => readFileSync(new URL(path, import.meta.url), "utf8");

test("the app's token copy matches design/tokens.css", () => {
  assert.equal(read("../src/tokens.css"), read("../../design/tokens.css"));
});

test("the app does not redefine the shared colors, only adds its own", () => {
  const css = read("../src/App.css");
  for (const name of ["--color-bg", "--color-surface", "--color-text", "--color-accent"]) {
    assert.doesNotMatch(css, new RegExp(`${name}:`), `${name} belongs in design/tokens.css`);
  }
  assert.doesNotMatch(css, /prefers-color-scheme/, "the app is dark by default and does not follow the system");
});
