// A tour of the main screens of the web page against a real hub with invented data, taking a picture of each (see `snap` in
// `harness.mjs`): the CI uploads them and someone looks at every one. The assertions are loose on purpose, they only prove the
// screen got where the picture is taken; what is wrong on the screen is for the eye. Run with `npm run test:e2e`.

import assert from "node:assert/strict";
import { after, before, describe, test } from "node:test";
import { launchBrowser, signIn, snap, startHub } from "./harness.mjs";

const TIMEOUT = 120_000;
const HOUR = 3_600_000;

let browser;
before(async () => {
  browser = await launchBrowser();
});
after(async () => {
  await browser?.close();
});

const spend = (hoursAgo, fields) => JSON.stringify({ kind: "spend", ts: Date.now() - hoursAgo * HOUR, channel: "server", user: null, agent: null, person: null, ...fields });

const LEDGER =
  [
    spend(1, { agent: "writer", model: "m", tokens: 1_000_000, cost_usd: 2.0, provider: "main" }),
    spend(2, { model: "m2", tokens: 500_000, cost_usd: 0.5, person: "ana", provider: "spare" }),
  ].join("\n") + "\n";

/** A package an author could have made, with a link in it, so the preview has something to warn about. Its hash is not
 * right on purpose: the preview of a package that was changed on the way is a screen worth seeing too. */
const PACKAGE_WITH_A_LINK = JSON.stringify({
  format: "warden-package",
  formatVersion: 1,
  kind: "skill",
  id: "tidy-notes",
  version: "1.0.0",
  author: "Ana",
  license: "CC0-1.0",
  content: { name: "tidy-notes", description: "Tidy up rough notes into short bullet points.", body: "Rewrite the notes as short bullets. See https://example.test/style for the style." },
  hash: "sha256:0000",
});

const FILES = {
  "warden/spend_ledger.jsonl": LEDGER,
  "vault/notes/weekly-plan.md": "# Weekly plan\n\n- Monday: review the budget\n- Wednesday: call the supplier\n- Friday: close the month\n",
  "vault/notes/ideas.md": "# Ideas\n\nA short list of things to try next quarter.\n",
  "vault/projects/home-reform/PROJECT.md": "---\nname: Home reform\ndescription: Receipts and plans of the reform\n---\nKeep the receipts of the reform in one place.\n",
};

for (const colorScheme of ["light", "dark"]) {
  describe(`the main screens, ${colorScheme}`, () => {
    test("each one is photographed", { timeout: TIMEOUT }, async () => {
      const hub = await startHub({ files: FILES });
      let context;
      try {
        const opened = await signIn(browser, hub, { colorScheme });
        context = opened.context;
        const { page } = opened;
        const tab = (name) => page.getByRole("navigation", { name: "Seções" }).getByRole("button", { name });

        await page.getByRole("navigation", { name: "Seções" }).waitFor();
        await snap(page, `chat-${colorScheme}`);

        await tab("Vault").click();
        await page.getByRole("button", { name: /weekly-plan/ }).waitFor();
        await snap(page, `vault-list-${colorScheme}`);
        await page.getByRole("button", { name: /weekly-plan/ }).click();
        await page.getByText("call the supplier").first().waitFor();
        await snap(page, `vault-note-${colorScheme}`);

        await tab("Uso").click();
        await page.getByText(/Gasto recente/).waitFor();
        await snap(page, `usage-${colorScheme}`);

        await tab("Skills").click();
        await page.getByRole("button", { name: "Importar" }).waitFor();
        await page.getByText("Nenhuma skill ainda.").waitFor();
        await snap(page, `skills-${colorScheme}`);
        await snap(page, `skills-gallery-${colorScheme}`, { focus: page.getByRole("heading", { name: "Galeria" }) });

        await page.getByRole("button", { name: "Importar" }).click();
        await page.getByPlaceholder(/Cole o conteúdo/).fill(PACKAGE_WITH_A_LINK);
        await snap(page, `skills-import-paste-${colorScheme}`);
        await page.getByRole("button", { name: "Ler" }).click();
        // The package was changed on the way (its hash is wrong): the screen says so instead of a preview.
        await page.locator(".error-banner").first().waitFor();
        await snap(page, `skills-import-refused-${colorScheme}`);

        await tab("Projetos").click();
        await page.getByText(/Home reform/).first().waitFor();
        await snap(page, `projects-${colorScheme}`);

        assert.deepEqual(opened.errors, [], "the page raised no error");
      } finally {
        await context?.close();
        await hub.stop();
      }
    });
  });
}
