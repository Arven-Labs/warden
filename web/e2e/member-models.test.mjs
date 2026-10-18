// P100 end to end: the owner releases a model to a member in Pessoas → "Modelos", and the member gives it to an agent of their own in
// Agentes. Before the release the picker is not there at all. Run with `npm run test:e2e` (see `settings.test.mjs` for what it needs).

import assert from "node:assert/strict";
import { after, before, describe, test } from "node:test";
import { PAIRING_KEY, launchBrowser, signIn, snap, startHub } from "./harness.mjs";

const TIMEOUT = 120_000;

let browser;
before(async () => {
  browser = await launchBrowser();
});
after(async () => {
  await browser?.close();
});

/** The pairing key every change in Pessoas asks for, typed into the form whose button is `submit`. */
const typeKey = (page, submit) =>
  page
    .locator("form.settings-confirm", { has: page.getByRole("button", { name: submit, exact: true }) })
    .getByLabel("Chave de pareamento do hub")
    .fill(PAIRING_KEY);

/** The owner's page: makes `ana` and returns her temporary password. */
async function createAna(page) {
  await page.getByRole("button", { name: "Pessoas" }).click();
  await page.getByRole("button", { name: "Adicionar pessoa" }).click();
  await page.getByLabel("Usuário").fill("ana");
  await page.getByLabel("Nome", { exact: true }).fill("Ana Souza");
  await typeKey(page, "Criar");
  await page.getByRole("button", { name: "Criar" }).click();
  const shown = page.locator(".settings-confirm", { hasText: "Senha provisória de" });
  await shown.waitFor();
  const password = (await shown.locator("code").innerText()).trim();
  await shown.getByRole("button", { name: "Já anotei" }).click();
  return password;
}

/** A new page signed in as `ana`, past the forced change of password and the recovery code, if the hub shows one. */
async function signInAsAna(hub, temporary) {
  const context = await browser.newContext({ viewport: { width: 1100, height: 1400 } });
  const page = await context.newPage();
  const errors = [];
  page.on("pageerror", (e) => errors.push(e.message));
  await page.goto(hub.url);
  await page.getByRole("tab", { name: "Usuário" }).click();
  await page.getByLabel("Usuário").fill("ana");
  await page.getByLabel("Senha").fill(temporary);
  await page.getByRole("button", { name: "Entrar" }).click();
  await page.getByText("troque a senha provisória por uma sua").waitFor();
  await page.getByLabel("Senha provisória").fill(temporary);
  await page.getByLabel("Senha nova", { exact: true }).fill("a-new-password-123");
  await page.getByLabel("Repita a senha nova").fill("a-new-password-123");
  await page.getByRole("button", { name: "Trocar senha" }).click();
  const newConversation = page.getByRole("button", { name: "+ Nova conversa" });
  const saved = page.getByLabel(/anotei|guardei|salvei/i);
  await Promise.race([newConversation.waitFor(), saved.waitFor()]);
  if (await saved.count()) {
    await saved.check();
    await page.getByRole("button", { name: /continuar|concluir|pronto/i }).click();
    await newConversation.waitFor();
  }
  return { page, context, errors };
}

describe("the models a member may give her agents", () => {
  test("the picker appears once the owner releases a model, and the agent keeps it", { timeout: TIMEOUT }, async () => {
    const hub = await startHub();
    const owner = await signIn(browser, hub);
    let member;
    try {
      await owner.page.getByRole("button", { name: "+ Nova conversa" }).waitFor();
      const temporary = await createAna(owner.page);
      member = await signInAsAna(hub, temporary);
      const { page } = member;

      // Nothing released: the form for a new agent has no model to pick.
      await page.getByRole("button", { name: "Agentes", exact: true }).click();
      await page.getByRole("button", { name: "Criar agente" }).click();
      await snap(page, "member-agent-form");
      assert.equal(await page.locator("label", { hasText: "Modelo" }).locator("select").count(), 0, "no picker before the owner releases a model");
      await page.getByRole("button", { name: "Cancelar" }).click();

      // The owner releases the hub's `main` model to Ana.
      const ana = owner.page.locator("li", { hasText: "Ana Souza" });
      await ana.getByRole("button", { name: "Modelos (só o padrão)" }).click();
      await owner.page.getByLabel("main", { exact: true }).check();
      await typeKey(owner.page, "Salvar modelos");
      await owner.page.getByRole("button", { name: "Salvar modelos" }).click();
      await ana.getByRole("button", { name: "Modelos (1 liberado)" }).waitFor();

      // She leaves the tab and comes back (it asks again, so no reconnecting): the picker lists it and the agent keeps it.
      await page.getByRole("button", { name: "Vault", exact: true }).click();
      await page.getByRole("button", { name: "Agentes", exact: true }).click();
      await page.getByRole("button", { name: "Criar agente" }).click();
      const picker = page.locator("label", { hasText: "Modelo" }).locator("select");
      await picker.waitFor();
      assert.deepEqual(await picker.locator("option").allInnerTexts(), ["Padrão do hub", "main"], "the default and what was released, nothing else");
      await page.getByLabel("Nome", { exact: true }).fill("cozinheiro");
      await page.getByLabel("Persona").fill("Você cozinha.");
      await picker.selectOption("main");
      await page.getByRole("button", { name: "Salvar", exact: true }).click();
      await page.getByText("modelo main").waitFor();

      // Editing it again shows the model it has, so saving does not clear it.
      await page.getByRole("button", { name: "Editar" }).click();
      assert.equal(await page.locator("label", { hasText: "Modelo" }).locator("select").inputValue(), "main");
      assert.deepEqual(member.errors, [], "her page raised no error");
    } finally {
      assert.deepEqual(owner.errors, [], "the owner's page raised no error");
      await member?.context.close();
      await owner.context.close();
      await hub.stop();
    }
  });
});
