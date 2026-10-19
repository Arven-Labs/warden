// P136 end to end: the page speaks English unless the browser asks for Portuguese or the person picks it, and the pick is kept. Against a
// real hub. Also the pictures of the Portuguese screens (the ones of `screens.test.mjs` are English), for someone to look at: a phrase
// that is longer in Portuguese is what breaks a layout. Run with `npm run test:e2e` (see `harness.mjs`).

import assert from "node:assert/strict";
import { after, before, describe, test } from "node:test";
import { PAIRING_KEY, launchBrowser, pageOptions, signIn, snap, startHub } from "./harness.mjs";

const TIMEOUT = 120_000;

let browser;
before(async () => {
  browser = await launchBrowser();
});
after(async () => {
  await browser?.close();
});

const tabs = (page, language) => page.getByRole("navigation", { name: language === "pt-BR" ? "Seções" : "Sections" });

describe("the language of the page", () => {
  test("a browser in English gets English, from the sign-in to the tabs", { timeout: TIMEOUT }, async () => {
    const hub = await startHub();
    let context;
    try {
      const opened = await signIn(browser, hub, { locale: "en-US" });
      context = opened.context;
      const { page } = opened;
      await tabs(page, "en-US").waitFor();
      for (const name of ["Chat", "Agents", "Vault", "Usage", "Skills", "Projects", "Organization", "Settings"]) {
        await tabs(page, "en-US").getByRole("button", { name, exact: true }).waitFor();
      }
      assert.equal(await page.evaluate(() => document.documentElement.lang), "en");
      assert.deepEqual(opened.errors, [], "the page raised no error");
    } finally {
      await context?.close();
      await hub.stop();
    }
  });

  test("a browser in Portuguese gets Portuguese without being asked", { timeout: TIMEOUT }, async () => {
    const hub = await startHub();
    let context;
    try {
      const opened = await signIn(browser, hub, { locale: "pt-BR" });
      context = opened.context;
      const { page } = opened;
      await tabs(page, "pt-BR").waitFor();
      for (const name of ["Chat", "Agentes", "Cofre", "Uso", "Skills", "Projetos", "Organização", "Configurações"]) {
        await tabs(page, "pt-BR").getByRole("button", { name, exact: true }).waitFor();
      }
      assert.equal(await page.evaluate(() => document.documentElement.lang), "pt-BR");
    } finally {
      await context?.close();
      await hub.stop();
    }
  });

  test("the person picks a language on the page, it changes at once and it is kept after a reload", { timeout: TIMEOUT }, async () => {
    const hub = await startHub();
    let context;
    try {
      const opened = await signIn(browser, hub, { locale: "en-US" });
      context = opened.context;
      const { page } = opened;
      await tabs(page, "en-US").waitFor();

      await page.getByLabel("Language").selectOption("pt-BR");
      await tabs(page, "pt-BR").getByRole("button", { name: "Cofre", exact: true }).waitFor();
      assert.equal(await page.evaluate(() => document.documentElement.lang), "pt-BR");

      await page.reload();
      await tabs(page, "pt-BR").getByRole("button", { name: "Cofre", exact: true }).waitFor();
      assert.equal(await page.getByLabel("Idioma").inputValue(), "pt-BR");

      await page.getByLabel("Idioma").selectOption("en");
      await tabs(page, "en-US").getByRole("button", { name: "Vault", exact: true }).waitFor();
      assert.deepEqual(opened.errors, [], "the page raised no error");
    } finally {
      await context?.close();
      await hub.stop();
    }
  });

  test("the sign-in screen has the picker too, before anybody is signed in", { timeout: TIMEOUT }, async () => {
    const hub = await startHub();
    const context = await browser.newContext({ ...pageOptions, locale: "en-US" });
    try {
      const page = await context.newPage();
      await page.goto(hub.url);
      await page.getByRole("tab", { name: "Pairing key" }).waitFor();
      await page.getByLabel("Language").selectOption("pt-BR");
      await page.getByRole("tab", { name: "Chave de pareamento" }).waitFor();
      await page.getByLabel("Chave de pareamento").first().fill(PAIRING_KEY);
      await page.getByRole("button", { name: "Entrar" }).click();
      await tabs(page, "pt-BR").waitFor();
    } finally {
      await context.close();
      await hub.stop();
    }
  });
});

describe("the Portuguese screens", () => {
  test("each one is photographed", { timeout: TIMEOUT }, async () => {
    const hub = await startHub({ allowMachineSettings: true });
    let context;
    try {
      const opened = await signIn(browser, hub, { locale: "pt-BR" });
      context = opened.context;
      const { page } = opened;
      const tab = (name) => tabs(page, "pt-BR").getByRole("button", { name, exact: true });
      await tabs(page, "pt-BR").waitFor();
      await snap(page, "pt-chat");
      await tab("Cofre").click();
      await snap(page, "pt-vault");
      await tab("Uso").click();
      await page.getByRole("heading", { name: "Limites de gasto" }).waitFor();
      await snap(page, "pt-usage");
      await tab("Skills").click();
      await page.getByText("Nenhuma skill ainda.").waitFor();
      await snap(page, "pt-skills");
      await tab("Pessoas").click();
      await page.getByRole("heading", { name: "Pessoas" }).waitFor();
      await snap(page, "pt-people");
      await tab("Configurações").click();
      await page.getByText("Máquina do hub").first().waitFor();
      await snap(page, "pt-settings");
      assert.deepEqual(opened.errors, [], "the page raised no error");
    } finally {
      await context?.close();
      await hub.stop();
    }
  });
});
