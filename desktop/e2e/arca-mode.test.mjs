// "Arca mode" in the Sync screen (Warden P183): turning the arca mode on takes two clicks (it cannot be undone), and once it is on
// the screen syncs with the hub picked. Against the pretend backend of `harness.mjs`: the answers are invented here, so what is proved
// is the screen and what it sends; the real work is the business of the Rust tests. Run with `npm run test:e2e` after `npm run build`.

import assert from "node:assert/strict";
import { after, before, describe, test } from "node:test";
import { deferred, launchBrowser, openApp, serveDist, snap } from "./harness.mjs";

const TIMEOUT = 120_000;
const HUB = { id: "hub-1", name: "Home hub", url: "https://hub.example.test", open: false };
const WORK = { id: "hub-2", name: "Work hub", url: "https://work.example.test", open: false };

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

/** Opens the Sync screen as a computer whose arca status is `status()` at each ask; `extra` adds or replaces backend answers. */
async function openSync(status, extra = {}, { hubs = [HUB], colorScheme = "light", locale = "en-US" } = {}) {
  const handlers = {
    list_hubs: hubs,
    list_conversations: [],
    list_projects: [],
    shared_conversation_ids: [],
    get_settings: { providers: [], activeProvider: "", combos: [], modelPolicies: [], outreach: [], toolCategories: [], persistGrants: true, grantTtlDays: 0, vaultPath: "", generatedPath: "", tavilyKey: "", whisperKey: "", enableShell: false, defaultModels: {}, mcpServers: [], agents: [], sshHosts: [], gitSync: null, limits: null, defaultLimits: [], limitsDisabledByEnv: false, prices: [], version: "0.0.0" },
    sync_status: { paired: true, deviceId: "d", ownerAddress: "0x0", lastTxId: null, lastSyncedAtMs: null, pendingVaultChanges: 0, pendingConfigChanged: false, pendingOauthChanges: 0, syncignorePatternCount: 0 },
    git_sync_configured: false,
    arca_pair_role: "grant",
    arca_mode_status: () => status(),
    ...extra,
  };
  const opened = await openApp(browser, app, { handlers, colorScheme, locale });
  await opened.page.getByRole("button", { name: "Sync", exact: true }).click();
  await opened.page.getByRole("heading", { name: locale === "pt-BR" ? "Modo arca" : "Arca mode" }).waitFor();
  return opened;
}

const asked = (calls, cmd) => calls.filter((c) => c.cmd === cmd);
const heading = (page, name = "Arca mode") => page.getByRole("heading", { name });

describe("the arca mode of the vault on the desktop", () => {
  test("turning it on takes a second click that says it cannot be undone, and then the screen offers to sync", { timeout: TIMEOUT }, async () => {
    let current = { hasKey: true, on: false, pendingSend: false };
    const { context, page, calls, errors } = await openSync(() => current, {
      arca_mode_enable: () => {
        current = { hasKey: true, on: true, pendingSend: false };
        return { needsReload: false };
      },
    });
    try {
      await page.getByText("It replaces the Arweave and git sync for this vault").waitFor();
      await snap(page, "desktop-arca-mode-ready", { focus: heading(page), top: true });

      // The first click only asks; nothing is sent to the backend.
      await page.getByRole("button", { name: "Turn on arca mode", exact: true }).click();
      await page.getByText("This cannot be undone").waitFor();
      assert.equal(asked(calls, "arca_mode_enable").length, 0, "one click does not turn it on");
      await snap(page, "desktop-arca-mode-confirm", { focus: heading(page), top: true });

      // Cancelling goes back, still off.
      await page.getByRole("button", { name: "Cancel", exact: true }).click();
      await page.getByRole("button", { name: "Turn on arca mode", exact: true }).waitFor();
      assert.equal(asked(calls, "arca_mode_enable").length, 0);

      // Confirming turns it on, and the screen now shows what the backend says.
      await page.getByRole("button", { name: "Turn on arca mode", exact: true }).click();
      await page.getByRole("button", { name: "Yes, turn it on", exact: true }).click();
      await page.getByText("Done: this vault is now synced by the arca.").waitFor();
      assert.equal(asked(calls, "arca_mode_enable").length, 1);
      await page.getByRole("button", { name: "Sync now", exact: true }).waitFor();
      assert.equal(await page.getByRole("button", { name: "Turn on arca mode" }).count(), 0, "there is no way back");
      assert.deepEqual(errors, [], "the page raised no error");
    } finally {
      await context.close();
    }
  });

  test("a computer without the key, or with a Send waiting, is told why and has nothing to turn on", { timeout: TIMEOUT }, async () => {
    for (const [status, text] of [
      [{ hasKey: false, on: false, pendingSend: false }, "This computer has no vault key yet"],
      [{ hasKey: true, on: false, pendingSend: true }, "A Send is waiting for the phone"],
    ]) {
      const { context, page, errors } = await openSync(() => status);
      try {
        await page.getByText(text).first().waitFor();
        assert.equal(await page.getByRole("button", { name: "Turn on arca mode" }).count(), 0);
        assert.deepEqual(errors, [], "the page raised no error");
      } finally {
        await context.close();
      }
    }
  });

  test("when turning it on is refused the reason is shown and nothing changes", { timeout: TIMEOUT }, async () => {
    const { context, page, errors } = await openSync(() => ({ hasKey: true, on: false, pendingSend: false }), {
      arca_mode_enable: () => Promise.reject("the Pull before turning the arca mode on failed, so it was not turned on: no network"),
    });
    try {
      await page.getByRole("button", { name: "Turn on arca mode", exact: true }).click();
      await page.getByRole("button", { name: "Yes, turn it on", exact: true }).click();
      await page.getByText(/Could not turn it on: the Pull before turning/).waitFor();
      await page.getByRole("button", { name: "Turn on arca mode", exact: true }).waitFor();
      assert.deepEqual(errors, [], "a refusal is shown, not raised");
    } finally {
      await context.close();
    }
  });

  test("sync now sends the hub picked, waits without a second click, and says what happened", { timeout: TIMEOUT }, async () => {
    const running = deferred();
    const { context, page, calls, errors } = await openSync(() => ({ hasKey: true, on: true, pendingSend: false, hubId: HUB.id }), { arca_sync_run: () => running.promise });
    try {
      await page.getByRole("button", { name: "Sync now", exact: true }).click();
      await page.getByRole("button", { name: "Syncing…" }).waitFor();
      assert.equal(await page.getByRole("button", { name: "Syncing…" }).isDisabled(), true, "a second click cannot start a second cycle");
      assert.deepEqual(asked(calls, "arca_sync_run").map((c) => c.args.hubId), [HUB.id]);

      running.resolve({ snapshotCreated: true, pulled: 4, merges: 1, conflicts: 2, filesChanged: 2, pushed: 5 });
      await page.getByText("Synced: 2 files changed here, 5 objects sent to the hub.").waitFor();
      await page.getByText(/Conflicts settled: 2/).waitFor();
      await snap(page, "desktop-arca-mode-synced", { focus: heading(page), top: true });
      await page.getByRole("button", { name: "Sync now", exact: true }).waitFor();
      assert.deepEqual(errors, [], "the page raised no error");
    } finally {
      running.resolve({ snapshotCreated: false, pulled: 0, merges: 0, conflicts: 0, filesChanged: 0, pushed: 0 });
      await context.close();
    }
  });

  test("a cycle with nothing new says so, and a refused one shows the reason", { timeout: TIMEOUT }, async () => {
    // A function, so a refusal is only made when the backend is asked (a rejected promise made earlier, with nobody waiting, is an error).
    let answer = () => ({ snapshotCreated: false, pulled: 0, merges: 0, conflicts: 0, filesChanged: 0, pushed: 0 });
    const { context, page, errors } = await openSync(() => ({ hasKey: true, on: true, pendingSend: false, hubId: HUB.id }), { arca_sync_run: () => answer() });
    try {
      await page.getByRole("button", { name: "Sync now", exact: true }).click();
      await page.getByText("Nothing new.").waitFor();

      answer = () => Promise.reject("not signed in to 'Home hub' yet: sign in to this hub first");
      await page.getByRole("button", { name: "Sync now", exact: true }).click();
      await page.getByText(/Sync failed: not signed in to 'Home hub' yet/).waitFor();
      assert.deepEqual(errors, [], "a refusal is shown, not raised");
    } finally {
      await context.close();
    }
  });

  test("with no hub saved it says to save one and the button stays off", { timeout: TIMEOUT }, async () => {
    const { context, page, errors } = await openSync(() => ({ hasKey: true, on: true, pendingSend: false }), {}, { hubs: [] });
    try {
      await page.getByText("Save a hub and sign in to it first").last().waitFor();
      assert.equal(await page.getByRole("button", { name: "Sync now" }).isDisabled(), true);
      assert.deepEqual(errors, [], "the page raised no error");
    } finally {
      await context.close();
    }
  });

  test("with the mode on and no hub chosen the selector starts empty, the sync button stays off, and choosing one records it", { timeout: TIMEOUT }, async () => {
    let current = { hasKey: true, on: true, pendingSend: false, hubId: null };
    const { context, page, calls, errors } = await openSync(() => current, {
      arca_mode_set_hub: ({ hubId }) => {
        current = { ...current, hubId };
        return null;
      },
    }, { hubs: [HUB, WORK] });
    try {
      const select = page.getByLabel("Hub for automatic sync");
      assert.equal(await select.inputValue(), "", "no hub is chosen for the person");
      assert.equal(await page.getByText("Choose a hub").count() > 0, true);
      assert.equal(await page.getByRole("button", { name: "Sync now" }).isDisabled(), true);
      assert.equal(await page.getByText(/Automatic sync uses/).count(), 0);
      await snap(page, "desktop-arca-mode-no-hub", { focus: heading(page), top: true });

      await select.selectOption(WORK.id);
      await page.getByText("Automatic sync uses Work hub.").waitFor();
      assert.deepEqual(asked(calls, "arca_mode_set_hub").map((c) => c.args.hubId), [WORK.id]);
      assert.equal(await select.inputValue(), WORK.id);
      assert.equal(await page.getByRole("button", { name: "Sync now" }).isDisabled(), false);
      assert.deepEqual(errors, [], "the page raised no error");
    } finally {
      await context.close();
    }
  });

  test("a hub already recorded is the one the selector opens on", { timeout: TIMEOUT }, async () => {
    const { context, page, calls, errors } = await openSync(() => ({ hasKey: true, on: true, pendingSend: false, hubId: WORK.id }), {}, { hubs: [HUB, WORK] });
    try {
      await page.getByText("Automatic sync uses Work hub.").waitFor();
      assert.equal(await page.getByLabel("Hub for automatic sync").inputValue(), WORK.id, "not the first of the list");
      assert.equal(asked(calls, "arca_mode_set_hub").length, 0, "opening the screen records nothing");
      assert.deepEqual(errors, [], "the page raised no error");
    } finally {
      await context.close();
    }
  });

  test("a recorded hub that was removed is flagged, the selector is empty, and the file is left alone", { timeout: TIMEOUT }, async () => {
    const { context, page, calls, errors } = await openSync(() => ({ hasKey: true, on: true, pendingSend: false, hubId: "hub-gone" }), {}, { hubs: [HUB, WORK] });
    try {
      await page.getByText(/The hub saved for automatic sync was removed/).waitFor();
      assert.equal(await page.getByLabel("Hub for automatic sync").inputValue(), "");
      assert.equal(await page.getByRole("button", { name: "Sync now" }).isDisabled(), true);
      assert.equal(asked(calls, "arca_mode_set_hub").length, 0, "nothing is written until the person chooses");
      assert.deepEqual(errors, [], "the page raised no error");
    } finally {
      await context.close();
    }
  });

  test("when recording the hub is refused the reason is shown and the selector goes back to what is recorded", { timeout: TIMEOUT }, async () => {
    const { context, page, errors } = await openSync(() => ({ hasKey: true, on: true, pendingSend: false, hubId: HUB.id }), {
      arca_mode_set_hub: () => Promise.reject("there is no saved hub 'hub-2' (it may have been removed)"),
    }, { hubs: [HUB, WORK] });
    try {
      await page.getByLabel("Hub for automatic sync").selectOption(WORK.id);
      await page.getByText(/Could not choose the hub: there is no saved hub/).waitFor();
      assert.equal(await page.getByLabel("Hub for automatic sync").inputValue(), HUB.id);
      await page.getByText("Automatic sync uses Home hub.").waitFor();
      assert.deepEqual(errors, [], "a refusal is shown, not raised");
    } finally {
      await context.close();
    }
  });

  test("in Portuguese the section reads in Portuguese", { timeout: TIMEOUT }, async () => {
    const { context, page, errors } = await openSync(() => ({ hasKey: true, on: false, pendingSend: false }), {}, { locale: "pt-BR" });
    try {
      await page.getByRole("button", { name: "Ligar o modo arca", exact: true }).waitFor();
      await snap(page, "desktop-pt-arca-mode", { focus: heading(page, "Modo arca"), top: true });
      assert.deepEqual(errors, [], "the page raised no error");
    } finally {
      await context.close();
    }
  });
});
