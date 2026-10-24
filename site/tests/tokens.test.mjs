import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { test } from "node:test";

const read = (path) => readFileSync(new URL(path, import.meta.url), "utf8");

test("the site's token copy matches design/tokens.css", () => {
  assert.equal(read("../app/tokens.css"), read("../../design/tokens.css"));
});
