// P157 — how the list groups the loose conversations (`src/protocol/conversationGroups.ts`). Run with `npm test`.

import assert from "node:assert/strict";
import { describe, test } from "node:test";
import { bucketLabel, groupConversations } from "../src/protocol/conversationGroups.ts";

// Noon on a Wednesday, on this machine's clock: whatever the time zone, the days around it are the calendar's.
const now = new Date(2026, 9, 14, 12, 0, 0).getTime();
const at = (daysAgo, hour = 9) => new Date(2026, 9, 14 - daysAgo, hour, 0, 0).getTime();
const conversation = (id, updatedAt, extra = {}) => ({ id, title: id, createdAt: 1, updatedAt, ...extra });
const ids = (items) => items.map((c) => c.id);

describe("how recent a conversation is", () => {
  test("counts on the calendar: this morning is today, last night is yesterday", () => {
    assert.equal(bucketLabel(at(0, 0), now), "Today");
    assert.equal(bucketLabel(at(0, 11), now), "Today");
    assert.equal(bucketLabel(new Date(2026, 9, 13, 23, 59, 59).getTime(), now), "Yesterday");
    assert.equal(bucketLabel(at(1, 0), now), "Yesterday");
  });

  test("goes back through the week and the month and then it is older", () => {
    assert.equal(bucketLabel(at(2), now), "Last 7 days");
    assert.equal(bucketLabel(at(7, 0), now), "Last 7 days", "the seven days before yesterday: from 2 to 7 days ago");
    assert.equal(bucketLabel(at(8, 23), now), "Last 30 days");
    assert.equal(bucketLabel(at(30, 0), now), "Last 30 days");
    assert.equal(bucketLabel(at(31, 23), now), "Older");
    assert.equal(bucketLabel(0, now), "Older");
  });
});

describe("the grouped list", () => {
  test("puts the pinned first, the rest in date blocks, the archived apart, and leaves empty blocks out", () => {
    const list = [
      conversation("new", at(0)),
      conversation("pinned", at(40), { pinned: true }),
      conversation("yesterday", at(1)),
      conversation("old", at(90)),
      conversation("stored", at(0), { archived: true }),
      conversation("week", at(3)),
      conversation("new2", at(0, 8)),
    ];
    const grouped = groupConversations(list, now);
    assert.deepEqual(ids(grouped.pinned), ["pinned"]);
    assert.deepEqual(grouped.buckets.map((b) => [b.label, ids(b.items)]), [["Today", ["new", "new2"]], ["Yesterday", ["yesterday"]], ["Last 7 days", ["week"]], ["Older", ["old"]]]);
    assert.deepEqual(ids(grouped.archived), ["stored"]);
  });

  test("an archived conversation is only in archived, even if it says it is also pinned", () => {
    const grouped = groupConversations([conversation("both", at(0), { pinned: true, archived: true })], now);
    assert.deepEqual([ids(grouped.pinned), grouped.buckets, ids(grouped.archived)], [[], [], ["both"]]);
  });

  test("an empty list is three empty parts, and the conversations are the same objects", () => {
    assert.deepEqual(groupConversations([], now), { pinned: [], buckets: [], archived: [] });
    const one = conversation("a", at(0));
    assert.equal(groupConversations([one], now).buckets[0].items[0], one);
  });
});
