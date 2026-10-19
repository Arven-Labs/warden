// P120, P123 — what the extension reads of the hub's settings and of the delegated tasks (`src/protocol/messages.ts`, `lib/modelPolicies.ts`).
// Run with `npm test`.

import assert from "node:assert/strict";
import { describe, test } from "node:test";
import { decode } from "../src/protocol/messages.ts";
import {
  delegationCandidates,
  delegationSummary,
  limitEdit,
  limitModels,
  nextPolicyId,
  policiesEdit,
  policiesWith,
  policiesWithout,
} from "../src/sidepanel/lib/modelPolicies.ts";

const settings = (agents, extra = {}) => ({ agents, ...extra });

describe("decoding the hub's settings", () => {
  test("keeps the agent ids the chat selector uses and reads the organization", () => {
    const reply = decode(
      JSON.stringify({
        type: "settings",
        requestId: 3,
        settings: settings(
          [
            { id: "chief", role: "CTO", canDelegateToAgents: true, canManageAgents: true, autonomy: 3, approvalRequired: ["spend_money"], delegationModels: ["fast"] },
            { id: "dev", reportsTo: "chief" },
          ],
          { modelPolicies: [{ id: "fast", model: "gpt-mini", description: "quick" }] },
        ),
      }),
    );
    assert.equal(reply.type, "settings");
    assert.deepEqual(reply.agentIds, ["chief", "dev"]);
    assert.deepEqual(reply.agents[0], {
      id: "chief",
      role: "CTO",
      reportsTo: null,
      canDelegateToAgents: true,
      canManageAgents: true,
      canMessageAgents: false,
      canManageTasks: false,
      canStartTasks: true,
      canCreateWorkers: true,
      canMessageUser: true,
      canChooseModels: true,
      autonomy: 3,
      approvalRequired: ["spend_money"],
      delegationModels: ["fast"],
    });
    assert.equal(reply.agents[1].reportsTo, "chief");
    assert.deepEqual(reply.modelPolicies, [{ id: "fast", model: "gpt-mini", description: "quick" }]);
  });

  test("reads the permissions a person took away, and an older hub that doesn't send them reads as on", () => {
    const reply = decode(JSON.stringify({ type: "settings", requestId: 1, settings: settings([{ id: "a", canStartTasks: false, canMessageUser: false }, { id: "b" }]) }));
    const [a, b] = reply.agents;
    assert.deepEqual([a.canStartTasks, a.canCreateWorkers, a.canMessageUser, a.canChooseModels], [false, true, false, true]);
    assert.deepEqual([b.canStartTasks, b.canCreateWorkers, b.canMessageUser, b.canChooseModels], [true, true, true, true]);
  });

  test("fills what an older hub leaves out: no policies, autonomy 4, an open model choice", () => {
    const reply = decode(JSON.stringify({ type: "settings", requestId: 1, settings: settings([{ id: "solo" }]) }));
    assert.deepEqual(reply.modelPolicies, []);
    assert.equal(reply.agents[0].autonomy, 4);
    assert.deepEqual(reply.agents[0].delegationModels, []);
    assert.deepEqual(reply.agents[0].approvalRequired, []);
  });

  test("an organization edit is answered with the agents as they are now", () => {
    const reply = decode(JSON.stringify({ type: "settingsSaved", requestId: 9, version: "v2", settings: settings([{ id: "a", reportsTo: "b" }, { id: "b" }]) }));
    assert.equal(reply.type, "settingsSaved");
    assert.equal(reply.requestId, 9);
    assert.deepEqual(reply.agents.map((a) => a.id), ["a", "b"]);
  });

  test("a refused key comes through as authRejected", () => {
    const reply = decode(JSON.stringify({ type: "settingsError", requestId: 2, message: "wrong key", conflict: false, authRejected: true }));
    assert.equal(reply.type, "settingsError");
    assert.equal(reply.authRejected, true);
  });
});

describe("decoding the device invite and the key rotation (P36)", () => {
  test("an invite comes back with its code and when it lapses", () => {
    const reply = decode(JSON.stringify({ type: "deviceInvite", requestId: 5, name: "Ana's phone", code: "abc", expiresMs: 9 }));
    assert.deepEqual(reply, { type: "deviceInvite", requestId: 5, name: "Ana's phone", code: "abc", expiresMs: 9 });
  });

  test("a rotation comes back with the new key", () => {
    const reply = decode(JSON.stringify({ type: "pairingKeyRotated", requestId: 6, newKey: "new" }));
    assert.deepEqual(reply, { type: "pairingKeyRotated", requestId: 6, newKey: "new" });
  });

  test("the invites that wait come back by name and deadline, with no code", () => {
    const reply = decode(JSON.stringify({ type: "deviceInviteList", requestId: 8, invites: [{ name: "Ana's phone", createdMs: 1, expiresMs: 9 }] }));
    assert.deepEqual(reply, { type: "deviceInviteList", requestId: 8, invites: [{ name: "Ana's phone", createdMs: 1, expiresMs: 9 }] });
  });

  test("a refused key comes through as authRejected", () => {
    const reply = decode(JSON.stringify({ type: "deviceError", requestId: 7, message: "wrong key", authRejected: true }));
    assert.equal(reply.type, "deviceError");
    assert.equal(reply.authRejected, true);
  });
});

describe("decoding the delegated tasks", () => {
  test("a task list keeps its tasks as the hub sent them", () => {
    const task = { id: "at-1", group: "g", assignee: "dev", objective: "x", channel: "cli", state: "running", createdAtMs: 5, controllable: true, pausable: true };
    const reply = decode(JSON.stringify({ type: "agentTaskList", requestId: 4, tasks: [task] }));
    assert.equal(reply.type, "agentTaskList");
    assert.deepEqual(reply.tasks, [task]);
  });

  test("a usage report keeps what each agent spent, and an older hub's report without it still decodes (P123)", () => {
    const report = { recent: { windowHours: 24, byAgent: [{ key: "dev", calls: 3, tokens: 90, costUsd: 0.25, unpricedCalls: 0 }] }, total: {} };
    const reply = decode(JSON.stringify({ type: "usageReport", requestId: 5, report }));
    assert.equal(reply.type, "usageReport");
    assert.deepEqual(reply.report.recent.byAgent, report.recent.byAgent);
    assert.equal(decode(JSON.stringify({ type: "usageReport", requestId: 6, report: {} })).type, "usageReport");
  });

  test("a usage report keeps the token totals and breakdowns by agent, provider and combo (P126)", () => {
    const entry = (key, messageCount, totalTokens) => ({ key, messageCount, usage: { totalTokens } });
    const report = {
      total: { totalTokens: 35 },
      messageCount: 3,
      byAgent: [entry("writer", 2, 30), entry(null, 1, 5)],
      byProvider: [entry("main", 3, 35)],
      byCombo: [entry("fast", 2, 30)],
    };
    const reply = decode(JSON.stringify({ type: "usageReport", requestId: 7, report }));
    assert.deepEqual(reply.report, report);
  });

  test("a grant list keeps the \"always\" permissions as the hub sent them (P122)", () => {
    const grants = [{ conversationId: "c1", agentId: "chief", covers: "delete_data" }, { conversationId: "c1", covers: "shell" }];
    const reply = decode(JSON.stringify({ type: "grantList", requestId: 6, grants, ttlDays: 7 }));
    assert.equal(reply.type, "grantList");
    assert.deepEqual(reply.grants, grants);
    assert.equal(reply.ttlDays, 7, "and how long one may go unused");
    assert.equal(decode(JSON.stringify({ type: "grantList", requestId: 6, grants })).ttlDays, undefined, "absent when the hub keeps none in a file");
  });

  test("a task error says whether the key was the problem", () => {
    const reply = decode(JSON.stringify({ type: "taskError", requestId: 5, message: "not running here", authRejected: false }));
    assert.equal(reply.type, "taskError");
    assert.equal(reply.authRejected, false);
  });
});

describe("decoding the feed of activity (P121)", () => {
  test("the events come through as the hub sent them, optional fields left out", () => {
    const events = [
      { id: "at-1-delegated", atMs: 5, kind: "delegated", actor: "manager", target: "dev", text: "build it", taskId: "at-1" },
      { id: "c-m1", atMs: 9, kind: "messaged_user", actor: "pirate", text: "disk full", conversationId: "channel-1" },
    ];
    const reply = decode(JSON.stringify({ type: "activityList", requestId: 8, events }));
    assert.equal(reply.type, "activityList");
    assert.deepEqual(reply.events, events);
  });
});

describe("the models a hub offers", () => {
  test("settings carry the ids of the providers and combos, apart from the policies", () => {
    const reply = decode(
      JSON.stringify({
        type: "settings",
        requestId: 1,
        settings: { agents: [], providers: [{ id: "main" }, { id: "spare" }], combos: [{ id: "both" }], modelPolicies: [{ id: "fast", model: "main", description: "" }] },
      }),
    );
    assert.deepEqual(reply.modelIds, ["main", "spare", "both"]);
    assert.deepEqual(delegationCandidates(reply.modelIds, reply.modelPolicies), ["main", "spare", "both", "fast"]);
  });

  test("a hub with no providers listed offers none", () => {
    assert.deepEqual(decode(JSON.stringify({ type: "settingsSaved", requestId: 1, settings: { agents: [] } })).modelIds, []);
  });
});

describe("editing a model limit and the policies", () => {
  test("the limit puts the default first and keeps the candidates' order for the rest, and nothing checked is open", () => {
    const candidates = ["main", "spare", "fast"];
    assert.deepEqual(limitModels(candidates, new Set(["main", "fast"]), "fast"), ["fast", "main"]);
    assert.deepEqual(limitModels(candidates, new Set(["spare", "fast", "main"]), "main"), ["main", "spare", "fast"]);
    assert.deepEqual(limitModels(candidates, new Set(["spare"]), "main"), ["spare"], "a default that isn't checked is ignored");
    assert.deepEqual(limitModels(candidates, new Set(), ""), []);
    assert.deepEqual(limitEdit("chief", ["fast"]), { kind: "setDelegationModels", id: "chief", models: ["fast"] });
  });

  test("a policy is replaced where it was, or added at the end, with the text trimmed", () => {
    const policies = [
      { id: "fast", model: "main", description: "quick" },
      { id: "deep", model: "spare" },
    ];
    assert.deepEqual(policiesWith(policies, { id: " fast ", model: " spare ", description: " cheap " }, "fast"), [
      { id: "fast", model: "spare", description: "cheap" },
      { id: "deep", model: "spare" },
    ]);
    assert.deepEqual(policiesWith(policies, { id: "code", model: "main" }, null).at(-1), { id: "code", model: "main", description: "" });
    assert.deepEqual(policiesWith(policies, { id: "renamed", model: "main", description: "" }, "deep").map((p) => p.id), ["fast", "renamed"]);
    assert.deepEqual(policiesWithout(policies, "fast").map((p) => p.id), ["deep"]);
  });

  test("the policies go to the hub with a description always present", () => {
    assert.deepEqual(policiesEdit([{ id: "deep", model: "spare" }]), { kind: "setModelPolicies", policies: [{ id: "deep", model: "spare", description: "" }] });
    assert.deepEqual(policiesEdit([]), { kind: "setModelPolicies", policies: [] });
  });

  test("a new policy gets a free name", () => {
    assert.equal(nextPolicyId(["main"]), "policy-1");
    assert.equal(nextPolicyId(["policy-1", "policy-2"]), "policy-3");
  });
});

describe("what a model limit says", () => {
  test("open, dictated and limited", () => {
    assert.match(delegationSummary(undefined), /^Open/);
    assert.match(delegationSummary([]), /^Open/);
    assert.equal(delegationSummary(["fast"]), "Dictated: every task it delegates runs on fast.");
    assert.match(delegationSummary(["fast", "deep"]), /^Limited to 2 models; fast is what/);
  });
});
