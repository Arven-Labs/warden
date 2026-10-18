// P157 — regenerar e editar, a parte pura (`src/hub/regenerate.ts`). Rode com `npm test`.

import assert from "node:assert/strict";
import { describe, test } from "node:test";
import { entriesBefore, isLastAnswer, lastUserEntry, messagesAfter, redoBlock } from "../src/hub/regenerate.ts";

const entry = (role, id) => ({ ...(id && { id }), role, content: id ?? role, attachments: [] });
const saved = [entry("user", "u1"), entry("assistant", "a1"), entry("user", "u2"), entry("assistant", "a2")];

describe("de qual mensagem o refazer parte", () => {
  test("regenerar parte da última mensagem da pessoa que o hub já guardou", () => {
    assert.equal(lastUserEntry(saved).id, "u2");
    assert.equal(lastUserEntry([entry("assistant", "a1")]), undefined);
    assert.equal(lastUserEntry([]), undefined);
    assert.equal(lastUserEntry([entry("user", "u1"), entry("user")]).id, "u1", "uma mensagem ainda sem id não serve");
  });

  test("só a última resposta guardada se regenera, e os erros depois dela não contam", () => {
    assert.deepEqual(saved.map((_, i) => isLastAnswer(saved, i)), [false, false, false, true]);
    const withError = [...saved, entry("error")];
    assert.equal(isLastAnswer(withError, 3), true);
    assert.equal(isLastAnswer([entry("user", "u1"), entry("assistant")], 1), false, "uma resposta sem id ainda não");
    assert.equal(isLastAnswer(saved, 9), false);
  });

  test("o modelo vê o que veio antes da mensagem, e tudo se a mensagem não está", () => {
    assert.deepEqual(entriesBefore(saved, "u2").map((e) => e.id), ["u1", "a1"]);
    assert.deepEqual(entriesBefore(saved, "u1"), []);
    assert.equal(entriesBefore(saved, "fantasma"), saved);
  });

  test("uma edição apaga também o que veio depois, e isso é contado para o aviso, sem os erros", () => {
    assert.equal(messagesAfter(saved, "u1"), 3);
    assert.equal(messagesAfter(saved, "a2"), 0);
    assert.equal(messagesAfter(saved, "fantasma"), 0);
    assert.equal(messagesAfter([entry("user", "u1"), entry("error"), entry("assistant", "a1")], "u1"), 1);
  });
});

describe("quando uma conversa se refaz", () => {
  const free = { onHub: true, thread: false, code: false, busy: false, offline: false };

  test("livre quando está no hub, não é thread nem de código, e nada está a caminho", () => {
    assert.equal(redoBlock(free), null);
  });

  test("cada motivo bloqueia e diz qual", () => {
    for (const [why, state] of [
      ["sem conexão", { offline: true }],
      ["resposta a caminho", { busy: true }],
      ["ainda não está no hub", { onHub: false }],
      ["thread", { thread: true }],
      ["projeto de código", { code: true }],
    ]) {
      assert.equal(typeof redoBlock({ ...free, ...state }), "string", why);
    }
  });
});
