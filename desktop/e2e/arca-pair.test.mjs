// "Pair a device with arca" in the Sync screen (P182 item 2): the person types both codes, and the field refuses to be filled by a
// paste. Against the pretend backend of `harness.mjs`: the events and the answers are invented here, so what is proved is the screen
// and what it sends back; the conversation itself is the business of the Rust tests. Run with `npm run test:e2e` after `npm run build`.

import assert from "node:assert/strict";
import { after, before, describe, test } from "node:test";
import { deferred, launchBrowser, openApp, serveDist } from "./harness.mjs";

const TIMEOUT = 120_000;
const HUB = { id: "hub-1", name: "Home hub", url: "https://hub.example.test", open: false };

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

/** Opens the Sync screen as a computer that `role` ("grant" has the key, "offer" has none); `run` is the pairing, settled by the test. */
async function openSync(role, run) {
  const handlers = {
    list_hubs: [HUB],
    list_conversations: [],
    list_projects: [],
    shared_conversation_ids: [],
    get_settings: { providers: [], activeProvider: "", combos: [], modelPolicies: [], outreach: [], toolCategories: [], persistGrants: true, grantTtlDays: 0, vaultPath: "", generatedPath: "", tavilyKey: "", whisperKey: "", enableShell: false, defaultModels: {}, mcpServers: [], agents: [], sshHosts: [], gitSync: null, limits: null, defaultLimits: [], limitsDisabledByEnv: false, prices: [], version: "0.0.0" },
    sync_status: { paired: true, deviceId: "d", ownerAddress: "0x0", lastTxId: null, lastSyncedAtMs: null, pendingVaultChanges: 0, pendingConfigChanged: false, pendingOauthChanges: 0, syncignorePatternCount: 0 },
    git_sync_configured: false,
    arca_pair_role: role,
    arca_pair_run: () => run.promise,
  };
  const opened = await openApp(browser, app, { handlers });
  await opened.page.getByRole("button", { name: "Sync", exact: true }).click();
  await opened.page.getByRole("heading", { name: "Pair a device with arca" }).waitFor();
  return opened;
}

/** What the backend sends the page: the `arca-pair-event` the page listens for. */
async function send(opened, payload) {
  const listener = opened.calls.find((c) => c.cmd === "plugin:event|listen" && c.args.event === "arca-pair-event");
  assert.ok(listener, "the screen listens for the pairing events");
  await opened.page.evaluate(
    ({ handler, payload }) => window.__TAURI_INTERNALS__.runCallback(handler, { event: "arca-pair-event", id: 1, payload }),
    { handler: listener.args.handler, payload },
  );
}

describe("pairing a device with arca on the desktop", () => {
  test("the side to play is the one the computer can play, and the codes are typed, never pasted", { timeout: TIMEOUT }, async () => {
    const run = deferred();
    const opened = await openSync("grant", run);
    const { context, page, calls, errors } = opened;
    try {
      await page.getByText("This computer has the vault key").waitFor();
      await page.getByRole("button", { name: "Start", exact: true }).click();
      assert.ok(calls.some((c) => c.cmd === "arca_pair_run" && c.args.hubId === HUB.id), "it starts the pairing with the hub picked");

      await send(opened, { kind: "choose", devices: ["dev-b"] });
      await page.getByRole("button", { name: "dev-b", exact: true }).click();
      assert.deepEqual(calls.findLast((c) => c.cmd === "arca_pair_answer").args.reply, { kind: "picked", index: 0 });

      await send(opened, { kind: "ask", ask: "codeShownByTheNewDevice" });
      const field = page.getByLabel("Code the new device shows");
      await field.waitFor();
      assert.equal(await field.inputValue(), "", "a question opens an empty field, nothing to confirm by clicking");

      // A real paste (the code on the clipboard, Ctrl+V) does not fill the field...
      await context.grantPermissions(["clipboard-read", "clipboard-write"]);
      await page.evaluate(() => navigator.clipboard.writeText("7f3a-91c2-0b4d-e8a5"));
      await field.focus();
      await page.keyboard.press("ControlOrMeta+V");
      assert.equal(await field.inputValue(), "", "a pasted code is not taken");

      // ...and typing does.
      await field.pressSequentially("7f3a-91c2-0b4d-e8a5");
      assert.equal(await field.inputValue(), "7f3a-91c2-0b4d-e8a5");
      await page.getByRole("button", { name: "Confirm", exact: true }).click();
      assert.deepEqual(calls.findLast((c) => c.cmd === "arca_pair_answer").args.reply, { kind: "typed", text: "7f3a-91c2-0b4d-e8a5" });
      assert.equal(await field.inputValue(), "", "the field is emptied once sent");

      await send(opened, { kind: "show", what: "keyFingerprint", value: "aaaa-bbbb-cccc-dddd" });
      await page.getByText("aaaa-bbbb-cccc-dddd").waitFor();
      assert.equal(await page.locator("#arca-pair-typed").count(), 0, "a computed value is never in a field");

      run.resolve("done");
      await page.getByText("Key handed over. The new device still has to check the fingerprint.").waitFor();
      assert.deepEqual(errors, [], "the page raised no error");
    } finally {
      await context.close();
    }
  });

  test("a computer with no key shows its code as text and asks for the fingerprint in an empty field", { timeout: TIMEOUT }, async () => {
    const run = deferred();
    const opened = await openSync("offer", run);
    const { context, page, errors } = opened;
    try {
      await page.getByText("This computer has no vault key yet").waitFor();
      await page.getByRole("button", { name: "Start", exact: true }).click();

      await send(opened, { kind: "show", what: "offerCode", value: "7f3a-91c2-0b4d-e8a5" });
      await page.getByText("7f3a-91c2-0b4d-e8a5").waitFor();
      assert.equal(await page.locator("#arca-pair-typed").count(), 0, "its own code is not in a field");

      await send(opened, { kind: "ask", ask: "fingerprintShownByTheOldDevice" });
      const field = page.getByLabel("Fingerprint the old device shows");
      await field.waitFor();
      assert.equal(await field.inputValue(), "");
      assert.equal(await page.getByText("7f3a-91c2-0b4d-e8a5").count(), 0, "the question replaces what was shown");

      run.resolve("done");
      await page.getByText("Key stored on this device.").waitFor();
      assert.deepEqual(errors, [], "the page raised no error");
    } finally {
      await context.close();
    }
  });

  test("giving up reaches the backend and says so, and an error from the backend is shown", { timeout: TIMEOUT }, async () => {
    const run = deferred();
    const opened = await openSync("offer", run);
    const { context, page, calls, errors } = opened;
    try {
      await page.getByRole("button", { name: "Start", exact: true }).click();
      await page.getByRole("button", { name: "Cancel", exact: true }).click();
      assert.ok(calls.some((c) => c.cmd === "arca_pair_cancel"), "cancel reaches the backend");
      run.resolve("cancelled");
      await page.getByText("Cancelled.").waitFor();
      assert.deepEqual(errors, [], "the page raised no error");
    } finally {
      await context.close();
    }

    const failing = deferred();
    const second = await openSync("offer", failing);
    try {
      await second.page.getByRole("button", { name: "Start", exact: true }).click();
      failing.reject("not signed in to 'Home hub' yet: sign in to this hub first");
      await second.page.getByText("Pairing failed: not signed in to 'Home hub' yet: sign in to this hub first").waitFor();
      assert.deepEqual(second.errors, [], "the page raised no error");
    } finally {
      await second.context.close();
    }
  });
});
