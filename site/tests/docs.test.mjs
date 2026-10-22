import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { test } from "node:test";
import { DOC_SLUGS, allDocs } from "../lib/docs.ts";

const docs = allDocs();

test("every page has a title and a unique slug", () => {
  assert.equal(new Set(DOC_SLUGS).size, DOC_SLUGS.length);
  for (const doc of docs) assert.ok(doc.title.length > 0, doc.slug);
});

test("internal links point at existing pages", () => {
  const slugs = new Set(DOC_SLUGS);
  for (const doc of docs) {
    for (const [, slug] of doc.body.matchAll(/\]\(\/docs\/([a-z-]+)\/\)/g)) {
      assert.ok(slugs.has(slug), `${doc.slug}.md links to a missing page: ${slug}`);
    }
  }
});

test("pages are English: no accented Portuguese prose", () => {
  // Product strings that stay in Portuguese on purpose (the shared-space folder and tab names).
  const allowed = /compartilhado|Espaços compartilhados/g;
  for (const doc of docs) {
    const text = doc.body.replace(allowed, "");
    assert.doesNotMatch(text, /[ãõçé]/, `${doc.slug}.md has Portuguese text`);
  }
});

/** The config keys of the first ```toml block: table headers and `key =` names. */
function tomlKeys(markdown) {
  const block = /```toml\n([\s\S]*?)```/.exec(markdown);
  assert.ok(block, "no toml block");
  return block[1]
    .split("\n")
    .map((line) => line.trim())
    .filter((line) => line.startsWith("[") || /^[a-z_]+\s*=/.test(line))
    .map((line) => (line.startsWith("[") ? line : line.split("=")[0].trim()));
}

test("the hub page's config example has the same keys as the Portuguese guide's", () => {
  const guide = readFileSync(new URL("../../GUIA-DE-USO.md", import.meta.url), "utf8");
  const hub = docs.find((d) => d.slug === "hub");
  assert.deepEqual(tomlKeys(hub.body), tomlKeys(guide));
});
