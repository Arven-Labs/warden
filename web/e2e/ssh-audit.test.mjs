// P47 end to end: the SSH usage log in Configurações → Máquina do hub, read from the `ssh_audit.jsonl` the hub's SSH tools write.
// Run with `npm run test:e2e` (see `settings.test.mjs` for what it needs).

import assert from "node:assert/strict";
import { after, before, describe, test } from "node:test";
import { launchBrowser, openSettings, snap, startHub } from "./harness.mjs";

const TIMEOUT = 120_000;

let browser;
before(async () => {
  browser = await launchBrowser();
});
after(async () => {
  await browser?.close();
});

/** One log line, the way the tools write it. */
const line = (ts, fields) => JSON.stringify({ ts, host_id: "prod", agent: null, action: "exec", approval: "not_required", outcome: { exit_code: 0 }, ...fields });

const LOG =
  [
    line(1_000, { agent: "ops", approval: "approved", command: "uptime" }),
    line(2_000, { action: "upload", local_path: "/tmp/a.txt", remote_path: "/srv/a.txt", overwrite: true, outcome: { bytes: 2048 } }),
    line(3_000, { agent: "ops", approval: "denied", command: "reboot", outcome: { error: "refused: not approved" } }),
    "this line is not JSON and must not break the screen",
  ].join("\n") + "\n";

describe("the SSH usage log", () => {
  test("lists the calls newest first with who asked, whether it was approved and how it ended", { timeout: TIMEOUT }, async () => {
    const hub = await startHub({ files: { "warden/ssh_audit.jsonl": LOG } });
    let context;
    try {
      const opened = await openSettings(browser, hub);
      context = opened.context;
      const { page } = opened;

      const log = page.locator("details.ssh-audit");
      assert.equal(await log.locator("table").count(), 0, "nothing is read until the log is opened");
      await log.locator("summary").click();
      await log.locator("table").waitFor();
      await snap(page, "ssh-audit-log", { focus: log });

      const rows = await log.locator("tbody tr").allInnerTexts();
      assert.equal(rows.length, 3, "the broken line is skipped");
      assert.match(rows[0], /reboot/);
      assert.match(rows[0], /recusada/);
      assert.match(rows[0], /erro: refused: not approved/);
      assert.match(rows[1], /\/tmp\/a\.txt → \/srv\/a\.txt \(sobrescreve\)/);
      assert.match(rows[1], /envio/);
      assert.match(rows[1], /2 KB/);
      assert.match(rows[2], /uptime/);
      assert.match(rows[2], /ops/);
      assert.match(rows[2], /aprovada/);
      assert.match(rows[2], /saiu com 0/);
      assert.deepEqual(opened.errors, [], "the page raised no error");
    } finally {
      await context?.close();
      await hub.stop();
    }
  });

  test("says so when no call was made yet", { timeout: TIMEOUT }, async () => {
    const hub = await startHub();
    let context;
    try {
      const opened = await openSettings(browser, hub);
      context = opened.context;
      const log = opened.page.locator("details.ssh-audit");
      await log.locator("summary").click();
      await log.getByText("Nenhuma chamada registrada ainda.").waitFor();
      assert.equal(await log.locator("table").count(), 0);
    } finally {
      await context?.close();
      await hub.stop();
    }
  });
});
