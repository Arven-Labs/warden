// A signing key made on the web page (P160): a real hub, a skill made on the screen, then "Criar uma chave nova" in the share form.
// The key is made in the browser and must never travel to the hub: the file it downloads is read back here, the export is signed with
// it and the hub (which reads the signed file back) names the same signer, and "Confiar nesta chave" lists the public key in the
// hub's config with the pairing key. Run with `npm run test:e2e` after `npm run build` and `cargo build -p warden-server --bin warden-server`.

import assert from "node:assert/strict";
import fs from "node:fs";
import { after, before, describe, test } from "node:test";
import { launchBrowser, PAIRING_KEY, signIn, snap, startHub } from "./harness.mjs";

const TIMEOUT = 120_000;

let browser;
before(async () => {
  browser = await launchBrowser();
});
after(async () => {
  await browser?.close();
});

/** A hub and a page on the Skills tab with a skill, "Compartilhar" open, and "Assinar com a minha chave" ticked. */
async function withShareForm(body) {
  const hub = await startHub();
  let context;
  try {
    const opened = await signIn(browser, hub);
    context = opened.context;
    const { page } = opened;
    try {
      const sections = page.getByRole("navigation", { name: "Seções" });
      await sections.waitFor();
      await sections.getByRole("button", { name: "Skills", exact: true }).click();
      await page.getByRole("button", { name: "+ Nova" }).click();
      await page.getByPlaceholder("review-pr").fill("greet");
      await page.getByPlaceholder(/Uma frase/).fill("Greets.");
      await page.getByPlaceholder(/As instruções completas/).fill("Say hello.");
      await page.getByRole("button", { name: "Salvar", exact: true }).click();
      await page.getByRole("button", { name: "Compartilhar", exact: true }).first().click();
      await page.getByLabel("Assinar com a minha chave").check();
      await body({ hub, ...opened });
    } catch (err) {
      console.log(`--- the frames the page sent (end):\n${opened.sent.slice(-8).map((f) => f.slice(0, 300)).join("\n")}\n--- the hub's log (end):\n${hub.log().slice(-1500)}`);
      await snap(page, "web-signing-key-failure");
      throw err;
    }
    assert.deepEqual(opened.errors, [], "the page raised no error");
  } finally {
    await context?.close();
    await hub.stop();
  }
}

/** "Criar uma chave nova", and the key file the page handed over, read from the download. */
async function makeKey(page) {
  const [download] = await Promise.all([page.waitForEvent("download"), page.getByRole("button", { name: "Criar uma chave nova" }).click()]);
  return fs.readFileSync(await download.path(), "utf8");
}

describe("a signing key made on the web page", { timeout: TIMEOUT }, () => {
  test("the key file is the one `ring` reads, the public key is shown, and the private half never reaches the hub", async () => {
    await withShareForm(async ({ page, sent }) => {
      const text = await makeKey(page);
      assert.match(text, /^3053020101300506032b657004220420[0-9a-f]{64}a123032100[0-9a-f]{64}\n$/, "a PKCS#8 v2 key file in hex");
      const publicKey = text.trim().slice(-64);
      await page.getByText(/Assinando como/).waitFor();
      assert.match(await page.getByText(/Assinando como/).innerText(), new RegExp(publicKey.slice(0, 16)));
      await page.getByText(/Guarde em um lugar seguro/).waitFor();
      const seed = text.trim().slice(32, 96);
      assert.ok(!sent.some((f) => f.includes(seed)), "the seed was not sent to the hub");
    });
  });

  test("the export is signed with the new key and the hub names that key as the signer", async () => {
    await withShareForm(async ({ page, sent }) => {
      const publicKey = (await makeKey(page)).trim().slice(-64);
      const [download] = await Promise.all([page.waitForEvent("download"), page.getByRole("button", { name: "Baixar arquivo" }).click()]);
      const file = JSON.parse(fs.readFileSync(await download.path(), "utf8"));
      assert.match(file.signature, new RegExp(`^ed25519:${publicKey}:[0-9a-f]{128}$`));
      // The hub read the signed file back (`previewSkillPackage`): that is what makes the screen say it was signed.
      assert.ok(sent.some((f) => f.includes("previewSkillPackage") && f.includes(publicKey)), "the signed file went back to the hub to be read");
      await page.getByText(/Assinado por você/).waitFor();
    });
  });

  test("the owner lists the new key in Quem assina with the pairing key, and a wrong key is refused", async () => {
    await withShareForm(async ({ page, hub }) => {
      const publicKey = (await makeKey(page)).trim().slice(-64);
      await page.getByRole("button", { name: "Confiar nesta chave" }).click();
      await page.getByLabel("Nome para esta chave").fill("Eu");
      await page.getByRole("button", { name: "Confiar", exact: true }).click();
      await page.getByLabel("Chave de pareamento do hub").fill("not-the-key");
      await page.getByRole("button", { name: "Confirmar" }).click();
      await page.locator(".error-banner").first().waitFor();
      assert.ok(!fs.readFileSync(hub.config, "utf8").includes(publicKey), "a wrong pairing key listed nothing");
      await page.getByLabel("Chave de pareamento do hub").fill(PAIRING_KEY);
      await page.getByRole("button", { name: "Confirmar" }).click();
      await page.getByText(/agora está na lista de quem assina/).waitFor();
      const config = fs.readFileSync(hub.config, "utf8");
      assert.match(config, /\[\[trusted_publishers\]\]/);
      assert.ok(config.includes(publicKey) && config.includes('name = "Eu"'), config.slice(-400));
    });
  });
});
