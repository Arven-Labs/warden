// P136 end to end: the app speaks English unless the system asks for Portuguese or the person picks it, and the pick is kept. Also the
// pictures of the Portuguese screens (the ones of `screens.test.mjs` are English), for someone to look at: a phrase that is longer in
// Portuguese is what breaks a layout. Against a pretend backend (see `harness.mjs`). Run with `npm run test:e2e` after `npm run build`.

import assert from "node:assert/strict";
import { after, before, describe, test } from "node:test";
import { launchBrowser, openApp, serveDist, snap } from "./harness.mjs";

const TIMEOUT = 120_000;

let browser;
let app;
before(async () => {
  browser = await launchBrowser();
  app = await serveDist();
});
after(async () => {
  await browser?.close();
  await app?.stop();
});

const NOW = Date.now();

const SETTINGS = {
  providers: [],
  activeProvider: "",
  combos: [],
  modelPolicies: [],
  outreach: [],
  toolCategories: [],
  persistGrants: true,
  grantTtlDays: 0,
  vaultPath: "",
  generatedPath: "",
  tavilyKey: "",
  whisperKey: "",
  enableShell: false,
  defaultModels: {},
  mcpServers: [],
  agents: [],
  sshHosts: [],
  gitSync: null,
  limits: null,
  defaultLimits: [],
  limitsDisabledByEnv: false,
  prices: [],
  version: "0.0.0",
};

const backend = () => ({
  list_conversations: [
    {
      id: "weekly",
      title: "Weekly review",
      messages: [{ id: "m1", role: "user", content: "What should I focus on this week?", createdAt: NOW - 60_000 }],
      createdAt: NOW - 60_000,
      updatedAt: NOW - 60_000,
    },
  ],
  list_projects: [],
  get_settings: SETTINGS,
  list_hubs: [],
  shared_conversation_ids: [],
  list_skills: [],
  list_skill_catalog: [],
  sync_status: {
    paired: true,
    deviceId: "device-laptop",
    ownerAddress: "0x00000000000000000000000000000000000000aa",
    lastTxId: null,
    lastSyncedAtMs: NOW - 3_600_000,
    pendingVaultChanges: 0,
    pendingConfigChanged: false,
    pendingOauthChanges: 0,
    syncignorePatternCount: 0,
  },
  git_sync_configured: false,
});

describe("the language of the app", () => {
  test("a system in English gets English", { timeout: TIMEOUT }, async () => {
    const { context, page, errors } = await openApp(browser, app, { handlers: backend(), locale: "en-US" });
    try {
      await page.getByRole("button", { name: "New chat" }).first().waitFor();
      for (const name of ["Usage", "Sync", "Vault", "Skills", "Projects", "Settings"]) {
        await page.getByRole("button", { name, exact: true }).waitFor();
      }
      assert.equal(await page.evaluate(() => document.documentElement.lang), "en");
      assert.deepEqual(errors, [], "the page raised no error");
    } finally {
      await context.close();
    }
  });

  test("a system in Portuguese gets Portuguese without being asked", { timeout: TIMEOUT }, async () => {
    const { context, page, errors } = await openApp(browser, app, { handlers: backend(), locale: "pt-BR" });
    try {
      await page.getByRole("button", { name: "Nova conversa" }).first().waitFor();
      for (const name of ["Uso", "Sync", "Cofre", "Skills", "Projetos", "Configurações"]) {
        await page.getByRole("button", { name, exact: true }).waitFor();
      }
      assert.equal(await page.evaluate(() => document.documentElement.lang), "pt-BR");
      assert.deepEqual(errors, [], "the page raised no error");
    } finally {
      await context.close();
    }
  });

  test("the person picks a language in Settings, it changes at once and it is kept after a reload", { timeout: TIMEOUT }, async () => {
    const { context, page, errors } = await openApp(browser, app, { handlers: backend(), locale: "en-US" });
    try {
      await page.getByRole("button", { name: "Settings", exact: true }).click();
      await page.getByLabel("Language").selectOption("pt-BR");
      await page.getByRole("button", { name: "Cofre", exact: true }).waitFor();
      assert.equal(await page.evaluate(() => document.documentElement.lang), "pt-BR");

      await page.reload();
      await page.getByRole("button", { name: "Cofre", exact: true }).waitFor();
      await page.getByRole("button", { name: "Configurações", exact: true }).click();
      assert.equal(await page.getByLabel("Idioma").inputValue(), "pt-BR");

      await page.getByLabel("Idioma").selectOption("en");
      await page.getByRole("button", { name: "Vault", exact: true }).waitFor();
      assert.deepEqual(errors, [], "the page raised no error");
    } finally {
      await context.close();
    }
  });
});

describe("the Portuguese screens", () => {
  test("each one is photographed", { timeout: TIMEOUT }, async () => {
    const { context, page, errors } = await openApp(browser, app, { handlers: backend(), locale: "pt-BR" });
    try {
      const open = (name) => page.getByRole("button", { name, exact: true }).click();
      await page.getByText("Weekly review").first().waitFor();
      await snap(page, "desktop-pt-chat");
      await open("Cofre");
      await page.getByRole("heading", { name: "Cofre" }).first().waitFor();
      await snap(page, "desktop-pt-vault");
      await open("Uso");
      await page.getByRole("heading", { name: "Limites de gasto" }).waitFor();
      await snap(page, "desktop-pt-usage");
      await open("Skills");
      await page.getByText("Nenhuma skill ainda.").waitFor();
      await snap(page, "desktop-pt-skills");
      await open("Sync");
      await page.getByRole("heading", { name: "Situação" }).waitFor();
      await snap(page, "desktop-pt-sync");
      await open("Configurações");
      await page.getByRole("heading", { name: "Provedores de modelo" }).waitFor();
      await snap(page, "desktop-pt-settings");
      assert.deepEqual(errors, [], "the page raised no error");
    } finally {
      await context.close();
    }
  });
});
