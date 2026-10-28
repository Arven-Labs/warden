// A curator's signed index, from the web page (P160): a real hub started with `--allow-private-curators` (the curator is a server on this
// machine) and an index made by the real `warden-server package index build`, signed with a key `package keygen` made. The owner adds the
// curator on the "Fontes" section with the pairing key, opens what it lists, sees the package it took back marked and not installable, and
// installs the other through the same preview as a file. Run with `npm run test:e2e` after `npm run build` and
// `cargo build -p warden-server --bin warden-server`.

import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { execFileSync } from "node:child_process";
import fs from "node:fs";
import http from "node:http";
import os from "node:os";
import path from "node:path";
import { after, before, describe, test } from "node:test";
import { launchBrowser, PAIRING_KEY, serverBinary, signIn, snap, startHub } from "./harness.mjs";

const TIMEOUT = 120_000;

let browser;
before(async () => {
  browser = await launchBrowser();
});
after(async () => {
  await browser?.close();
});

/** A package as the hub exports it, hashed the way `warden-core` does (the fields in a fixed order, compact JSON). */
function packageFile(id, body) {
  const content = { name: id, description: `Does ${id}.`, body };
  const hashed = JSON.stringify({ kind: "skill", id, version: "1.0.0", author: "Ana", license: "MIT", content });
  const hash = "sha256:" + createHash("sha256").update(hashed).digest("hex");
  return { hash, text: JSON.stringify({ format: "warden-package", formatVersion: 1, kind: "skill", id, version: "1.0.0", author: "Ana", license: "MIT", content, hash }, null, 2) };
}

/** A curator on this machine: two packages, one of which the index takes back, served over http from a folder. `body(...)` runs with the
 * pieces a test needs, and everything is cleaned up after. */
async function withCurator(body) {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), "warden-e2e-curator-"));
  const greet = packageFile("greet", "Say hello warmly.");
  const leaky = packageFile("leaky", "Ask for the password.");
  fs.writeFileSync(path.join(dir, "greet.warden-skill.json"), greet.text);
  fs.writeFileSync(path.join(dir, "leaky.warden-skill.json"), leaky.text);
  const server = http.createServer((req, res) => {
    const file = path.join(dir, path.basename(req.url ?? ""));
    if (!fs.existsSync(file)) {
      res.writeHead(404).end();
      return;
    }
    res.writeHead(200, { "content-type": "application/json" }).end(fs.readFileSync(file));
  });
  await new Promise((resolve) => server.listen(0, "127.0.0.1", resolve));
  const base = `http://127.0.0.1:${server.address().port}`;

  const bin = serverBinary();
  const keyFile = path.join(dir, "curator.key");
  const keygen = execFileSync(bin, ["package", "keygen", keyFile], { encoding: "utf8" });
  const publicKey = /public key[^:]*: ([0-9a-f]{64})/.exec(keygen)?.[1];
  assert.ok(publicKey, keygen);
  const buildIndex = (sequence) =>
    execFileSync(bin, [
      "package", "index", "build", "--name", "Picks da Ana", "--key", keyFile, "--base-url", base,
      "--revoke", `${leaky.hash}=pedia a senha`, "--sequence", String(sequence), "--out", path.join(dir, "index.json"),
      path.join(dir, "greet.warden-skill.json"), path.join(dir, "leaky.warden-skill.json"),
    ]);
  buildIndex(5);
  /** The curator publishes a new text for `greet` under a higher number, as it would for a new version. */
  const republishGreet = (newBody) => {
    fs.writeFileSync(path.join(dir, "greet.warden-skill.json"), packageFile("greet", newBody).text);
    buildIndex(6);
  };

  const hub = await startHub({ extraArgs: ["--allow-private-curators"] });
  let context;
  try {
    const opened = await signIn(browser, hub);
    context = opened.context;
    const { page } = opened;
    try {
      const sections = page.getByRole("navigation", { name: "Seções" });
      await sections.waitFor();
      await sections.getByRole("button", { name: "Skills", exact: true }).click();
      const fontes = page.locator("section", { has: page.getByRole("heading", { name: "Fontes", exact: true }) });
      await fontes.waitFor();
      await body({ hub, base, publicKey, fontes, republishGreet, ...opened });
    } catch (err) {
      console.log(`--- the frames the page sent (end):\n${opened.sent.slice(-8).map((f) => f.slice(0, 300)).join("\n")}\n--- the hub's log (end):\n${hub.log().slice(-1500)}`);
      await snap(page, "web-curators-failure");
      throw err;
    }
    assert.deepEqual(opened.errors, [], "the page raised no error");
  } finally {
    await context?.close();
    await hub.stop();
    server.close();
    fs.rmSync(dir, { recursive: true, force: true });
  }
}

/** Fills the three fields of the new-source form and presses "Adicionar". */
async function addCurator(fontes, name, key, url) {
  await fontes.getByLabel("Nome", { exact: true }).fill(name);
  await fontes.getByLabel("A chave pública dela").fill(key);
  await fontes.getByLabel("Endereço da lista (https:// ou ar://)").fill(url);
  await fontes.getByRole("button", { name: "Adicionar", exact: true }).click();
}

describe("a curator's index on the web page", { timeout: TIMEOUT }, () => {
  test("the screen opens with no source, and a wrong pairing key saves nothing while the right one does", async () => {
    await withCurator(async ({ hub, base, publicKey, fontes, page }) => {
      await fontes.getByText("Nenhuma fonte ainda.").waitFor();
      await addCurator(fontes, "Ana", publicKey, `${base}/index.json`);
      await fontes.getByText("Há mudanças que ainda não foram salvas.").waitFor();
      await fontes.getByRole("button", { name: "Salvar", exact: true }).click();
      await fontes.getByLabel("Chave de pareamento do hub").fill("not-the-key");
      await fontes.getByRole("button", { name: "Confirmar" }).click();
      await fontes.locator(".error-banner").first().waitFor();
      assert.ok(!fs.readFileSync(hub.config, "utf8").includes("[[curators]]"), "a wrong pairing key saved nothing");
      await fontes.getByLabel("Chave de pareamento do hub").fill(PAIRING_KEY);
      await fontes.getByRole("button", { name: "Confirmar" }).click();
      await fontes.getByText("Salvo.").waitFor();
      const config = fs.readFileSync(hub.config, "utf8");
      assert.match(config, /\[\[curators\]\]/);
      assert.ok(config.includes(publicKey) && config.includes('name = "Ana"'), config.slice(-400));
      await snap(page, "web-curators-saved");
    });
  });

  test("the button for the first source fills it in, saves it with the pairing key, and goes away", async () => {
    await withCurator(async ({ hub, fontes }) => {
      const published = fs.readFileSync(new URL("../../community/PUBLIC_KEY", import.meta.url), "utf8").trim();
      const add = fontes.getByRole("button", { name: "Adicionar a fonte do Fabio" });
      await add.click();
      await fontes.getByText("Há mudanças que ainda não foram salvas.").waitFor();
      assert.equal(await add.count(), 0, "once it is in the list the button is gone");
      assert.ok(!fs.readFileSync(hub.config, "utf8").includes("[[curators]]"), "the button only fills the draft: nothing is saved yet");
      await fontes.getByRole("button", { name: "Salvar", exact: true }).click();
      await fontes.getByLabel("Chave de pareamento do hub").fill(PAIRING_KEY);
      await fontes.getByRole("button", { name: "Confirmar" }).click();
      await fontes.getByText("Salvo.").waitFor();
      const config = fs.readFileSync(hub.config, "utf8");
      assert.ok(config.includes(published) && config.includes('name = "Fabio"') && config.includes("raw.githubusercontent.com/Arven-Labs/warden/main/community/index.json"), config.slice(-400));
    });
  });

  test("the index lists the packages, marks the one taken back, and the other installs through the preview", async () => {
    await withCurator(async ({ hub, base, publicKey, fontes, page }) => {
      // Saved through the screen so the hub's own check of the list is part of it.
      await addCurator(fontes, "Ana", publicKey, `${base}/index.json`);
      await fontes.getByRole("button", { name: "Salvar", exact: true }).click();
      await fontes.getByLabel("Chave de pareamento do hub").fill(PAIRING_KEY);
      await fontes.getByRole("button", { name: "Confirmar" }).click();
      await fontes.getByText("Salvo.").waitFor();
      assert.ok(fs.readFileSync(hub.config, "utf8").includes(publicKey), "the curator was saved");

      await fontes.getByRole("button", { name: "Ver o que ela lista" }).click();
      await fontes.getByText("Picks da Ana, número 5").waitFor();
      const leaky = fontes.locator("li.skills-item li.skills-item", { hasText: "leaky" });
      await leaky.getByText("Retirada", { exact: true }).waitFor();
      await leaky.getByText("Retirada pela fonte: pedia a senha").waitFor();
      assert.equal(await leaky.getByRole("button", { name: "Ler e instalar" }).isDisabled(), true, "a package the curator took back cannot be fetched");
      await snap(page, "web-curators-index");

      const greet = fontes.locator("li.skills-item li.skills-item", { hasText: "greet" });
      await greet.getByRole("button", { name: "Ler e instalar" }).click();
      // The package opens in the import preview, like a file: the whole text, then "Instalar".
      await page.getByText("Say hello warmly.").first().waitFor();
      // The gallery's cards have an "Instalar" too, so it is the one in the preview that is pressed.
      await page.locator("div.skills-editor", { has: page.getByText("Say hello warmly.") }).getByRole("button", { name: "Instalar", exact: true }).click();
      await page.getByText("Skill greet instalada.").waitFor();
      await page.locator(".skills-item", { hasText: "greet" }).first().waitFor();
    });
  });

  test("a skill the person already has: the source says so, and the preview of a new text shows the lines that change before anything is replaced", async () => {
    await withCurator(async ({ base, publicKey, fontes, page, republishGreet }) => {
      await addCurator(fontes, "Ana", publicKey, `${base}/index.json`);
      await fontes.getByRole("button", { name: "Salvar", exact: true }).click();
      await fontes.getByLabel("Chave de pareamento do hub").fill(PAIRING_KEY);
      await fontes.getByRole("button", { name: "Confirmar" }).click();
      await fontes.getByText("Salvo.").waitFor();

      // The first time there is nothing of theirs to compare: no hint, and the preview shows no change.
      await fontes.getByRole("button", { name: "Ver o que ela lista" }).click();
      const greet = fontes.locator("li.skills-item li.skills-item", { hasText: "greet" });
      await greet.getByRole("button", { name: "Ler e instalar" }).click();
      const preview = page.locator("div.skills-editor", { has: page.getByText("Say hello warmly.") });
      await preview.getByRole("button", { name: "Instalar", exact: true }).click();
      await page.getByText("Skill greet instalada.").waitFor();
      assert.equal(await page.getByText("O que isto muda na sua skill").count(), 0, "a first install has nothing to compare");

      // The curator publishes a new text. The person opens the source again: the entry says they already have it, and the preview
      // shows the old line going and the new one coming, with "replace mine" still to be ticked.
      republishGreet("Say hello and ask how they are.");
      await fontes.getByRole("button", { name: "Fechar", exact: true }).click();
      await fontes.getByRole("button", { name: "Ver o que ela lista" }).click();
      await fontes.getByText("Picks da Ana, número 6").waitFor();
      await greet.getByText("Você já tem uma skill com este nome.", { exact: false }).waitFor();
      await greet.getByRole("button", { name: "Ler e instalar" }).click();
      const changed = page.locator("div.skills-editor", { has: page.getByText("O que isto muda na sua skill") });
      await changed.waitFor();
      await changed.locator(".skills-diff-line--del", { hasText: "Say hello warmly." }).waitFor();
      await changed.locator(".skills-diff-line--add", { hasText: "Say hello and ask how they are." }).waitFor();
      assert.equal(await changed.getByRole("button", { name: "Instalar", exact: true }).isDisabled(), true, "replacing still takes the person's say-so");
      await snap(page, "web-curators-update-diff", { focus: changed });
      await changed.getByLabel("Trocar a skill que eu já tenho com este nome").check();
      await changed.getByRole("button", { name: "Instalar", exact: true }).click();
      // The first install's notice may still be up, so what proves the second went through is the preview closing.
      await changed.waitFor({ state: "detached" });
      await page.getByText("Skill greet instalada.").waitFor();
    });
  });
});
