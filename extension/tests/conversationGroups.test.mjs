// P157 — como a lista agrupa as conversas soltas (`src/protocol/conversationGroups.ts`). Rode com `npm test`.

import assert from "node:assert/strict";
import { describe, test } from "node:test";
import { bucketLabel, groupConversations } from "../src/protocol/conversationGroups.ts";

// Meio-dia de uma quarta-feira, no relógio desta máquina: seja qual for o fuso, os dias em volta são os do calendário.
const now = new Date(2026, 9, 14, 12, 0, 0).getTime();
const at = (daysAgo, hour = 9) => new Date(2026, 9, 14 - daysAgo, hour, 0, 0).getTime();
const conversation = (id, updatedAt, extra = {}) => ({ id, title: id, createdAt: 1, updatedAt, ...extra });
const ids = (items) => items.map((c) => c.id);

describe("quão recente é uma conversa", () => {
  test("conta no calendário: esta manhã é hoje, a noite passada é ontem", () => {
    assert.equal(bucketLabel(at(0, 0), now), "Hoje");
    assert.equal(bucketLabel(at(0, 11), now), "Hoje");
    assert.equal(bucketLabel(new Date(2026, 9, 13, 23, 59, 59).getTime(), now), "Ontem");
    assert.equal(bucketLabel(at(1, 0), now), "Ontem");
  });

  test("volta pela semana e pelo mês e depois é anterior", () => {
    assert.equal(bucketLabel(at(2), now), "Últimos 7 dias");
    assert.equal(bucketLabel(at(7, 0), now), "Últimos 7 dias", "os sete dias antes de ontem: de 2 a 7 dias atrás");
    assert.equal(bucketLabel(at(8, 23), now), "Últimos 30 dias");
    assert.equal(bucketLabel(at(30, 0), now), "Últimos 30 dias");
    assert.equal(bucketLabel(at(31, 23), now), "Anteriores");
    assert.equal(bucketLabel(0, now), "Anteriores");
  });
});

describe("a lista agrupada", () => {
  test("põe as fixadas primeiro, o resto em blocos por data, as arquivadas à parte, e deixa os blocos vazios de fora", () => {
    const list = [
      conversation("nova", at(0)),
      conversation("fixa", at(40), { pinned: true }),
      conversation("ontem", at(1)),
      conversation("velha", at(90)),
      conversation("guardada", at(0), { archived: true }),
      conversation("semana", at(3)),
      conversation("nova2", at(0, 8)),
    ];
    const grouped = groupConversations(list, now);
    assert.deepEqual(ids(grouped.pinned), ["fixa"]);
    assert.deepEqual(grouped.buckets.map((b) => [b.label, ids(b.items)]), [["Hoje", ["nova", "nova2"]], ["Ontem", ["ontem"]], ["Últimos 7 dias", ["semana"]], ["Anteriores", ["velha"]]]);
    assert.deepEqual(ids(grouped.archived), ["guardada"]);
  });

  test("uma conversa arquivada só está em arquivadas, mesmo que diga que também está fixada", () => {
    const grouped = groupConversations([conversation("as-duas", at(0), { pinned: true, archived: true })], now);
    assert.deepEqual([ids(grouped.pinned), grouped.buckets, ids(grouped.archived)], [[], [], ["as-duas"]]);
  });

  test("uma lista vazia são três partes vazias, e as conversas são os mesmos objetos", () => {
    assert.deepEqual(groupConversations([], now), { pinned: [], buckets: [], archived: [] });
    const one = conversation("a", at(0));
    assert.equal(groupConversations([one], now).buckets[0].items[0], one);
  });
});
