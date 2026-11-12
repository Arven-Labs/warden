// "Arca mode" in the Sync screen (Warden P183): turning the arca mode on takes two clicks (it cannot be undone), and once it is on
// the screen syncs with the hub picked. Against the pretend backend of `harness.mjs`: the answers are invented here, so what is proved
// is the screen and what it sends; the real work is the business of the Rust tests. Run with `npm run test:e2e` after `npm run build`.

import assert from "node:assert/strict";
import { after, before, describe, test } from "node:test";
import { deferred, launchBrowser, openApp, serveDist, snap } from "./harness.mjs";

const TIMEOUT = 120_000;
// What `arca_sync_run` answers when nothing moved and nothing needs a warning (the shape of `SyncPayload`).
const CLEAN_CYCLE = { snapshotCreated: false, pulled: 0, merges: 0, conflicts: 0, filesChanged: 0, pushed: 0, secretLosersSaved: 0, secretIgnored: 0, secretSkipped: [], recoveredKept: 0, secretFolderMissing: false, vaultEmptied: false, secretEmptied: false };
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

  test("the confirmation has a box for the MCP logins, off by default, and what it says is what the backend gets", { timeout: TIMEOUT }, async () => {
    for (const ticked of [false, true]) {
      let current = { hasKey: true, on: false, pendingSend: false, loginsAccepted: false };
      const { context, page, calls, errors } = await openSync(() => current, {
        arca_mode_enable: () => {
          current = { hasKey: true, on: true, pendingSend: false, loginsAccepted: ticked };
          return { needsReload: false };
        },
      });
      try {
        await page.getByRole("button", { name: "Turn on arca mode", exact: true }).click();
        const box = page.getByRole("checkbox", { name: /Also sync the MCP logins/ });
        assert.equal(await box.isChecked(), false, "the logins are never on by default");
        if (ticked) await box.check();
        await page.getByRole("button", { name: "Yes, turn it on", exact: true }).click();
        await page.getByText("Done: this vault is now synced by the arca.").waitFor();
        assert.deepEqual(asked(calls, "arca_mode_enable").map((c) => c.args.syncLogins), [ticked]);
        assert.deepEqual(errors, [], "the page raised no error");
      } finally {
        await context.close();
      }
    }
  });

  test("a mode that was turned on before the consent says the logins are not syncing, and accepting takes a second click", { timeout: TIMEOUT }, async () => {
    let current = { hasKey: true, on: true, pendingSend: false, hubId: HUB.id, loginsAccepted: false };
    const { context, page, calls, errors } = await openSync(() => current, {
      arca_mode_accept_logins: () => {
        current = { ...current, loginsAccepted: true };
        return null;
      },
    });
    try {
      await page.getByText(/MCP logins are not being synced/).waitFor();
      await page.getByRole("button", { name: "Sync MCP logins", exact: true }).click();
      await page.getByText(/There is no way back in this version/).waitFor();
      assert.equal(asked(calls, "arca_mode_accept_logins").length, 0, "one click does not accept");

      await page.getByRole("button", { name: "Cancel", exact: true }).click();
      assert.equal(asked(calls, "arca_mode_accept_logins").length, 0);
      await page.getByRole("button", { name: "Sync MCP logins", exact: true }).click();
      await page.getByRole("button", { name: "Yes, sync them", exact: true }).click();
      await page.getByText("MCP logins are being synced.").waitFor();
      assert.equal(asked(calls, "arca_mode_accept_logins").length, 1);
      assert.equal(await page.getByRole("button", { name: "Sync MCP logins" }).count(), 0, "nothing left to accept");
      assert.deepEqual(errors, [], "the page raised no error");
    } finally {
      await context.close();
    }
  });

  test("a refused acceptance shows why and leaves the logins not syncing", { timeout: TIMEOUT }, async () => {
    const { context, page, errors } = await openSync(() => ({ hasKey: true, on: true, pendingSend: false, hubId: HUB.id, loginsAccepted: false }), {
      arca_mode_accept_logins: () => Promise.reject("the logins folder /x/mcp_oauth is a link"),
    });
    try {
      await page.getByRole("button", { name: "Sync MCP logins", exact: true }).click();
      await page.getByRole("button", { name: "Yes, sync them", exact: true }).click();
      await page.getByText(/Could not start syncing the logins: the logins folder \/x\/mcp_oauth is a link/).waitFor();
      await page.getByText(/MCP logins are not being synced/).waitFor();
      assert.deepEqual(errors, [], "a refusal is shown, not raised");
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

      running.resolve({ ...CLEAN_CYCLE, snapshotCreated: true, pulled: 4, merges: 1, conflicts: 2, filesChanged: 2, pushed: 5 });
      await page.getByText("Synced: 2 files changed here, 5 objects sent to the hub.").waitFor();
      await page.getByText(/Conflicts settled: 2/).waitFor();
      await snap(page, "desktop-arca-mode-synced", { focus: heading(page), top: true });
      await page.getByRole("button", { name: "Sync now", exact: true }).waitFor();
      assert.deepEqual(errors, [], "the page raised no error");
    } finally {
      running.resolve(CLEAN_CYCLE);
      await context.close();
    }
  });

  test("the logins a cycle left out or ignored are listed under the sentence, with the reason in plain words", { timeout: TIMEOUT }, async () => {
    const { context, page, errors } = await openSync(() => ({ hasKey: true, on: true, pendingSend: false, hubId: HUB.id }), {
      arca_sync_run: () => ({
        snapshotCreated: false,
        pulled: 0,
        merges: 0,
        conflicts: 0,
        filesChanged: 0,
        pushed: 0,
        secretLosersSaved: 1,
        secretIgnored: 2,
        recoveredKept: 3,
        secretSkipped: [
          { path: "notes.txt", reason: "not accepted by this device" },
          { path: "GitHub.json", reason: "differs only by case from another file" },
          { path: "x.json", reason: "something new" },
        ],
      }),
    });
    try {
      await page.getByRole("button", { name: "Sync now", exact: true }).click();
      await page.getByText("Nothing new.").waitFor();
      await page.getByText(/Login files from other devices ignored: 2/).waitFor();
      await page.getByText(/Files left by an interrupted update: 3/).waitFor();
      await page.getByText(/Login conflicts kept as copies: 1/).waitFor();
      await page.getByText("notes.txt: this file is not a login this device syncs").waitFor();
      await page.getByText("GitHub.json: another file differs from it only by letter case").waitFor();
      await page.getByText("x.json: something new").waitFor();
      assert.deepEqual(errors, [], "the page raised no error");
    } finally {
      await context.close();
    }
  });

  test("a logins folder that is gone is warned about, in both languages, and nothing else changes", { timeout: TIMEOUT }, async () => {
    for (const [locale, button, warning] of [
      ["en-US", "Sync now", /The MCP logins folder is missing/],
      ["pt-BR", "Sincronizar agora", /A pasta dos logins MCP não existe/],
    ]) {
      const { context, page, errors } = await openSync(() => ({ hasKey: true, on: true, pendingSend: false, hubId: HUB.id, loginsAccepted: true }), {
        arca_sync_run: () => ({ ...CLEAN_CYCLE, secretFolderMissing: true }),
      }, { locale });
      try {
        await page.getByRole("button", { name: button, exact: true }).click();
        await page.getByText(warning).waitFor();
        assert.deepEqual(errors, [], "the page raised no error");
      } finally {
        await context.close();
      }
    }
  });

  test("an emptied vault or logins folder is warned about, in both languages", { timeout: TIMEOUT }, async () => {
    for (const [locale, button, over, warning] of [
      ["en-US", "Sync now", { vaultEmptied: true }, /Every file of the vault is gone from this computer/],
      ["en-US", "Sync now", { secretEmptied: true }, /The MCP logins folder has no login left/],
      ["pt-BR", "Sincronizar agora", { vaultEmptied: true }, /Todos os arquivos do cofre sumiram deste computador/],
      ["pt-BR", "Sincronizar agora", { secretEmptied: true }, /A pasta dos logins MCP não tem mais nenhum login/],
    ]) {
      const { context, page, errors } = await openSync(() => ({ hasKey: true, on: true, pendingSend: false, hubId: HUB.id, loginsAccepted: true }), {
        arca_sync_run: () => ({ ...CLEAN_CYCLE, ...over }),
      }, { locale });
      try {
        await page.getByRole("button", { name: button, exact: true }).click();
        await page.getByText(warning).waitFor();
        assert.deepEqual(errors, [], "the page raised no error");
      } finally {
        await context.close();
      }
    }
  });

  test("emptying a class on purpose takes a second click, names what it removes, and sends only that class", { timeout: TIMEOUT }, async () => {
    for (const [locale, sync, labels] of [
      ["en-US", "Sync now", { vault: "Remove the vault files on purpose", logins: "Remove the logins on purpose", yes: "Yes, remove them", cancel: "Cancel", others: /on the other devices/ }],
      ["pt-BR", "Sincronizar agora", { vault: "Remover os arquivos do cofre de propósito", logins: "Remover os logins de propósito", yes: "Sim, remover", cancel: "Cancelar", others: /nos outros dispositivos/ }],
    ]) {
      let allowed = false;
      const { context, page, calls, errors } = await openSync(() => ({ hasKey: true, on: true, pendingSend: false, hubId: HUB.id, loginsAccepted: true }), {
        arca_sync_run: () => ({ ...CLEAN_CYCLE, vaultEmptied: true, secretEmptied: true }),
        arca_sync_allow_empty: ({ class: which }) => {
          allowed = true;
          return { ...CLEAN_CYCLE, snapshotCreated: true, pushed: 1, vaultEmptied: which !== "vault", secretEmptied: which !== "secret" };
        },
      }, { locale });
      try {
        await page.getByRole("button", { name: sync, exact: true }).click();
        // Before the cycle says anything is emptied there is nothing to confirm; once it does, one button per class.
        await page.getByRole("button", { name: labels.vault, exact: true }).waitFor();
        await page.getByRole("button", { name: labels.logins, exact: true }).waitFor();

        // The first click only asks, and says it reaches the other devices; nothing is sent.
        await page.getByRole("button", { name: labels.vault, exact: true }).click();
        await page.getByText(labels.others).waitFor();
        assert.equal(asked(calls, "arca_sync_allow_empty").length, 0, "one click does not remove anything");
        await page.getByRole("button", { name: labels.cancel, exact: true }).click();
        await page.getByRole("button", { name: labels.vault, exact: true }).waitFor();
        assert.equal(asked(calls, "arca_sync_allow_empty").length, 0, "cancelling sends nothing");

        // Confirming the vault sends only the vault, with the hub that is chosen; the logins are still warned about and still ask.
        await page.getByRole("button", { name: labels.vault, exact: true }).click();
        await page.getByRole("button", { name: labels.yes, exact: true }).click();
        await page.getByRole("button", { name: labels.logins, exact: true }).waitFor();
        assert.equal(allowed, true);
        assert.deepEqual(asked(calls, "arca_sync_allow_empty").map((c) => c.args), [{ hubId: HUB.id, class: "vault" }]);
        assert.equal(await page.getByRole("button", { name: labels.vault, exact: true }).count(), 0, "the vault was confirmed: no warning and no button");
        assert.deepEqual(errors, [], "the page raised no error");
      } finally {
        await context.close();
      }
    }
  });

  test("a confirmed sync that fails shows the reason, not an error of the page",{ timeout: TIMEOUT }, async () => {
    const { context, page, calls, errors } = await openSync(() => ({ hasKey: true, on: true, pendingSend: false, hubId: HUB.id, loginsAccepted: true }), {
      arca_sync_run: () => ({ ...CLEAN_CYCLE, secretEmptied: true }),
      arca_sync_allow_empty: () => Promise.reject("the hub is not reachable"),
    });
    try {
      await page.getByRole("button", { name: "Sync now", exact: true }).click();
      await page.getByRole("button", { name: "Remove the logins on purpose", exact: true }).click();
      await page.getByRole("button", { name: "Yes, remove them", exact: true }).click();
      await page.getByText(/Sync failed: the hub is not reachable/).waitFor();
      assert.equal(asked(calls, "arca_sync_allow_empty").length, 1);
      assert.deepEqual(errors, [], "a refusal is shown, not raised");
    } finally {
      await context.close();
    }
  });

  test("a cycle with nothing new says so, and a refused one shows the reason", { timeout: TIMEOUT }, async () => {
    // A function, so a refusal is only made when the backend is asked (a rejected promise made earlier, with nobody waiting, is an error).
    let answer = () => CLEAN_CYCLE;
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
