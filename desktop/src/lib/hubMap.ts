// P102 phase 2 — what a hub says, as the screens of this app already read it. Pure on purpose (no Tauri, no React):
// the hub's `ConversationSummary`, `HistoryMessage`, `ChatResponse`, `AgentSettingsDto` and `ProjectDto` become
// `Conversation`, `ChatMessage`, a turn's reply, `AgentEntry` and `ProjectEntry`, and `tests/hubMap.test.mjs` runs them
// with Node. The wire shapes mirror `crates/warden-server-protocol/src/protocol.rs`.

import { t } from "../i18n/index.ts";
import type {
  AgentEntry,
  Attachment,
  ChatMessage,
  Conversation,
  Credit,
  LimitStatus,
  ProjectEntry,
  ProviderFallback,
  RecentSpend,
  SpendStatus,
  Usage,
  UsageByKey,
  UsageSummary,
} from "../types";

/** The hub's `UserInfoDto`, only the part this app reads. */
export interface HubUser {
  id: string;
  name: string;
  role: string;
  mustChangePassword: boolean;
  /** Encrypted, and only a password sign-in opens it again after the hub restarted. */
  locked?: boolean;
  /** P120 — what the owner lets them do with the organization of the agents: "view" or "edit"; absent: none. */
  orgAccess?: string;
}

/** Where the connection to a hub stands (`warden_server::remote_client::RemoteState`, tagged by `state`). */
export type RemoteState =
  | { state: "connecting" }
  | { state: "connected"; user: HubUser | null }
  | { state: "retrying"; error: string; inSecs: number }
  | { state: "stopped"; error: string | null };

/** The `remote-hub-state` event. */
export interface RemoteStatePayload {
  hubId: string;
  state: RemoteState;
}

/** The `remote-truthid-challenge` event (P113): the QR the TruthID app scans to sign in to a hub, drawn by the backend. */
export interface TruthIdChallengePayload {
  hubId: string;
  svg: string;
  /** The QR's own JSON, for pasting where a camera is not an option. */
  payload: string;
  /** When the TruthID app stops accepting this QR. */
  expiresAtMs: number;
}

/** Whole seconds a QR is still good for (never below 0). */
export function qrSecondsLeft(expiresAtMs: number, nowMs: number): number {
  return Math.max(0, Math.ceil((expiresAtMs - nowMs) / 1000));
}

/** What a TruthID sign-in says to the person: the QR is waiting, it ran out, or the hub turned it away. `null`: nothing to say yet. */
export function truthIdProgress(state: RemoteState | undefined, qr: TruthIdChallengePayload | null, nowMs: number): { kind: "waiting" | "expired" | "failed"; text: string } | null {
  if (state?.state === "stopped" && state.error) return { kind: "failed", text: state.error };
  if (!qr) return null;
  const left = qrSecondsLeft(qr.expiresAtMs, nowMs);
  return left > 0 ? { kind: "waiting", text: t("truthId.scan", { seconds: left }) } : { kind: "expired", text: t("truthId.expired") };
}

export interface HubConversationSummary {
  id: string;
  title: string;
  createdAt: number;
  updatedAt: number;
  agentId?: string;
  projectId?: string;
  workdir?: string;
  /** Set on a thread (P125). */
  parent?: { conversationId: string; messageId: string };
  replies?: number;
  /** Absent when not (P157). */
  pinned?: boolean;
  archived?: boolean;
}

export interface HubHistoryMessage {
  /** The hub's id for the message (P125); a hub from before threads sends none. */
  id?: string;
  role: "user" | "assistant";
  content: string;
  createdAt: number;
  attachments?: Attachment[];
}

export interface HubAgent {
  id: string;
  persona: string;
  providerId: string;
  canDelegateToAgents: boolean;
  canManageAgents: boolean;
  canMessageAgents?: boolean;
  canManageTasks?: boolean;
  canStartTasks?: boolean;
  canCreateWorkers?: boolean;
  canMessageUser?: boolean;
  canChooseModels?: boolean;
  allowedTools: string[] | null;
  autonomy?: number;
  approvalRequired?: string[];
  role?: string | null;
  reportsTo?: string | null;
  delegationModels?: string[];
  sharedWith?: string[];
}

export interface HubProject {
  id: string;
  name: string;
  description: string;
  instructions: string;
  workdir?: string;
  code?: boolean;
}

/** A turn that ended in a `chatError`: the hub's own words, the spending limit it stopped on, if any, and whether the
 * hosted credit ran out (P168), which the chat answers with a button to the balance screen. */
export class HubTurnError extends Error {
  spendLimitId?: string;
  topUp: boolean;

  constructor(message: string, spendLimitId?: string, topUp = false) {
    super(message);
    this.name = "HubTurnError";
    this.spendLimitId = spendLimitId;
    this.topUp = topUp;
  }
}

/** The hub's conversation as the list shows it. Its messages come from the history, so those already loaded for the
 * same conversation are kept. */
export function conversationFromSummary(summary: HubConversationSummary, loaded?: Conversation): Conversation {
  return {
    id: summary.id,
    title: summary.title,
    messages: loaded?.messages ?? [],
    createdAt: summary.createdAt,
    updatedAt: summary.updatedAt,
    ...(summary.agentId ? { agentId: summary.agentId } : {}),
    ...(summary.projectId ? { projectId: summary.projectId } : {}),
    ...(summary.workdir ? { workdir: summary.workdir } : {}),
    ...(summary.parent ? { parent: summary.parent, replies: summary.replies ?? 0 } : {}),
    ...(summary.pinned ? { pinned: true } : {}),
    ...(summary.archived ? { archived: true } : {}),
  };
}

/** The list the hub just sent, newest first, keeping the messages already loaded. A conversation this screen made and
 * the hub doesn't list yet (its first turn still running) stays on top. */
export function mergeConversations(previous: Conversation[], summaries: HubConversationSummary[], keepLocal: Set<string> = new Set()): Conversation[] {
  const byId = new Map(previous.map((c) => [c.id, c]));
  const fromHub = summaries.map((s) => conversationFromSummary(s, byId.get(s.id)));
  const listed = new Set(fromHub.map((c) => c.id));
  const notListedYet = previous.filter((c) => keepLocal.has(c.id) && !listed.has(c.id));
  return [...notListedYet, ...fromHub].sort((a, b) => b.updatedAt - a.updatedAt);
}

/** A conversation's messages from the hub's history. The id of a message is the one the hub gave it (P125, kept also as `hubId`,
 * what a thread is started from); a hub from before threads gives none, and then one is made from the conversation and the
 * position: it is the same on every load, which is what `replaceWithSaved`-style merging needs. */
export function messagesFromHistory(conversationId: string, history: HubHistoryMessage[]): ChatMessage[] {
  return history.map((m, index) => ({
    id: m.id ? m.id : `${conversationId}:${index}`,
    ...(m.id ? { hubId: m.id } : {}),
    role: m.role,
    content: m.content,
    createdAt: m.createdAt,
    ...(m.attachments && m.attachments.length > 0 ? { attachments: m.attachments } : {}),
  }));
}

/** What a finished turn adds to its answer that the history doesn't keep: the tokens and the reserve notice. */
export interface TurnExtras {
  usage?: Usage;
  fallbacks?: ProviderFallback[];
}

/** Puts a turn's usage and fallback notice on the last answer of `messages`. */
export function decorateLastAnswer(messages: ChatMessage[], extras: TurnExtras): ChatMessage[] {
  const last = messages.length - 1;
  if (last < 0 || messages[last].role !== "assistant" || (!extras.usage && !extras.fallbacks)) return messages;
  return messages.map((m, i) => (i === last ? { ...m, ...(extras.usage ? { usage: extras.usage } : {}), ...(extras.fallbacks ? { fallbacks: extras.fallbacks } : {}) } : m));
}

/** What a `chat` turn returns: the answer, or the error the hub gave instead. */
export function turnFromReply(reply: unknown): { content: string } & TurnExtras {
  const r = (reply ?? {}) as Record<string, unknown>;
  if (r.type === "chatError") {
    throw new HubTurnError(String(r.message ?? t("hub.couldNotAnswer")), typeof r.spendLimitId === "string" ? r.spendLimitId : undefined, r.topUp === true);
  }
  if (r.type !== "chatResponse") {
    throw new HubTurnError(t("hub.somethingElse", { type: String(r.type) }));
  }
  const usage = r.usage as Usage | null | undefined;
  const fallbacks = r.fallbacks as ProviderFallback[] | undefined;
  return {
    content: String(r.content ?? ""),
    ...(usage ? { usage } : {}),
    ...(fallbacks && fallbacks.length > 0 ? { fallbacks } : {}),
  };
}

/** Throws the hub's refusal (`conversationError`, `projectError`, `settingsError`, `historyError`) as an error; returns
 * the reply otherwise. */
export function expectReply<T extends { type: string }>(reply: unknown, ...types: string[]): T {
  const r = (reply ?? {}) as { type?: string; message?: string };
  if (r.type && types.includes(r.type)) return reply as T;
  if (typeof r.type === "string" && r.type.endsWith("Error")) throw new Error(r.message ?? t("hub.refused"));
  throw new Error(t("hub.somethingElse", { type: String(r.type) }));
}

export function agentFromHub(agent: HubAgent): AgentEntry {
  return {
    id: agent.id,
    persona: agent.persona,
    providerId: agent.providerId,
    canDelegateToAgents: agent.canDelegateToAgents,
    canManageAgents: agent.canManageAgents,
    canMessageAgents: agent.canMessageAgents ?? false,
    canManageTasks: agent.canManageTasks ?? false,
    // On when the hub says nothing: it is what an agent from before these did.
    canStartTasks: agent.canStartTasks ?? true,
    canCreateWorkers: agent.canCreateWorkers ?? true,
    canMessageUser: agent.canMessageUser ?? true,
    canChooseModels: agent.canChooseModels ?? true,
    allowedTools: agent.allowedTools,
    autonomy: agent.autonomy ?? 4,
    approvalRequired: agent.approvalRequired ?? [],
    ...(agent.role ? { role: agent.role } : {}),
    ...(agent.reportsTo ? { reportsTo: agent.reportsTo } : {}),
    ...(agent.sharedWith && agent.sharedWith.length > 0 ? { sharedWith: agent.sharedWith } : {}),
    ...(agent.delegationModels && agent.delegationModels.length > 0 ? { delegationModels: agent.delegationModels } : {}),
  };
}

export function projectFromHub(project: HubProject): ProjectEntry {
  return {
    id: project.id,
    name: project.name,
    description: project.description,
    instructions: project.instructions,
    ...(project.workdir ? { workdir: project.workdir } : {}),
    code: project.code ?? false,
  };
}

/** The `chat` message for a turn: only what the hub reads (no history: it keeps its own; no model: the hub picks it from
 * the agent). A project and a folder are never both, and only the message that creates the conversation carries them. */
export function chatMessage(args: {
  content: string;
  attachments: Attachment[];
  conversationId: string;
  agentId: string;
  projectId: string;
  workdir: string;
  creating: boolean;
  /** The message this conversation is a thread of (P125): carried only by the turn that creates it, and then the thread works in
   * the project and the folder of that conversation, so none is sent here. */
  threadOf?: { conversationId: string; messageId: string };
  /** The message to redo the conversation from (P157): the turn takes the place of that message and of all that followed. */
  replaceFrom?: string;
}): Record<string, unknown> {
  const thread = args.creating ? args.threadOf : undefined;
  return {
    type: "chat",
    message: args.content,
    conversationId: args.conversationId,
    attachments: args.attachments,
    ...(args.agentId ? { agentId: args.agentId } : {}),
    ...(!thread && args.creating && args.projectId ? { projectId: args.projectId } : {}),
    ...(!thread && args.creating && !args.projectId && args.workdir ? { workdir: args.workdir } : {}),
    ...(thread ? { threadOf: thread } : {}),
    ...(args.replaceFrom ? { replaceFrom: args.replaceFrom } : {}),
  };
}

/** The hub's `UsageReportDto`, the part this app reads. */
export interface HubUsageReport {
  total: Usage;
  conversationCount: number;
  messageCount: number;
  limitsEnabled: boolean;
  limits: LimitStatus[];
  recent?: RecentSpend;
  ledgerError?: string;
  /** P126 — tokens by agent, provider and combo; absent from a hub that does not tell them. */
  byAgent?: UsageByKey[];
  byProvider?: UsageByKey[];
  byCombo?: UsageByKey[];
  /** The hosted credit; absent without a `cloud` provider and from a member's report. */
  credit?: Credit;
}

/** The Usage screen's two kinds of numbers out of one report. A hub from before P126 doesn't split tokens by agent, provider
 * or combo (its dollars are split in `recent`), so those lists are empty and the screen hides them. */
export function usageFromReport(report: HubUsageReport): { summary: UsageSummary; spend: SpendStatus } {
  return {
    summary: {
      conversationCount: report.conversationCount,
      messageCount: report.messageCount,
      total: report.total,
      byAgent: report.byAgent ?? [],
      byProvider: report.byProvider ?? [],
      ...(report.byCombo ? { byCombo: report.byCombo } : {}),
      ...(report.credit ? { credit: report.credit } : {}),
    },
    spend: { limitsEnabled: report.limitsEnabled, limits: report.limits, recent: report.recent ?? null, ledgerError: report.ledgerError ?? null },
  };
}

/** What a vault screen rejects with, whichever machine it asks. `conflict`: the note changed since it was opened. */
export interface VaultFailure {
  message: string;
  conflict: boolean;
}

/** Like `expectReply`, but a `vaultError` keeps its `conflict` flag, which the screen turns into "reload or overwrite". */
export function expectVaultReply<T extends { type: string }>(reply: unknown, ...types: string[]): T {
  const r = (reply ?? {}) as { type?: string; message?: string; conflict?: boolean };
  if (r.type === "vaultError") {
    const failure: VaultFailure = { message: r.message ?? t("hub.refused"), conflict: r.conflict === true };
    throw failure;
  }
  return expectReply<T>(reply, ...types);
}
