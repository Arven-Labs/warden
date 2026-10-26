import assert from "node:assert/strict";
import { test } from "node:test";
import { callbackAddress } from "../src/lib/hubMcpLogin.ts";
import { lib } from "../src/i18n/en/lib.ts";
import { libPt } from "../src/i18n/pt-BR/lib.ts";
import { settings } from "../src/i18n/en/settings.ts";
import { settingsPt } from "../src/i18n/pt-BR/settings.ts";

const PREFIX = "http://127.0.0.1:51234/callback";

test("the query that reached the listener becomes the address the hub is handed", () => {
  assert.deepEqual(callbackAddress(PREFIX, "code=abc&state=xyz"), { address: `${PREFIX}?code=abc&state=xyz` });
  assert.deepEqual(callbackAddress(PREFIX, "?code=abc&state=xyz&scope=mcp"), { address: `${PREFIX}?code=abc&state=xyz&scope=mcp` }, "a leading ? is not doubled");
  assert.deepEqual(callbackAddress(PREFIX, "state=xyz&code=abc"), { address: `${PREFIX}?state=xyz&code=abc` }, "the order does not matter");
});

test("a refusal or a return without the code says why and is not sent to the hub", () => {
  assert.deepEqual(callbackAddress(PREFIX, "error=access_denied&state=xyz"), { problem: "denied" });
  assert.deepEqual(callbackAddress(PREFIX, "code=abc&error=server_error&state=xyz"), { problem: "denied" });
  assert.deepEqual(callbackAddress(PREFIX, "state=xyz"), { problem: "noCode" });
  assert.deepEqual(callbackAddress(PREFIX, "code=abc"), { problem: "noCode" });
  assert.deepEqual(callbackAddress(PREFIX, "code=&state="), { problem: "noCode" });
  assert.deepEqual(callbackAddress(PREFIX, ""), { problem: "noCode" });
});

test("every line the hub sign-ins can show exists in both languages", () => {
  const keys = [
    "st.hubLogin.title", "st.hubLogin.hint", "st.hubLogin.none", "st.hubLogin.connected", "st.hubLogin.notConnected", "st.hubLogin.connect",
    "st.hubLogin.again", "st.hubLogin.disconnect", "st.hubLogin.waiting", "st.hubLogin.done", "st.hubLogin.forgot", "st.hubLogin.failed",
    "st.hubLogin.loadFailed", "st.hubLogin.problem.denied", "st.hubLogin.problem.noCode",
  ];
  for (const [name, table] of [["en", settings], ["pt-BR", settingsPt]]) {
    for (const key of keys) assert.ok(table[key], `${name}: ${key}`);
  }
  for (const [name, table] of [["en", lib], ["pt-BR", libPt]]) assert.ok(table["askKey.mcpLogin"], `${name}: askKey.mcpLogin`);
  assert.match(settings["st.hubLogin.failed"], /\{error\}/);
  assert.match(settingsPt["st.hubLogin.loadFailed"], /\{error\}/);
});
