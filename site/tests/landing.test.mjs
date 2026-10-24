import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { test } from "node:test";

const read = (path) => readFileSync(new URL(path, import.meta.url), "utf8");
const guide = read("../../GUIA-DE-USO.md");
const tabs = read("../components/InstallTabs.tsx");
const landing = read("../components/Landing.tsx");

test("the install commands on the landing come from the guide", () => {
  for (const cmd of [
    "sudo apt install ./Warden_*_amd64.deb",
    "chmod +x Warden_*.AppImage && ./Warden_*.AppImage",
    "docker volume create warden-data",
    "--read-only --tmpfs /tmp -v warden-data:/data -p 7420:7420",
    "ghcr.io/arven-labs/warden-server:latest serve --no-embedding-warm-up --run-tasks",
  ]) {
    assert.ok(tabs.includes(cmd), `landing is missing: ${cmd}`);
    assert.ok(guide.includes(cmd), `guide no longer has: ${cmd}`);
  }
});

test("internal links on the landing point at real routes", () => {
  for (const [, href] of landing.matchAll(/href[:=]\s*"(\/docs\/[a-z-]+\/)"/g)) {
    assert.ok(read(`../content/docs/${href.split("/")[2]}.md`).length > 0, href);
  }
});

test("the landing copy is English", () => {
  assert.doesNotMatch(landing + tabs, /[ãõç]/);
});
