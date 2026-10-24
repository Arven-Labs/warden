// The theme of the app (P137): dark until the person picks light in the Settings, kept in this webview, and the shield is the mark.
// Against a pretend backend (see `harness.mjs`). Run with `npm run test:e2e` after `npm run build`.

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
  list_conversations: [],
  list_projects: [],
  get_settings: SETTINGS,
  list_hubs: [],
  shared_conversation_ids: [],
  list_skills: [],
  list_skill_catalog: [],
});

const background = (page) => page.evaluate(() => getComputedStyle(document.body).backgroundColor);

describe("the theme", () => {
  test("opens dark even when the system is light, switches to light in Settings and keeps the choice", { timeout: TIMEOUT }, async () => {
    // A system in the light theme and no saved choice.
    const { context, page, errors } = await openApp(browser, app, { handlers: backend(), colorScheme: "light", theme: null, locale: "en-US" });
    try {
      await page.getByRole("button", { name: "New chat" }).first().waitFor();
      assert.equal(await background(page), "rgb(7, 6, 11)");
      await snap(page, "theme-dark");

      await page.getByRole("button", { name: "Settings", exact: true }).click();
      await page.getByRole("button", { name: "Switch to the light theme" }).click();
      assert.equal(await background(page), "rgb(251, 249, 255)");
      await snap(page, "theme-settings-light");

      await page.reload();
      await page.getByRole("button", { name: "Settings", exact: true }).waitFor();
      assert.equal(await background(page), "rgb(251, 249, 255)", "the choice survives a reload");

      await page.getByRole("button", { name: "Settings", exact: true }).click();
      await page.getByRole("button", { name: "Switch to the dark theme" }).click();
      assert.equal(await background(page), "rgb(7, 6, 11)");
      assert.deepEqual(errors, [], "the page raised no error");
    } finally {
      await context.close();
    }
  });
});
