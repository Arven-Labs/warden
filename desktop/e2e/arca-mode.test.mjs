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

  describe("stopping the sync of the MCP logins", () => {
    const status = (accepted) => ({ hasKey: true, on: true, pendingSend: false, hubId: HUB.id, loginsAccepted: accepted });

    test("the button asks first, says what it does not do, and only the second click stops it; then the screen offers to accept again", { timeout: TIMEOUT }, async () => {
      let accepted = true;
      const { context, page, calls, errors } = await openSync(() => status(accepted), {
        arca_mode_revoke_logins: () => {
          accepted = false;
          return null;
        },
      });
      try {
        await page.getByText("MCP logins are being synced.").waitFor();
        await page.getByRole("button", { name: "Stop syncing MCP logins", exact: true }).click();
        await page.getByText(/Nothing is deleted/).waitFor();
        await page.getByText(/already on the hub and on your other devices stays there/).waitFor();
        assert.equal(asked(calls, "arca_mode_revoke_logins").length, 0, "one click does not stop it");

        await page.getByRole("button", { name: "Cancel", exact: true }).click();
        await page.getByRole("button", { name: "Stop syncing MCP logins", exact: true }).waitFor();
        assert.equal(asked(calls, "arca_mode_revoke_logins").length, 0, "cancelling stops nothing");

        await page.getByRole("button", { name: "Stop syncing MCP logins", exact: true }).click();
        await page.getByRole("button", { name: "Yes, stop syncing", exact: true }).click();
        await page.getByText("MCP logins are not being synced. They stay on this computer until you accept.").waitFor();
        await page.getByRole("button", { name: "Sync MCP logins", exact: true }).waitFor();
        assert.equal(asked(calls, "arca_mode_revoke_logins").length, 1);
        assert.equal(await page.getByRole("button", { name: "Stop syncing MCP logins", exact: true }).count(), 0, "once stopped there is nothing to stop");
        assert.deepEqual(errors, [], "the page raised no error");
      } finally {
        await context.close();
      }
    });

    test("with the logins not accepted there is no button to stop them (a guard: it also holds before the change)", { timeout: TIMEOUT }, async () => {
      const { context, page } = await openSync(() => status(false));
      try {
        await page.getByRole("button", { name: "Sync MCP logins", exact: true }).waitFor();
        assert.equal(await page.getByRole("button", { name: "Stop syncing MCP logins", exact: true }).count(), 0);
      } finally {
        await context.close();
      }
    });

    test("a refusal shows the reason, the logins stay accepted on the screen, and the page raises no error", { timeout: TIMEOUT }, async () => {
      const { context, page, errors } = await openSync(() => status(true), { arca_mode_revoke_logins: () => Promise.reject("this vault is not in arca mode") });
      try {
        await page.getByRole("button", { name: "Stop syncing MCP logins", exact: true }).click();
        await page.getByRole("button", { name: "Yes, stop syncing", exact: true }).click();
        await page.getByText(/Could not stop syncing the logins: this vault is not in arca mode/).waitFor();
        await page.getByText("MCP logins are being synced.").waitFor();
        assert.deepEqual(errors, [], "a refusal is shown, not raised");
      } finally {
        await context.close();
      }
    });

    test("the same in Portuguese", { timeout: TIMEOUT }, async () => {
      let accepted = true;
      const { context, page, calls, errors } = await openSync(() => status(accepted), { arca_mode_revoke_logins: () => { accepted = false; return null; } }, { locale: "pt-BR" });
      try {
        await page.getByRole("button", { name: "Parar de sincronizar logins MCP", exact: true }).click();
        await page.getByText(/Nada é apagado/).waitFor();
        await page.getByRole("button", { name: "Sim, parar", exact: true }).click();
        await page.getByRole("button", { name: "Sincronizar logins MCP", exact: true }).waitFor();
        assert.equal(asked(calls, "arca_mode_revoke_logins").length, 1);
        assert.deepEqual(errors, [], "the page raised no error");
      } finally {
        await context.close();
      }
    });
  });

  describe("the copies of the losers of conflicts", () => {
    const ON = () => ({ hasKey: true, on: true, pendingSend: false, hubId: HUB.id, loginsAccepted: true });
    const copy = (path, over = {}) => ({ id: `secret/${path}.conflito-dev-a-0011223344556677`, class: "secret", path, device: "dev-a", shaPrefix: "0011223344556677", edited: false, sizeBytes: 10, modifiedAtMs: Date.UTC(2026, 10, 12, 12, 0, 0), ...over });
    const row = (page, path) => page.getByRole("listitem").filter({ hasText: path });

    async function emit(page, calls, event) {
      let listener;
      for (let i = 0; i < 100 && !listener; i++) {
        listener = calls.find((c) => c.cmd === "plugin:event|listen" && c.args.event === event);
        if (!listener) await page.waitForTimeout(100);
      }
      assert.ok(listener, `the section listens to ${event}`);
      await page.evaluate(({ handler, event }) => window.__TAURI_INTERNALS__.runCallback(handler, { event, id: 1, payload: null }), { handler: listener.args.handler, event });
    }

    test("the copies are listed when the section opens, with name, device and size and never the text, in both languages", { timeout: TIMEOUT }, async () => {
      for (const [locale, title, hint, tail] of [
        ["en-US", "Conflict copies", /Deleting does not overwrite the disk/, /dev-a.*10 B/],
        ["pt-BR", "Cópias de conflito", /Apagar não sobrescreve o disco/, /dev-a.*10 B/],
      ]) {
        const { context, page, errors } = await openSync(ON, { arca_losers_list: () => [copy("github.json"), copy("slack.json")] }, { locale });
        try {
          await page.getByRole("heading", { name: title }).waitFor();
          await page.getByText(hint).waitFor();
          await row(page, "github.json").getByText(tail).waitFor();
          await row(page, "slack.json").waitFor();
          assert.deepEqual(errors, [], "the page raised no error");
        } finally {
          await context.close();
        }
      }
    });

    test("deleting one copy takes a second click that says it cannot be undone, and sends only that copy", { timeout: TIMEOUT }, async () => {
      let items = [copy("github.json"), copy("slack.json")];
      const { context, page, calls, errors } = await openSync(ON, {
        arca_losers_list: () => items,
        arca_losers_clear: ({ ids }) => {
          items = items.filter((i) => !ids.includes(i.id));
          return { removed: ids.length, skipped: [], keptTmp: 0 };
        },
      });
      try {
        await row(page, "slack.json").getByRole("button", { name: "Delete this copy", exact: true }).click();
        await page.getByText(/will be deleted and cannot be recovered from here/).waitFor();
        assert.equal(asked(calls, "arca_losers_clear").length, 0, "one click does not delete anything");

        await page.getByRole("button", { name: "Cancel", exact: true }).click();
        assert.equal(asked(calls, "arca_losers_clear").length, 0, "cancelling deletes nothing");

        await row(page, "slack.json").getByRole("button", { name: "Delete this copy", exact: true }).click();
        await page.getByRole("button", { name: "Yes, delete", exact: true }).click();
        await page.getByText("Deleted: 1.").waitFor();
        assert.deepEqual(asked(calls, "arca_losers_clear").map((c) => c.args), [{ ids: [copy("slack.json").id] }]);
        await row(page, "slack.json").waitFor({ state: "detached" });
        await row(page, "github.json").waitFor();
        assert.deepEqual(errors, [], "the page raised no error");
      } finally {
        await context.close();
      }
    });

    test("'delete all' asks first and sends every copy but the edited one", { timeout: TIMEOUT }, async () => {
      let items = [copy("github.json"), copy("notion.json", { edited: true }), copy("slack.json")];
      const { context, page, calls, errors } = await openSync(ON, {
        arca_losers_list: () => items,
        arca_losers_clear: ({ ids }) => {
          items = items.filter((i) => !ids.includes(i.id));
          return { removed: ids.length, skipped: [], keptTmp: 0 };
        },
      });
      try {
        await page.getByRole("button", { name: "Delete all copies", exact: true }).click();
        await page.getByText(/Every copy you did not edit will be deleted/).waitFor();
        assert.equal(asked(calls, "arca_losers_clear").length, 0, "one click does not delete anything");

        await page.getByRole("button", { name: "Yes, delete all", exact: true }).click();
        await page.getByText("Deleted: 2.").waitFor();
        assert.deepEqual(asked(calls, "arca_losers_clear").map((c) => c.args), [{ ids: [copy("github.json").id, copy("slack.json").id] }]);
        await row(page, "notion.json").waitFor();
        assert.deepEqual(errors, [], "the page raised no error");
      } finally {
        await context.close();
      }
    });

    test("a copy the person edited is marked, has no delete button, and a list of only edited ones offers no 'delete all'", { timeout: TIMEOUT }, async () => {
      const { context, page, errors } = await openSync(ON, { arca_losers_list: () => [copy("github.json", { edited: true })] });
      try {
        await row(page, "github.json").getByText("You edited this copy: it is kept").waitFor();
        assert.equal(await page.getByRole("button", { name: "Delete this copy", exact: true }).count(), 0);
        assert.equal(await page.getByRole("button", { name: "Delete all copies", exact: true }).count(), 0);
        assert.deepEqual(errors, [], "the page raised no error");
      } finally {
        await context.close();
      }
    });

    test("what stayed after deleting is told with its reason, and a work file that may hold a login is warned about", { timeout: TIMEOUT }, async () => {
      const { context, page, errors } = await openSync(ON, {
        arca_losers_list: () => [copy("github.json"), copy("slack.json")],
        arca_losers_clear: () => ({ removed: 1, skipped: [{ id: copy("slack.json").id, reason: "edited" }], keptTmp: 1 }),
      });
      try {
        await page.getByRole("button", { name: "Delete all copies", exact: true }).click();
        await page.getByRole("button", { name: "Yes, delete all", exact: true }).click();
        await page.getByText("Deleted: 1.").waitFor();
        await page.getByText(/slack\.json.*you edited this copy, so it was kept/).waitFor();
        await page.getByText(/A temporary file that may still hold a login was left in the conflicts folder/).waitFor();
        assert.deepEqual(errors, [], "the page raised no error");
      } finally {
        await context.close();
      }
    });

    test("a refused deletion shows the reason, in Portuguese too, and is not an error of the page", { timeout: TIMEOUT }, async () => {
      const { context, page, errors } = await openSync(ON, {
        arca_losers_list: () => [copy("github.json")],
        arca_losers_clear: () => Promise.reject("this vault is not in arca mode"),
      }, { locale: "pt-BR" });
      try {
        await row(page, "github.json").getByRole("button", { name: "Apagar esta cópia", exact: true }).click();
        await page.getByRole("button", { name: "Sim, apagar", exact: true }).click();
        await page.getByText(/Não foi possível apagar as cópias: this vault is not in arca mode/).waitFor();
        assert.deepEqual(errors, [], "a refusal is shown, not raised");
      } finally {
        await context.close();
      }
    });

    test("the list follows the loop: a copy saved by an automatic cycle shows up by itself", { timeout: TIMEOUT }, async () => {
      let items = [];
      const { context, page, calls, errors } = await openSync(ON, { arca_losers_list: () => items });
      try {
        await page.getByRole("button", { name: "Sync now", exact: true }).waitFor();
        assert.equal(await page.getByRole("heading", { name: "Conflict copies" }).count(), 0, "nothing to list: no section");

        items = [copy("github.json")];
        await emit(page, calls, "arca-loop-report");
        await row(page, "github.json").waitFor();
        assert.deepEqual(errors, [], "the page raised no error");
      } finally {
        await context.close();
      }
    });

    test("with the mode off there is no section of copies (a guard: it also holds before the change)", { timeout: TIMEOUT }, async () => {
      const off = await openSync(() => ({ hasKey: true, on: false, pendingSend: false }), { arca_losers_list: () => [copy("github.json")] });
      try {
        await off.page.getByRole("button", { name: "Turn on arca mode", exact: true }).waitFor();
        assert.equal(await off.page.getByRole("heading", { name: "Conflict copies" }).count(), 0);
      } finally {
        await off.context.close();
      }
    });
  });

  describe("what the automatic cycle says without a click", () => {
    const ON = () => ({ hasKey: true, on: true, pendingSend: false, hubId: HUB.id, loginsAccepted: true });
    const loopOf = (over, atMs = Date.now() + 60_000) => ({ atMs, report: CLEAN_CYCLE, error: null, stopped: false, ...over });

    /** Sends the event the backend emits after each round of the automatic loop (it carries nothing: the screen asks again). */
    async function loopEvent(page, calls) {
      let listener;
      for (let i = 0; i < 100 && !listener; i++) {
        listener = calls.find((c) => c.cmd === "plugin:event|listen" && c.args.event === "arca-loop-report");
        if (!listener) await page.waitForTimeout(100);
      }
      assert.ok(listener, "the section listens to arca-loop-report");
      await page.evaluate(({ handler }) => window.__TAURI_INTERNALS__.runCallback(handler, { event: "arca-loop-report", id: 1, payload: null }), { handler: listener.args.handler });
    }

    test("a warning the automatic cycle left is there when the section opens, in both languages, with no click", { timeout: TIMEOUT }, async () => {
      for (const [locale, warning] of [
        ["en-US", /The MCP logins folder is missing/],
        ["pt-BR", /A pasta dos logins MCP não existe/],
      ]) {
        const { context, page, errors } = await openSync(ON, { arca_loop_last: () => loopOf({ report: { ...CLEAN_CYCLE, secretFolderMissing: true } }) }, { locale });
        try {
          await page.getByText(warning).waitFor();
          assert.deepEqual(errors, [], "the page raised no error");
        } finally {
          await context.close();
        }
      }
    });

    test("a warning that arrives while the section is open appears by itself, and a clean cycle after it takes it away", { timeout: TIMEOUT }, async () => {
      let current = null;
      const { context, page, calls, errors } = await openSync(ON, { arca_loop_last: () => current });
      try {
        await page.getByRole("button", { name: "Sync now", exact: true }).waitFor();
        assert.equal(await page.getByText(/Every file of the vault is gone/).count(), 0);

        current = loopOf({ report: { ...CLEAN_CYCLE, vaultEmptied: true } });
        await loopEvent(page, calls);
        await page.getByText(/Every file of the vault is gone from this computer/).waitFor();
        // The same button as after "Sync now": it asks first and sends nothing.
        await page.getByRole("button", { name: "Remove the vault files on purpose", exact: true }).waitFor();

        current = loopOf({}, Date.now() + 120_000);
        await loopEvent(page, calls);
        await page.getByText(/Every file of the vault is gone/).waitFor({ state: "detached" });
        assert.deepEqual(errors, [], "the page raised no error");
      } finally {
        await context.close();
      }
    });

    test("a loop the hub stopped says so, with how to start it again, and a hub that was only down says it failed", { timeout: TIMEOUT }, async () => {
      let current = loopOf({ report: null, error: { kind: "authRejected", message: "401 from the hub" }, stopped: true });
      const { context, page, calls, errors } = await openSync(ON, { arca_loop_last: () => current });
      try {
        await page.getByText(/Automatic sync stopped: the hub does not accept this device\. Sign in to it again and press Sync now\. Detail: 401 from the hub/).waitFor();

        current = loopOf({ report: null, error: { kind: "other", message: "connection refused" } });
        await loopEvent(page, calls);
        await page.getByText("The last automatic sync failed: connection refused").waitFor();
        assert.equal(await page.getByText(/Automatic sync stopped/).count(), 0, "a hub that is down is not a stop");
        assert.deepEqual(errors, [], "the page raised no error");
      } finally {
        await context.close();
      }
    });

    test("the stop is told in Portuguese too", { timeout: TIMEOUT }, async () => {
      const { context, page, errors } = await openSync(ON, { arca_loop_last: () => loopOf({ report: null, error: { kind: "authRejected", message: "401" }, stopped: true }) }, { locale: "pt-BR" });
      try {
        await page.getByText(/Sync automático parado: o hub não aceita este dispositivo\. Entre de novo no hub e use Sincronizar agora\. Detalhe: 401/).waitFor();
        assert.deepEqual(errors, [], "the page raised no error");
      } finally {
        await context.close();
      }
    });

    test("a manual sync made after the automatic cycle is not overwritten by that older cycle", { timeout: TIMEOUT }, async () => {
      // The automatic cycle ran a minute ago and warned; the person synced by hand afterwards and nothing is wrong any more.
      const oldWarning = loopOf({ report: { ...CLEAN_CYCLE, secretFolderMissing: true } }, Date.now() - 60_000);
      const { context, page, calls, errors } = await openSync(ON, { arca_loop_last: () => oldWarning, arca_sync_run: () => CLEAN_CYCLE });
      try {
        await page.getByText(/The MCP logins folder is missing/).waitFor();
        await page.getByRole("button", { name: "Sync now", exact: true }).click();
        await page.getByText("Nothing new.").waitFor();
        await page.getByText(/The MCP logins folder is missing/).waitFor({ state: "detached" });

        // The loop reports again, still with that same old cycle: it must not bring the warning back over the newer manual result.
        await loopEvent(page, calls);
        await page.waitForTimeout(500);
        assert.equal(await page.getByText(/The MCP logins folder is missing/).count(), 0);
        assert.deepEqual(errors, [], "the page raised no error");
      } finally {
        await context.close();
      }
    });

    test("a section whose mode is off shows nothing of the automatic cycle (a guard: it also holds before the change)", { timeout: TIMEOUT }, async () => {
      const { context, page } = await openSync(() => ({ hasKey: true, on: false, pendingSend: false }), { arca_loop_last: () => loopOf({ report: { ...CLEAN_CYCLE, vaultEmptied: true } }) });
      try {
        await page.getByRole("button", { name: "Turn on arca mode", exact: true }).waitFor();
        assert.equal(await page.getByText(/Every file of the vault is gone/).count(), 0, "nothing automatic runs before the mode is on");
      } finally {
        await context.close();
      }
    });
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
