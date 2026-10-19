// P10 end to end: the Usage screen's dollars, split by provider, agent and person, and per day, from a spend ledger
// the hub reads when it starts. Run with `npm run test:e2e` (see `settings.test.mjs` for what it needs).

import assert from "node:assert/strict";
import { after, before, describe, test } from "node:test";
import { PAIRING_KEY, launchBrowser, pageOptions, signIn, snap, startHub } from "./harness.mjs";

const TIMEOUT = 120_000;
const HOUR = 3_600_000;

let browser;
before(async () => {
  browser = await launchBrowser();
});
after(async () => {
  await browser?.close();
});

/** One ledger line, the way the hub writes it. The default limits keep the last 24 hours, so these are recent. */
function spend(hoursAgo, fields) {
  return JSON.stringify({ kind: "spend", ts: Date.now() - hoursAgo * HOUR, channel: "server", user: null, agent: null, person: null, ...fields });
}

/** $2.00 on `main` for the writer, $0.50 on `spare` for Ana, and one call from before the provider was kept, with no price. */
const LEDGER = [
  spend(1, { agent: "writer", model: "m", tokens: 1_000_000, cost_usd: 2.0, provider: "main" }),
  spend(2, { model: "m2", tokens: 500_000, cost_usd: 0.5, person: "ana", provider: "spare" }),
  spend(3, { model: "unpriced", tokens: 100, cost_usd: null }),
].join("\n") + "\n";

/** `US$ 2,00` → `2`: the screen writes numbers the Brazilian way. */
const dollars = (text) => Number(/US\$\s*([\d.]+(?:,\d+)?)/.exec(text)?.[1].replace(/\./g, "").replace(",", "."));

const table = (page, title) => page.locator("table.usage-table", { has: page.locator("caption", { hasText: title }) });

describe("the Usage screen", () => {
  test("shows the ledger's dollars by provider, agent and person, and per day", { timeout: TIMEOUT }, async () => {
    const hub = await startHub({ files: { "warden/spend_ledger.jsonl": LEDGER } });
    let context;
    try {
      const opened = await signIn(browser, hub);
      context = opened.context;
      const { page } = opened;
      await page.getByRole("button", { name: "Uso" }).click();
      await page.getByText(/Gasto recente/).waitFor();

      const provider = table(page, "Por provedor");
      assert.match(await provider.getByRole("row", { name: /^main/ }).innerText(), /US\$\s*2,00/, "$2.00 went to main");
      assert.match(await provider.getByRole("row", { name: /^spare/ }).innerText(), /US\$\s*0,50/, "$0.50 to spare");
      const legacy = await provider.getByRole("row", { name: /sem provedor registrado/ }).innerText();
      assert.ok(legacy.includes("—"), `a call with no price and no provider has no dollar figure: ${legacy}`);

      const agent = table(page, "Por agente");
      assert.match(await agent.getByRole("row", { name: /^writer/ }).innerText(), /US\$\s*2,00/);
      assert.ok(await agent.getByRole("row", { name: /sem agente/ }).count(), "the calls with no agent are labelled, not hidden");

      const person = table(page, "Por pessoa");
      assert.match(await person.getByRole("row", { name: /^ana/ }).innerText(), /US\$\s*0,50/);
      assert.ok(await person.getByRole("row", { name: /o dono/ }).count(), "the owner is the empty key");
      assert.match(await page.locator("body").innerText(), /“Por agente” é o agente com que o turno começou/, "the screen says what 'by agent' means");

      // Dollars per day: the three calls may fall on two calendar days, so the sum is what is certain.
      const section = page.locator("section.usage-section", { has: page.getByRole("heading", { name: /Gasto por dia/ }) });
      const labels = await section.locator(".usage-daily-slot").evaluateAll((slots) => slots.map((s) => s.getAttribute("aria-label") ?? ""));
      assert.equal(labels.length, 30, "thirty days");
      const total = labels.reduce((sum, label) => sum + (/US\$/.test(label) ? dollars(label) : 0), 0);
      const calls = labels.reduce((sum, label) => sum + Number(/em (\d+) chamadas/.exec(label)?.[1] ?? 0), 0);
      assert.equal(calls, 3, "every call is on some day");
      assert.ok(Math.abs(total - 2.5) < 1e-6, `$2.50 across the days, got ${total}`);
      assert.match(await section.innerText(), /só guarda a janela do limite mais longo/, "and it says how far back the ledger reaches");
      await snap(page, "usage-top");
      await snap(page, "usage-daily", { focus: section });
      assert.deepEqual(opened.errors, [], "the page raised no error");
    } finally {
      await context?.close();
      await hub.stop();
    }
  });
});

/** The owner's page makes `ana` (Pessoas → Adicionar pessoa) and returns her temporary password. */
async function createAna(page) {
  await page.getByRole("button", { name: "Pessoas" }).click();
  await page.getByRole("button", { name: "Adicionar pessoa" }).click();
  await page.getByLabel("Usuário").fill("ana");
  await page.getByLabel("Nome", { exact: true }).fill("Ana Souza");
  await page
    .locator("form.settings-confirm", { has: page.getByRole("button", { name: "Criar", exact: true }) })
    .getByLabel("Chave de pareamento do hub")
    .fill(PAIRING_KEY);
  await page.getByRole("button", { name: "Criar" }).click();
  const shown = page.locator(".settings-confirm", { hasText: "Senha provisória de" });
  await shown.waitFor();
  const password = (await shown.locator("code").innerText()).trim();
  await shown.getByRole("button", { name: "Já anotei" }).click();
  return password;
}

/** A new page signed in as `ana`, past the forced change of password and the recovery code, if the hub shows one. */
async function signInAsAna(hub, temporary) {
  const context = await browser.newContext(pageOptions);
  const page = await context.newPage();
  const errors = [];
  page.on("pageerror", (e) => errors.push(e.message));
  await page.goto(hub.url);
  await page.getByRole("tab", { name: "Usuário" }).click();
  await page.getByLabel("Usuário").fill("ana");
  await page.getByLabel("Senha").fill(temporary);
  await page.getByRole("button", { name: "Entrar" }).click();
  await page.getByText("troque a senha provisória por uma sua").waitFor();
  await page.getByLabel("Senha provisória").fill(temporary);
  await page.getByLabel("Senha nova", { exact: true }).fill("a-new-password-123");
  await page.getByLabel("Repita a senha nova").fill("a-new-password-123");
  await page.getByRole("button", { name: "Trocar senha" }).click();
  const newConversation = page.getByRole("button", { name: "+ Nova conversa" });
  const saved = page.getByLabel(/anotei|guardei|salvei/i);
  await Promise.race([newConversation.waitFor(), saved.waitFor()]);
  if (await saved.count()) {
    await saved.check();
    await page.getByRole("button", { name: /continuar|concluir|pronto/i }).click();
    await newConversation.waitFor();
  }
  return { page, context, errors };
}

describe("a member's Usage screen (P99)", () => {
  test("shows only her own spending and limits, and no way to allow more", { timeout: TIMEOUT }, async () => {
    // The owner spent $2.00 on `main`, Ana $0.50 on `spare`; Ana has a limit of her own and the hub has one that adds up everybody.
    const extraConfig = [
      "",
      "[[limits]]",
      'id = "ana-day"',
      'scope = "person"',
      'target = "ana"',
      "window_hours = 24",
      "max_tokens = 1000000",
      "",
      "[[limits]]",
      'id = "hub-day"',
      "window_hours = 24",
      "max_tokens = 100000000",
      "",
    ].join("\n");
    const hub = await startHub({ extraConfig, files: { "warden/spend_ledger.jsonl": LEDGER } });
    const owner = await signIn(browser, hub);
    let member;
    try {
      await owner.page.getByRole("button", { name: "+ Nova conversa" }).waitFor();
      const temporary = await createAna(owner.page);

      member = await signInAsAna(hub, temporary);
      const { page } = member;
      await page.getByRole("button", { name: "Uso", exact: true }).click();
      await page.getByText(/Gasto recente/).waitFor();

      const body = await page.locator("body").innerText();
      assert.ok(body.includes("ana-day"), "her own limit is there");
      assert.ok(!body.includes("hub-day"), "the limit that adds up everyone's spending is not");
      assert.ok(!/Por device|Por pessoa|Liberar/.test(body), "nothing about devices or other people, and no way to allow more");
      assert.match(body, /peça a quem administra/, "and it says who to ask");

      const provider = table(page, "Por provedor");
      assert.match(await provider.getByRole("row", { name: /^spare/ }).innerText(), /US\$\s*0,50/, "her $0.50");
      assert.equal(await provider.getByRole("row", { name: /^main/ }).count(), 0, "the owner's $2.00 on main is not hers");
      assert.deepEqual(member.errors, [], "her page raised no error");

      // The owner still sees everything, with the way to allow more still there for a limit that ran out.
      await owner.page.getByRole("button", { name: "Uso", exact: true }).click();
      await owner.page.getByText(/Gasto recente/).waitFor();
      const all = await owner.page.locator("body").innerText();
      assert.ok(all.includes("ana-day") && all.includes("hub-day") && all.includes("Por device"), "the owner's screen is whole");
    } finally {
      assert.deepEqual(owner.errors, [], "the owner's page raised no error");
      await member?.context.close();
      await owner.context.close();
      await hub.stop();
    }
  });
});
