import assert from "node:assert/strict";
import { test } from "node:test";
import { checkPastedAddress } from "../src/hub/mcpLogin.ts";
import { machine } from "../src/i18n/en/machine.ts";
import { machinePt } from "../src/i18n/pt-BR/machine.ts";

const PREFIX = "http://127.0.0.1:45871/callback";
const GOOD = `${PREFIX}?code=abc123&state=xyz789`;

test("the address the browser ended on is taken as it is, with the spaces and quotes a copy brings taken off", () => {
  assert.deepEqual(checkPastedAddress(GOOD, PREFIX), { address: GOOD });
  for (const wrapped of [`  ${GOOD}\n`, `"${GOOD}"`, `'${GOOD}'`, `<${GOOD}>`]) {
    assert.deepEqual(checkPastedAddress(wrapped, PREFIX), { address: GOOD }, wrapped);
  }
  assert.deepEqual(checkPastedAddress(`${PREFIX}?state=xyz789&code=abc123&scope=mcp`, PREFIX), { address: `${PREFIX}?state=xyz789&code=abc123&scope=mcp` }, "the order and extra fields do not matter");
});

test("what cannot be the return address says why, before the hub is asked", () => {
  assert.deepEqual(checkPastedAddress("", PREFIX), { problem: "empty" });
  assert.deepEqual(checkPastedAddress("   ", PREFIX), { problem: "empty" });
  assert.deepEqual(checkPastedAddress("code=abc&state=xyz", PREFIX), { problem: "notAnAddress" });
  assert.deepEqual(checkPastedAddress(`${GOOD} and more words`, PREFIX), { problem: "notAnAddress" });
  for (const elsewhere of [
    "https://evil.example/callback?code=abc&state=xyz",
    "http://127.0.0.1:2/callback?code=abc&state=xyz",
    `${PREFIX}/../x?code=abc&state=xyz`,
    `${PREFIX}x?code=abc&state=xyz`,
    PREFIX,
  ]) {
    assert.deepEqual(checkPastedAddress(elsewhere, PREFIX), { problem: "wrongPlace" }, elsewhere);
  }
  assert.deepEqual(checkPastedAddress(`${PREFIX}?error=access_denied&state=xyz789`, PREFIX), { problem: "denied" });
  assert.deepEqual(checkPastedAddress(`${PREFIX}?state=xyz789`, PREFIX), { problem: "noCode" });
  assert.deepEqual(checkPastedAddress(`${PREFIX}?code=abc123`, PREFIX), { problem: "noCode" });
  assert.deepEqual(checkPastedAddress(`${PREFIX}?code=&state=`, PREFIX), { problem: "noCode" });
});

test("every line the sign-in can show exists in both languages", () => {
  const keys = [
    "machine.oauth", "machine.login.saveFirst", "machine.login.checking", "machine.login.connected", "machine.login.notConnected", "machine.login.connect",
    "machine.login.again", "machine.login.disconnect", "machine.login.keyPrompt", "machine.login.working", "machine.login.steps", "machine.login.open",
    "machine.login.pasteLabel", "machine.login.finish", "machine.login.done", "machine.login.forgot", "machine.login.failed",
    "machine.login.problem.empty", "machine.login.problem.notAnAddress", "machine.login.problem.wrongPlace", "machine.login.problem.denied", "machine.login.problem.noCode",
    "machine.cat.needs.oauth", "machine.cat.slack.label", "machine.cat.slack.description", "machine.cat.notionRemote.label", "machine.cat.notionRemote.description",
    "machine.cat.sentry.label", "machine.cat.sentry.description",
  ];
  for (const [name, table] of [["en", machine], ["pt-BR", machinePt]]) {
    for (const key of keys) assert.ok(table[key], `${name}: ${key}`);
  }
  assert.match(machine["machine.login.failed"], /\{error\}/);
  assert.match(machinePt["machine.login.failed"], /\{error\}/);
});
