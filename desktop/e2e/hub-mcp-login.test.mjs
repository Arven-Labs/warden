// "Hub sign-ins" in Settings (P131): the desktop signs the hub in to its OAuth MCP servers by catching the browser's return itself.
// Against the pretend backend of `harness.mjs`: the hub's replies and the three `mcp_login_*` commands are invented here, so what is
// proved is the screen and what it asks for (the pairing key dialog, the port handed to the hub, the address handed back).
// What the listener and the hub do is the business of their own Rust tests. Run with `npm run test:e2e` after `npm run build`.

import assert from "node:assert/strict";
import { after, before, describe, test } from "node:test";
import { launchBrowser, openApp, serveDist, snap } from "./harness.mjs";

const TIMEOUT = 120_000;
const HUB = { id: "hub-office", name: "Office hub", url: "wss://hub.example.test", open: false };
const PORT = 49152;
const PREFIX = `http://127.0.0.1:${PORT}/callback`;
const OWNER = { state: "connected", user: null };
const MEMBER = { state: "connected", user: { id: "u-ana", name: "Ana", role: "member", mustChangePassword: false } };

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

/** The hub's answers, by the type of the message the page sends. `signedIn` is the list the hub keeps, changed by complete and forget. */
function hubReplies(signedIn) {
  return (message) => {
    switch (message.type) {
      case "listConversations":
        return { type: "conversationList", conversations: [] };
      case "listProjects":
        return { type: "projectList", projects: [] };
      case "requestSettings":
        return { type: "settings", settings: { agents: [], machine: { mcpServers: [{ name: "sentry", oauth: true }, { name: "notes" }] } } };
      case "listMcpLogins":
        return { type: "mcpLogins", names: [...signedIn] };
      case "beginMcpOauth":
        return { type: "mcpOauthStarted", flowId: "flow-1", authUrl: "https://auth.example.test/authorize?state=s1", redirectPrefix: PREFIX };
      case "completeMcpOauth":
        signedIn.add("sentry");
        return { type: "mcpOauthDone", name: "sentry" };
      case "forgetMcpOauth":
        signedIn.delete("sentry");
        return { type: "mcpLogins", names: [...signedIn] };
      default:
        return { type: "ok" };
    }
  };
}

/** Opens the app with the saved hub in use and connected as `state`, then goes to Settings. */
async function openHubSettings({ state = OWNER, signedIn = new Set(), query = "code=abc&state=s1" } = {}) {
  const handlers = {
    list_hubs: [HUB],
    get_settings: { providers: [], activeProvider: "", combos: [], modelPolicies: [], outreach: [], toolCategories: [], persistGrants: true, grantTtlDays: 0, vaultPath: "", generatedPath: "", tavilyKey: "", whisperKey: "", enableShell: false, defaultModels: {}, mcpServers: [], agents: [], sshHosts: [], gitSync: null, limits: null, defaultLimits: [], limitsDisabledByEnv: false, prices: [], version: "0.0.0" },
    remote_request: ({ message }) => hubReplies(signedIn)(message),
    mcp_login_listen: PORT,
    mcp_login_wait: query,
  };
  const opened = await openApp(browser, app, { handlers, colorScheme: "dark" });
  const { page, calls } = opened;
  await page.locator("#hub-switcher-select").selectOption(HUB.id);
  // The connection state reaches the page as the `remote-hub-state` event; the pretend backend has to send it.
  await page.waitForFunction(() => document.querySelector("#hub-switcher-select")?.value === "hub-office");
  const listener = calls.find((c) => c.cmd === "plugin:event|listen" && c.args.event === "remote-hub-state");
  assert.ok(listener, "the app listens for the state of the hub");
  await page.evaluate(
    ({ handler, payload }) => window.__TAURI_INTERNALS__.runCallback(handler, { event: "remote-hub-state", id: 1, payload }),
    { handler: listener.args.handler, payload: { hubId: HUB.id, state } },
  );
  await page.getByRole("button", { name: "Settings", exact: true }).click();
  return { ...opened, signedIn };
}

// The MCP gallery above has a `.skill-card` that mentions Sentry too, so the card is found by its name, not by a word in its text.
const card = (page) => page.locator(".skill-card").filter({ has: page.locator(".skill-card-name", { hasText: /^sentry$/ }) });
const section = (page) => page.locator("section.settings-section").filter({ has: page.getByRole("heading", { name: "Hub sign-ins" }) });
const remoteMessages = (calls, type) => calls.filter((c) => c.cmd === "remote_request" && c.args.message.type === type).map((c) => c.args.message);

describe("the desktop signs the hub in to an OAuth MCP server", () => {
  test("lists the OAuth servers of the hub, signed out, and leaves the others out", { timeout: TIMEOUT }, async () => {
    const { context, page, errors } = await openHubSettings();
    try {
      await page.getByRole("heading", { name: "Hub sign-ins" }).waitFor();
      await card(page).waitFor();
      assert.ok(await card(page).getByText("Not signed in").isVisible());
      assert.equal(await page.locator(".skill-card", { hasText: "notes" }).count(), 0, "a server that does not sign in with OAuth is not listed");
      await snap(page, "desktop-hub-signins", { focus: page.getByRole("heading", { name: "Hub sign-ins" }), top: true });
      assert.deepEqual(errors, []);
    } finally {
      await context.close();
    }
  });

  test("signs in: the pairing key, the listener's port to the hub, the browser's return back to it", { timeout: TIMEOUT }, async () => {
    const { context, page, calls, errors } = await openHubSettings();
    try {
      await card(page).getByRole("button", { name: "Sign in", exact: true }).click();
      const dialog = page.getByRole("dialog");
      await dialog.waitFor();
      await dialog.locator("input[type=password]").fill("PAIRING_KEY");
      await dialog.getByRole("button", { name: "Continue" }).click();
      await card(page).getByText("Signed in", { exact: true }).waitFor();
      await page.getByText("Signed in. The hub is connecting the server.").waitFor();

      const [begin] = remoteMessages(calls, "beginMcpOauth");
      assert.equal(begin.name, "sentry");
      assert.equal(begin.redirectPort, PORT, "the hub is told the port of this computer's listener");
      assert.equal(begin.pairingKey, "PAIRING_KEY");
      const [complete] = remoteMessages(calls, "completeMcpOauth");
      assert.equal(complete.flowId, "flow-1");
      assert.equal(complete.callbackUrl, `${PREFIX}?code=abc&state=s1`, "the hub gets the address the browser ended at, put back together");
      assert.ok(calls.some((c) => c.cmd === "plugin:opener|open_url"), "the sign-in page is opened in the browser");
      assert.ok(calls.some((c) => c.cmd === "mcp_login_cancel"), "the listener is let go when it ends");
      await snap(page, "desktop-hub-signins-done", { focus: page.getByRole("heading", { name: "Hub sign-ins" }), top: true });
      assert.deepEqual(errors, []);
    } finally {
      await context.close();
    }
  });

  test("a browser that comes back refused does not reach the hub", { timeout: TIMEOUT }, async () => {
    const { context, page, calls, errors } = await openHubSettings({ query: "error=access_denied&state=s1" });
    try {
      await card(page).getByRole("button", { name: "Sign in", exact: true }).click();
      const dialog = page.getByRole("dialog");
      await dialog.locator("input[type=password]").fill("PAIRING_KEY");
      await dialog.getByRole("button", { name: "Continue" }).click();
      await page.getByText("The service refused the sign-in. Try again and approve it.").waitFor();
      assert.equal(remoteMessages(calls, "completeMcpOauth").length, 0);
      assert.ok(await card(page).getByText("Not signed in").isVisible());
      await snap(page, "desktop-hub-signins-refused", { focus: page.getByRole("heading", { name: "Hub sign-ins" }), top: true });
      assert.deepEqual(errors, []);
    } finally {
      await context.close();
    }
  });

  test("closing the pairing key dialog starts nothing", { timeout: TIMEOUT }, async () => {
    const { context, page, calls, errors } = await openHubSettings();
    try {
      await card(page).getByRole("button", { name: "Sign in", exact: true }).click();
      const dialog = page.getByRole("dialog");
      await dialog.waitFor();
      await dialog.getByRole("button", { name: "Cancel" }).click();
      await card(page).getByRole("button", { name: "Sign in", exact: true }).waitFor();
      assert.equal(remoteMessages(calls, "beginMcpOauth").length, 0);
      assert.equal(await section(page).locator(".settings-error-banner").count(), 0, "cancelling is not an error");
      assert.deepEqual(errors, []);
    } finally {
      await context.close();
    }
  });

  test("signs out: the hub forgets the login", { timeout: TIMEOUT }, async () => {
    const { context, page, calls, errors } = await openHubSettings({ signedIn: new Set(["sentry"]) });
    try {
      await card(page).getByText("Signed in", { exact: true }).waitFor();
      await card(page).getByRole("button", { name: "Sign out" }).click();
      const dialog = page.getByRole("dialog");
      await dialog.locator("input[type=password]").fill("PAIRING_KEY");
      await dialog.getByRole("button", { name: "Continue" }).click();
      await card(page).getByText("Not signed in").waitFor();
      await page.getByText("Signed out: the login was deleted from the hub.").waitFor();
      const [forget] = remoteMessages(calls, "forgetMcpOauth");
      assert.equal(forget.name, "sentry");
      assert.equal(forget.pairingKey, "PAIRING_KEY");
      assert.deepEqual(errors, []);
    } finally {
      await context.close();
    }
  });

  test("a member of the hub has no such section", { timeout: TIMEOUT }, async () => {
    const { context, page, calls, errors } = await openHubSettings({ state: MEMBER });
    try {
      await page.getByRole("heading", { name: "Settings", exact: true }).waitFor();
      assert.equal(await page.getByRole("heading", { name: "Hub sign-ins" }).count(), 0);
      assert.equal(remoteMessages(calls, "listMcpLogins").length, 0, "nothing is asked of the hub");
      assert.deepEqual(errors, []);
    } finally {
      await context.close();
    }
  });
});
