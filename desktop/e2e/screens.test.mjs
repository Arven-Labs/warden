// The screens only the desktop has (publishing a skill, the gallery, the QR of the sync) plus the chat, photographed in light and dark
// against a pretend backend with invented data (see `harness.mjs`). The photos are what this is for: the CI uploads them and someone
// looks at each one (`project/AUTOMACAO.md`). The assertions are on the text of the page and on what it asked the backend for.
// What the Rust behind each command does is the business of `src-tauri`'s own tests.
// Run with `npm run test:e2e` after `npm run build`.

import assert from "node:assert/strict";
import fs from "node:fs";
import { after, before, describe, test } from "node:test";
import { deferred, launchBrowser, openApp, serveDist, snap } from "./harness.mjs";

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
const message = (id, role, content, createdAt) => ({ id, role, content, createdAt });

const CONVERSATIONS = [
  {
    id: "weekly",
    title: "Weekly review",
    messages: [
      message("m1", "user", "What should I focus on this week?", NOW - 60_000),
      message("m2", "assistant", "Three things: close the month, call the supplier and send the budget. The rest can wait.", NOW - 30_000),
    ],
    createdAt: NOW - 60_000,
    updatedAt: NOW - 30_000,
  },
  {
    id: "trip",
    title: "Trip to Lisbon",
    messages: [message("t1", "user", "Plan three days in Lisbon.", NOW - 86_400_000)],
    createdAt: NOW - 86_400_000,
    updatedAt: NOW - 86_400_000,
  },
];

const EMPTY_SETTINGS = {
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

/** A QR-looking picture, so the photo shows where it goes and how big it is. Not a real code. */
function fakeQr() {
  const cells = [];
  for (let y = 0; y < 25; y++) {
    for (let x = 0; x < 25; x++) {
      const finder = (x < 7 && y < 7) || (x > 17 && y < 7) || (x < 7 && y > 17);
      if (finder ? x % 6 === 0 || y % 6 === 0 || (x % 6 > 1 && x % 6 < 5 && y % 6 > 1 && y % 6 < 5) : (x * 7 + y * 13 + x * y) % 3 === 0) {
        cells.push(`<rect x="${x}" y="${y}" width="1" height="1"/>`);
      }
    }
  }
  return `<svg xmlns="http://www.w3.org/2000/svg" viewBox="-1 -1 27 27" width="220" height="220"><rect x="-1" y="-1" width="27" height="27" fill="#fff"/>${cells.join("")}</svg>`;
}

const SKILLS = [
  { name: "tidy-notes", description: "Tidy up rough notes into short bullet points.", body: "Rewrite the notes as short bullets.", agents: [] },
  { name: "weekly-plan", description: "Turn a list of tasks into a plan for the week.", body: "Group the tasks by day.", agents: ["writer"] },
];

const CATALOG = [
  { name: "meeting-summary", description: "Turn meeting notes into decisions and next steps.", body: "List the decisions first.", status: "installed" },
  { name: "email-draft", description: "Draft a short, polite email from a few lines of context.", body: "Keep it under 120 words.", status: "notInstalled" },
  { name: "commit-message", description: "Write a commit message from a diff.", body: "One line, then a short body.", status: "modified" },
];

/** What the screens ask for when the page opens, answered with the invented data above. */
function backend(extra = {}) {
  return {
    list_conversations: CONVERSATIONS,
    list_projects: [{ id: "home-reform", name: "Home reform", description: "Receipts and plans of the reform", instructions: "Keep the receipts in one place." }],
    get_settings: EMPTY_SETTINGS,
    list_hubs: [],
    shared_conversation_ids: [],
    list_skills: SKILLS,
    list_skill_catalog: CATALOG,
    sync_status: {
      paired: true,
      deviceId: "device-laptop",
      ownerAddress: "0x00000000000000000000000000000000000000aa",
      lastTxId: "tx-last-sync",
      lastSyncedAtMs: NOW - 3_600_000,
      pendingVaultChanges: 3,
      pendingConfigChanged: true,
      pendingOauthChanges: 0,
      syncignorePatternCount: 2,
    },
    git_sync_configured: false,
    ...extra,
  };
}

for (const colorScheme of ["light", "dark"]) {
  describe(`the desktop screens, ${colorScheme}`, () => {
    test("the chat lists the conversations and opens one", { timeout: TIMEOUT }, async () => {
      const { context, page, errors } = await openApp(browser, app, { handlers: backend(), colorScheme });
      try {
        await page.getByText("Weekly review").first().waitFor();
        await snap(page, `desktop-chat-${colorScheme}`);
        await page.getByText("Weekly review").first().click();
        await page.getByText("Three things: close the month").waitFor();
        await snap(page, `desktop-chat-open-${colorScheme}`);
        assert.deepEqual(errors, [], "the page raised no error");
      } finally {
        await context.close();
      }
    });

    test("skills: the list, the gallery and publishing one by QR", { timeout: TIMEOUT }, async () => {
      const waiting = deferred();
      const handlers = backend({
        export_skill_package: { content: JSON.stringify({ format: "warden-package", kind: "skill", id: "tidy-notes" }), attachmentsSkipped: false },
        preview_skill_package: { id: "tidy-notes", version: "1.0.0", license: "CC0-1.0", name: "tidy-notes", description: "Tidy up rough notes.", body: "Rewrite the notes as short bullets.", hash: "sha256:00", status: "installed" },
        skill_publish_begin: { qrSvg: fakeQr(), hash: "sha256:00" },
        skill_publish_await: () => waiting.promise,
      });
      const { context, page, calls, errors } = await openApp(browser, app, { handlers, colorScheme });
      try {
        await page.getByRole("button", { name: "Skills", exact: true }).click();
        await page.getByRole("heading", { name: "Gallery" }).waitFor();
        await snap(page, `desktop-skills-${colorScheme}`);
        await snap(page, `desktop-skills-gallery-${colorScheme}`, { focus: page.getByRole("heading", { name: "Gallery" }), top: true });
        assert.ok(await page.getByText("Installed", { exact: true }).count() > 0, "the gallery says which one is installed");

        const card = page.locator(".skill-card", { hasText: "tidy-notes" }).first();
        await card.getByRole("button", { name: "Publish", exact: true }).click();
        await page.getByText("Publish tidy-notes").waitFor();
        const show = page.getByRole("button", { name: "Show the QR code" });
        assert.equal(await show.isDisabled(), true, "no QR before the person says they understand it is permanent");
        await snap(page, `desktop-publish-form-${colorScheme}`, { focus: page.getByText("Publish tidy-notes"), top: true });

        await page.getByLabel("I understand it will be public and permanent").check();
        assert.equal(await show.isDisabled(), false);
        await show.click();
        await page.getByText("Scan this code with the TruthID app").waitFor();
        assert.equal(await page.locator(".sync-qr-image svg").count(), 1, "the QR from the backend is on the screen");
        await snap(page, `desktop-publish-qr-${colorScheme}`, { focus: page.getByText("Publish tidy-notes"), top: true });
        assert.ok(calls.some((c) => c.cmd === "skill_publish_begin"), "the page asked the backend to begin the publication");

        waiting.resolve({ txId: "invented-tx-id-0000000000000000000000000000000" });
        await page.getByText("Published. Its address is").waitFor();
        assert.match(await page.locator("code", { hasText: "ar://" }).innerText(), /^ar:\/\/invented-tx-id/);
        await snap(page, `desktop-publish-done-${colorScheme}`, { focus: page.getByText("Publish tidy-notes"), top: true });
        assert.deepEqual(errors, [], "the page raised no error");
      } finally {
        waiting.resolve({ txId: "closed" });
        await context.close();
      }
    });

    test("skills: a source lists its packages, a taken-back one cannot be installed, and one opens the import preview", { timeout: TIMEOUT }, async () => {
      const key = "ab".repeat(32);
      const entry = { id: "tidy-notes", version: "1.0.0", hash: "sha256:11", source: "https://example.org/tidy.json", description: "Tidy up rough notes.", license: "CC0-1.0", revoked: false };
      const handlers = backend({
        list_curators: [{ name: "Ana", key, url: "https://example.org/index.json" }],
        fetch_curator_index: {
          curator: "Ana",
          name: "Ana's picks",
          sequence: 3,
          entries: [entry, { ...entry, id: "old-notes", hash: "sha256:22", description: "An old one.", revoked: true, revokedReason: "it asked for a password" }],
        },
        fetch_curator_package: JSON.stringify({ format: "warden-package", kind: "skill", id: "tidy-notes" }),
        preview_skill_package: { id: "tidy-notes", version: "1.0.0", license: "CC0-1.0", name: "tidy-notes", description: "Tidy up rough notes.", body: "Rewrite the notes as short bullets.", hash: "sha256:11", status: "notInstalled" },
      });
      const { context, page, calls, errors } = await openApp(browser, app, { handlers, colorScheme });
      try {
        await page.getByRole("button", { name: "Skills", exact: true }).click();
        await page.getByRole("heading", { name: "Sources" }).waitFor();
        await page.getByRole("button", { name: "See what it lists" }).click();
        await page.getByText("Ana's picks, number 3").waitFor();
        await page.getByText("Taken back by the source: it asked for a password").waitFor();
        await snap(page, `desktop-sources-${colorScheme}`, { focus: page.getByRole("heading", { name: "Sources" }), top: true });

        const install = page.getByRole("button", { name: "Read and install" });
        assert.equal(await install.nth(1).isDisabled(), true, "what the curator took back cannot be fetched");
        await install.first().click();
        await page.getByText("Rewrite the notes as short bullets.").first().waitFor();
        const fetched = calls.find((c) => c.cmd === "fetch_curator_package");
        assert.deepEqual(fetched?.args, { curator: "Ana", hash: "sha256:11" }, "the page asked for the package by its hash in the index");
        assert.ok(calls.some((c) => c.cmd === "preview_skill_package"), "the text went to the same preview as a file");
        assert.deepEqual(errors, [], "the page raised no error");
      } finally {
        await context.close();
      }
    });

    test("skills: a package for a skill the person has says so in the list and shows what changes in the preview", { timeout: TIMEOUT }, async () => {
      const key = "ab".repeat(32);
      const entry = { id: "tidy-notes", version: "1.1.0", hash: "sha256:33", source: "https://example.org/tidy.json", description: "Tidy up rough notes.", license: "CC0-1.0", revoked: false };
      const handlers = backend({
        list_curators: [{ name: "Ana", key, url: "https://example.org/index.json" }],
        fetch_curator_index: { curator: "Ana", name: "Ana's picks", sequence: 4, entries: [entry, { ...entry, id: "brand-new", hash: "sha256:44" }] },
        fetch_curator_package: JSON.stringify({ format: "warden-package", kind: "skill", id: "tidy-notes" }),
        preview_skill_package: {
          id: "tidy-notes", version: "1.1.0", license: "CC0-1.0", name: "tidy-notes", description: "Tidy up rough notes.",
          body: "Rewrite the notes as bullets.", hash: "sha256:33", status: "modified",
        },
      });
      const { context, page, errors } = await openApp(browser, app, { handlers, colorScheme });
      try {
        await page.getByRole("button", { name: "Skills", exact: true }).click();
        await page.getByRole("button", { name: "See what it lists" }).click();
        await page.getByText("Ana's picks, number 4").waitFor();
        const hints = page.getByText("You already have a skill with this name.", { exact: false });
        assert.equal(await hints.count(), 1, "only the entry named like a skill of theirs says so");

        await page.getByRole("button", { name: "Read and install" }).first().click();
        const change = page.locator(".skill-editor", { has: page.getByText("What this changes in your skill") });
        await change.waitFor();
        await change.getByText("The description changes", { exact: false }).waitFor();
        await change.locator(".skill-diff-line--del", { hasText: "Rewrite the notes as short bullets." }).waitFor();
        await change.locator(".skill-diff-line--add", { hasText: "Rewrite the notes as bullets." }).waitFor();
        await snap(page, `desktop-sources-update-diff-${colorScheme}`, { focus: change, top: false });
        assert.deepEqual(errors, [], "the page raised no error");
      } finally {
        await context.close();
      }
    });

    test("skills: an entry that updates a skill installed from that source says there is a new version and offers to show it", { timeout: TIMEOUT }, async () => {
      const key = "ab".repeat(32);
      const entry = { id: "tidy-notes", version: "1.1.0", hash: "sha256:33", source: "https://example.org/tidy.json", description: "Tidy up rough notes.", license: "CC0-1.0", revoked: false };
      const handlers = backend({
        list_curators: [{ name: "Ana", key, url: "https://example.org/index.json" }],
        // `tidy-notes` is a skill the person has, and the backend marks this entry as the update of what they installed from Ana.
        fetch_curator_index: { curator: "Ana", name: "Ana's picks", sequence: 4, entries: [{ ...entry, update: true }, { ...entry, id: "brand-new", hash: "sha256:44" }] },
        fetch_curator_package: JSON.stringify({ format: "warden-package", kind: "skill", id: "tidy-notes" }),
        preview_skill_package: {
          id: "tidy-notes", version: "1.1.0", license: "CC0-1.0", name: "tidy-notes", description: "Tidy up rough notes.",
          body: "Rewrite the notes as bullets.", hash: "sha256:33", status: "modified",
        },
      });
      const { context, page, errors } = await openApp(browser, app, { handlers, colorScheme });
      try {
        await page.getByRole("button", { name: "Skills", exact: true }).click();
        await page.getByRole("button", { name: "See what it lists" }).click();
        await page.getByText("Ana's picks, number 4").waitFor();
        const notice = page.getByText("There is a new version of a skill you installed from this source.", { exact: false });
        await notice.first().waitFor();
        assert.equal(await notice.count(), 1, "only the entry the backend marked says so");
        assert.equal(await page.getByText("You already have a skill with this name.", { exact: false }).count(), 0, "the new notice replaces the plain one");
        assert.equal(await page.getByRole("button", { name: "Read and install" }).count(), 1, "the other entry keeps its button");
        await snap(page, `desktop-sources-update-label-${colorScheme}`, { focus: page.getByRole("heading", { name: "Sources" }), top: true });

        await page.getByRole("button", { name: "See the update" }).click();
        const change = page.locator(".skill-editor", { has: page.getByText("What this changes in your skill") });
        await change.waitFor();
        assert.deepEqual(errors, [], "the page raised no error");
      } finally {
        await context.close();
      }
    });

    test("skills: the button for the first source fills it in, and saving sends it", { timeout: TIMEOUT }, async () => {
      const published = fs.readFileSync(new URL("../../community/PUBLIC_KEY", import.meta.url), "utf8").trim();
      const handlers = backend({ list_curators: [], save_curators: (args) => args.curators });
      const { context, page, calls, errors } = await openApp(browser, app, { handlers, colorScheme });
      try {
        await page.getByRole("button", { name: "Skills", exact: true }).click();
        await page.getByRole("heading", { name: "Sources" }).waitFor();
        const add = page.getByRole("button", { name: "Add Fabio's source" });
        await add.click();
        await page.getByText("You have changes that are not saved yet.").first().waitFor();
        assert.equal(await add.count(), 0, "once it is in the list the button is gone");
        assert.equal(calls.filter((c) => c.cmd === "save_curators").length, 0, "the button only fills the draft");
        await page.locator("section", { has: page.getByRole("heading", { name: "Sources" }) }).getByRole("button", { name: "Save", exact: true }).click();
        await page.getByText("Saved.").first().waitFor();
        const saved = calls.find((c) => c.cmd === "save_curators");
        assert.deepEqual(saved?.args.curators.map((c) => [c.name, c.key]), [["Fabio", published]]);
        assert.deepEqual(errors, [], "the page raised no error");
      } finally {
        await context.close();
      }
    });

    test("sync: sending shows the QR to approve and then the result", { timeout: TIMEOUT }, async () => {
      const waiting = deferred();
      const handlers = backend({
        sync_push_begin: { qrSvg: fakeQr(), filesChanged: 3, configChanged: true },
        sync_push_await: () => waiting.promise,
      });
      const { context, page, errors } = await openApp(browser, app, { handlers, colorScheme });
      try {
        await page.getByRole("button", { name: "Sync", exact: true }).click();
        await page.getByRole("heading", { name: "Sync" }).waitFor();
        await snap(page, `desktop-sync-${colorScheme}`);

        await page.getByRole("button", { name: "Send", exact: true }).click();
        await page.getByText("Waiting for approval…").waitFor();
        assert.equal(await page.locator(".sync-qr-image svg").count(), 1);
        await snap(page, `desktop-sync-qr-${colorScheme}`, { focus: page.locator(".sync-qr-card") });

        waiting.resolve({ txId: "invented-sync-tx", filesChanged: 3, configChanged: true });
        await page.getByText(/Sent — tx invented-sync-tx/).waitFor();
        await snap(page, `desktop-sync-sent-${colorScheme}`);
        assert.deepEqual(errors, [], "the page raised no error");
      } finally {
        waiting.resolve({ txId: "closed", filesChanged: 0, configChanged: false });
        await context.close();
      }
    });
  });
}
