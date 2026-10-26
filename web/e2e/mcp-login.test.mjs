// The hub signs in to an OAuth MCP server (P131), from the web page: a real hub, and a pretend OAuth server (`fake-oauth.mjs`) it does
// discovery, registration and the code exchange with, for real. The part a person does in another browser tab (approve, then copy the
// address the page ends on) is done here by `addressTheBrowserEndsAt`. Run with `npm run test:e2e` after `npm run build` and
// `cargo build -p warden-server --bin warden-server`.

import assert from "node:assert/strict";
import { after, before, describe, test } from "node:test";
import { addressTheBrowserEndsAt, startFakeOAuth } from "./fake-oauth.mjs";
import { launchBrowser, openSettings, PAIRING_KEY, saveWithKey, snap, startHub } from "./harness.mjs";

const TIMEOUT = 120_000;

let browser;
before(async () => {
  browser = await launchBrowser();
});
after(async () => {
  await browser?.close();
});

/** A hub that already has the OAuth server saved, and the settings page open on it. */
async function withLogin(body) {
  const oauth = await startFakeOAuth();
  const hub = await startHub({
    allowMachineSettings: true,
    extraConfig: `
[[mcp_servers]]
name = "sentry"
url = "${oauth.mcpUrl}"
oauth = true
`,
  });
  let context;
  try {
    const opened = await openSettings(browser, hub);
    context = opened.context;
    // The providers and agents are `settings-card`s too: the MCP server's is the one with an address.
    const card = opened.page.locator("li.settings-card").filter({ hasText: "Endereço" });
    await body({ hub, oauth, card, ...opened });
    assert.deepEqual(opened.errors, [], "the page raised no error");
  } finally {
    await context?.close();
    await hub.stop();
    await oauth.stop();
  }
}

/** "Entrar" → the pairing key → "Confirmar". Leaves the page on the box that takes the address. */
async function startSignIn(page, card, key = PAIRING_KEY) {
  await card.getByRole("button", { name: "Entrar", exact: true }).click();
  await page.getByLabel("Chave de pareamento do hub").fill(key);
  await page.getByRole("button", { name: "Confirmar" }).click();
}

const frames = (sent, type) => sent.filter((s) => s.includes(`"${type}"`)).map((s) => JSON.parse(s));

describe("the web page signs the hub in to an OAuth MCP server", () => {
  test("a saved server starts without a login, and a wrong pairing key is refused", { timeout: TIMEOUT }, async () => {
    await withLogin(async ({ page, card, sent }) => {
      await card.getByText("Ainda sem login.").waitFor();
      await snap(page, "web-mcp-login-signed-out", { focus: card });
      await startSignIn(page, card, "not-the-key");
      await page.getByText("Chave de pareamento errada.").waitFor();
      assert.equal(await page.getByLabel("O endereço em que o navegador terminou").count(), 0, "no sign-in opened with a wrong key");
      assert.equal(frames(sent, "completeMcpOauth").length, 0);
    });
  });

  test("signs in end to end, then signs out", { timeout: TIMEOUT }, async () => {
    await withLogin(async ({ page, card, sent, oauth }) => {
      await card.getByText("Ainda sem login.").waitFor();
      await startSignIn(page, card);

      const link = card.getByRole("link", { name: "Abrir a página de login" });
      await link.waitFor();
      const authUrl = await link.getAttribute("href");
      assert.ok(authUrl.startsWith(oauth.url), "the page to open belongs to the OAuth server the hub talked to");
      await snap(page, "web-mcp-login-flow", { focus: card });

      const address = await addressTheBrowserEndsAt(authUrl);
      await page.getByLabel("O endereço em que o navegador terminou").fill(address);
      await card.getByRole("button", { name: "Concluir" }).click();
      await card.getByText("Conectado. O hub está conectando o servidor.").waitFor({ timeout: 60_000 });
      await card.getByText("Conectado: o login está guardado no hub.").waitFor();
      assert.equal(oauth.state.tokens, 1, "the hub exchanged the code for a token");
      assert.equal(frames(sent, "completeMcpOauth")[0].callbackUrl, address);
      await snap(page, "web-mcp-login-connected", { focus: card });

      await card.getByRole("button", { name: "Sair", exact: true }).click();
      await page.getByLabel("Chave de pareamento do hub").fill(PAIRING_KEY);
      await page.getByRole("button", { name: "Confirmar" }).click();
      await card.getByText("Desconectado: o login foi apagado do hub.").waitFor({ timeout: 60_000 });
      await card.getByText("Ainda sem login.").waitFor();
      assert.equal(frames(sent, "forgetMcpOauth").length, 1);
    });
  });

  test("an address from the wrong place, or without the code, says so and lets the right one finish", { timeout: TIMEOUT }, async () => {
    await withLogin(async ({ page, card, sent, oauth }) => {
      await startSignIn(page, card);
      const link = card.getByRole("link", { name: "Abrir a página de login" });
      await link.waitFor();
      const address = await addressTheBrowserEndsAt(await link.getAttribute("href"));
      const field = page.getByLabel("O endereço em que o navegador terminou");
      const finish = card.getByRole("button", { name: "Concluir" });

      await field.fill("https://example.test/callback?code=x&state=y");
      await finish.click();
      await card.getByText(/Essa não é a página em que o login termina/).waitFor();

      await field.fill(address.split("?")[0]);
      await finish.click();
      await card.getByText(/O endereço não tem o código/).waitFor();

      await field.fill(`${address.split("?")[0]}?error=access_denied&state=y`);
      await finish.click();
      await card.getByText(/O serviço recusou o login/).waitFor();
      assert.equal(frames(sent, "completeMcpOauth").length, 0, "none of the three reached the hub");

      await field.fill(address);
      await finish.click();
      await card.getByText("Conectado. O hub está conectando o servidor.").waitFor({ timeout: 60_000 });
      assert.equal(oauth.state.tokens, 1);
    });
  });
});

describe("the Sentry entry of the gallery", () => {
  test("is added as an OAuth server with no header, and asks to be saved before the sign-in", { timeout: TIMEOUT }, async () => {
    const oauth = await startFakeOAuth();
    const hub = await startHub({ allowMachineSettings: true });
    let context;
    try {
      const opened = await openSettings(browser, hub);
      context = opened.context;
      const { page, sent } = opened;
      const entry = page.locator("li.skills-item", { hasText: "Sentry (hospedado — OAuth)" });
      await snap(page, "web-mcp-gallery-sentry", { focus: entry });
      await entry.getByRole("button", { name: "Adicionar" }).click();
      await entry.getByText("Já está na lista").waitFor();

      const card = page.locator("li.settings-card").filter({ hasText: "Endereço" });
      const address = card.locator("input").nth(1);
      assert.equal(await address.inputValue(), "https://mcp.sentry.dev/mcp", "the gallery's address is the one Sentry publishes");
      await card.getByText(/Salve-o primeiro/).waitFor();
      await snap(page, "web-mcp-gallery-sentry-added", { focus: card });

      // Saving would make the hub reach for the real sentry.dev: the address is pointed at the pretend server first.
      await address.fill(oauth.mcpUrl);
      await page.getByRole("button", { name: "Salvar" }).click();
      const dialog = page.getByRole("alertdialog");
      await dialog.waitFor();
      await dialog.getByRole("checkbox").check();
      await dialog.getByRole("button", { name: "Continuar" }).click();
      await saveWithKey(page);
      await page.getByText(/✓ Salvo/).waitFor({ timeout: 60_000 });
      const saved = JSON.parse(sent.filter((s) => s.includes('"saveSettings"')).at(-1));
      const server = saved.update.machine.mcpServers.find((s) => s.name === "sentry");
      assert.equal(server.oauth, true);
      assert.equal(server.url, oauth.mcpUrl);
      assert.equal((server.headers ?? []).length, 0, "no Authorization header goes with an OAuth server");
      await card.getByText("Ainda sem login.").waitFor({ timeout: 60_000 });
      assert.deepEqual(opened.errors, []);
    } finally {
      await context?.close();
      await hub.stop();
      await oauth.stop();
    }
  });
});
