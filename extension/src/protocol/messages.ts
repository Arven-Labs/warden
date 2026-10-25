/**
 * Mirrors `crates/warden-server-protocol/src/protocol.rs` — now including the tool-call variants
 * (Phase 8.3-8.6), since `Hello.tools` is no longer always empty (see `background/tools/index.ts`).
 * Wire shape: internally-tagged JSON with a `type` field, both the tag and every field name
 * camelCase (`#[serde(tag = "type", rename_all = "camelCase", rename_all_fields =
 * "camelCase")]` on the Rust side) — locked by `protocol.rs`'s own round-trip tests, not guessed.
 */

export interface Usage {
  promptTokens: number;
  completionTokens: number;
  totalTokens: number;
}

export interface Attachment {
  mimeType: string;
  data: string;
}

/** Mirrors `warden_core::tool::ToolSpec` — `parameters` is a raw JSON-Schema object, not a
 * `inputSchema` wrapper. */
export interface ToolSpec {
  name: string;
  description: string;
  parameters: unknown;
}

/** Mirrors `warden_server_protocol::protocol::SkillDto` (P72). `agents` is the agent restriction
 * (empty = every agent) — this client only displays it; an edit that sends `[]` keeps whatever the
 * server has stored. */
export interface SkillDto {
  name: string;
  description: string;
  body: string;
  agents: string[];
  /** P104 — a suggestion the assistant made, still pending. Sending a save without it accepts the skill. */
  proposed?: boolean;
  source?: string;
  proposedAt?: number;
  /** P115: the suggestion is a change to this existing skill; accepting applies it there. */
  revises?: string;
}

/** Mirrors `warden_server_protocol::protocol::CatalogSkillDto` (P140): one ready-made skill of the hub's gallery and where it
 * stands in the person's vault — not there, there and identical, there but different, or there as an untouched older copy of the gallery's
 * text (`outdated`: installing updates it without asking). */
export interface CatalogSkillDto {
  name: string;
  description: string;
  body: string;
  status: "notInstalled" | "installed" | "modified" | "outdated";
}

/** Mirrors `warden_server_protocol::protocol::ConversationSummary` (P78). */
export interface ConversationSummary {
  id: string;
  title: string;
  createdAt: number;
  updatedAt: number;
  /** The agent this conversation last spoke with (P46/P87), restored when it's opened. */
  agentId?: string;
  /** The project (P103) the conversation belongs to; absent outside any project. */
  projectId?: string;
  /** P124 — set when the project is a code project: a turn of it is a task for the code engine, which can be stopped. */
  code?: boolean;
  /** The folder the conversation works in (P102): a path of the hub's machine, or `node:<id>:<path>` on a node. */
  workdir?: string;
  /** P125 — set on a thread: the conversation and the message it came from. */
  parent?: ThreadParent;
  /** P125 — for a thread, how many messages the person sent in it. */
  replies?: number;
  /** P157 — kept at the top of the list; absent when not (or a hub from before it). */
  pinned?: boolean;
  /** P157 — put away, listed apart; absent when not. A conversation is never both pinned and archived. */
  archived?: boolean;
}

/** Mirrors `ThreadParentDto` (P125). */
export interface ThreadParent {
  conversationId: string;
  messageId: string;
}

/** Mirrors `DirEntryDto` (P102): a folder in the folder browser. */
export interface DirEntry {
  name: string;
  path: string;
}

/** What the folder browser shows (`dirList`): `path` is "" for a member's list of allowed folders, `parent` absent at the top. */
export interface DirListing {
  path: string;
  parent?: string;
  dirs: DirEntry[];
}

/** A tool in this device's turn needs the person's yes (P87) — mirrors `ServerMessage::ApprovalRequest`. */
export interface ApprovalPrompt {
  approvalId: number;
  target: string;
  action: string;
  detail: string;
  /** P122 — the kind of action this agent has to get approved (`critical_infra`...), when the ask comes from that rule. */
  category?: string;
}

/** Where a delegated task stands (P123). */
export type AgentTaskState = "pending" | "running" | "waiting" | "paused" | "done" | "failed" | "cancelled";

/** What can be done to a task in progress, from the screen. */
export type AgentTaskAction = "pause" | "resume" | "cancel";

/** Mirrors `ActivityEventDto` (P121): something that happened between the agents, in the activity feed. `kind`: `delegated`, `started`, `done`, `failed`,
 * `cancelled` (the tasks, with `taskId`), `note`, `reply` (agents writing to each other) or `messaged_user` (the agent writing to
 * the person first); the last three carry the `conversationId`. An empty `actor` is the assistant that answers with no agent chosen. */
export interface ActivityEvent {
  id: string;
  atMs: number;
  kind: string;
  actor: string;
  target?: string;
  text: string;
  taskId?: string;
  conversationId?: string;
}

/** Mirrors `GrantDto` (P122): an "always" given in a conversation. `covers` is a risk category (`delete_data`) or a tool name; with no `agentId`, the default agent.
 * `lastUsedMs`: when it was given or when it last spared the agent from asking (absent on an older hub). */
export interface Grant {
  conversationId: string;
  agentId?: string;
  covers: string;
  lastUsedMs?: number;
}

/** The "always" grants given and after how many unused days one expires. `ttlDays` is missing when the hub does not keep them in a file (they end
 * with the connection) and on an older hub. */
export interface GrantsView {
  grants: Grant[];
  ttlDays?: number;
}

/** Mirrors `AgentTaskDto` (P123): a task an agent delegated in the background. `group` is shared by the tasks one turn started. */
export interface AgentTask {
  id: string;
  group: string;
  /** The agent that delegated. */
  owner?: string | null;
  /** Who does the work, or the name given to a temporary helper. */
  assignee: string;
  /** The task this is a subtask of, when its agent started it from inside another task. */
  parentId?: string | null;
  objective: string;
  model?: string | null;
  channel: string;
  state: AgentTaskState;
  result?: string | null;
  error?: string | null;
  promptTokens?: number | null;
  completionTokens?: number | null;
  totalTokens?: number | null;
  createdAtMs: number;
  startedAtMs?: number | null;
  finishedAtMs?: number | null;
  /** Runs in the process of the hub that answered: it can be paused, resumed or stopped from here. */
  controllable?: boolean;
  /** Among those, the ones that can also be paused: a delegation the agent is waiting on can only be stopped. */
  pausable?: boolean;
}

/** What the Organization screen reads of an agent in the hub's settings (P120, P123): the rest of `AgentSettings` is left out. */
export interface AgentInfo {
  id: string;
  role?: string | null;
  reportsTo?: string | null;
  canDelegateToAgents: boolean;
  canManageAgents: boolean;
  canMessageAgents: boolean;
  canManageTasks: boolean;
  /** P122 — on by default; when off, the agent does not start background work, does not create temporary agents, does not start conversations and does not choose the tasks' model. */
  canStartTasks: boolean;
  canCreateWorkers: boolean;
  canMessageUser: boolean;
  canChooseModels: boolean;
  /** 1 only answers, 2 suggests, 3 asks before every change, 4 acts alone, 5 also manages its subordinates without asking. */
  autonomy: number;
  approvalRequired: string[];
  /** The models the agent may choose when delegating; empty = open. */
  delegationModels: string[];
}

/** Mirrors `ModelPolicyDto` (P123): a name ("fast", "reasoning"...) that a delegating agent uses for a task's model. */
export interface ModelPolicy {
  id: string;
  model: string;
  description?: string;
}

/** A change made through the tree, as the hub receives it (`AgentOrgEdit`). An absent `role` or `reportsTo` means none. */
export type OrgEdit =
  | { kind: "setPosition"; id: string; role?: string; reportsTo?: string }
  | { kind: "addReport"; id: string; persona: string; role?: string; reportsTo?: string }
  | { kind: "remove"; id: string }
  /** P123 — the models `id` may pick when it delegates, in order (the first is the default; empty leaves the choice open). */
  | { kind: "setDelegationModels"; id: string; models: string[] }
  /** P123 — replaces the named model policies with this list. */
  | { kind: "setModelPolicies"; policies: Array<{ id: string; model: string; description: string }> }
  /** P122 — what `id` may do on its own, all at once: the autonomy level (1 to 5), the four permissions, and the kinds of action (ids,
   * e.g. `spend_money`) that always need the person's yes. Only the owner makes this edit. */
  | {
      kind: "setPowers";
      id: string;
      autonomy: number;
      canStartTasks: boolean;
      canCreateWorkers: boolean;
      canMessageUser: boolean;
      canChooseModels: boolean;
      approvalRequired: string[];
    };

/** What the hub's settings say about the agents (P87, P120, P123), reduced to what this client shows. */
export interface HubAgents {
  agents: AgentInfo[];
  modelPolicies: ModelPolicy[];
  /** The ids of the providers and combos: what a policy can be answered by, and what an agent can be limited to besides the policies. */
  modelIds: string[];
}

/** Mirrors `warden_server_protocol::protocol::HistoryMessage` (P40). */
export interface HistoryMessage {
  /** P125 — what a thread is attached to; absent on a hub from before threads. */
  id?: string;
  role: "user" | "assistant";
  content: string;
  createdAt: number;
  attachments: Attachment[];
}

/** P36 — an invite that waits to be used. The hub never sends its code. */
export interface DeviceInviteInfo {
  name: string;
  createdMs: number;
  expiresMs: number;
}

export type ClientMessage =
  /** `authKey` is the hub's pairing key (P36), only needed until this device holds a
   * `deviceToken` from an earlier `helloAck`. */
  | { type: "hello"; deviceId: string; deviceName: string; authKey: string; deviceToken?: string; tools: ToolSpec[] }
  | { type: "ping"; nonce: number }
  /** `conversationId` (P78) picks one of this device's conversations — a new id starts a new one;
   * omitted, the turn goes to the device's default conversation. */
  | { type: "chat"; message: string; conversationId?: string; agentId?: string; workdir?: string; threadOf?: ThreadParent; replaceFrom?: string }
  /** P124, P157 — stops the turn a conversation is running, a code task or an ordinary turn; the work so far is kept (for an ordinary turn, what the model had written is the answer) and the turn ends with the usual answer. */
  | { type: "cancelTurn"; conversationId: string }
  /** P102 — the folders inside `path` on the hub's machine (or a node's, `node:<id>:<path>`); no `path`: where the person starts. Answered by `dirList`/`dirError`. */
  | { type: "listDirs"; requestId: number; path?: string }
  /** P87 — only for the configured agents' ids, as the web's selector does. */
  | { type: "requestSettings"; requestId: number }
  | { type: "resolveApproval"; approvalId: number; approved: boolean }
  /** P123 — the work agents delegated to each other, answered by `agentTaskList`. */
  | { type: "listAgentTasks"; requestId: number }
  /** P123 — the usage and spending report (the extension reads each agent's spending in it), answered by `usageReport`. The hub wants the offset
   * from UTC of the person's day, in minutes. */
  | { type: "requestUsage"; requestId: number; tzOffsetMinutes: number }
  /** P122 — what the person said "always" to on this connection, answered by `grantList`; `revokeGrant` takes one back and is answered by the new list. */
  | { type: "listGrants"; requestId: number }
  | { type: "revokeGrant"; requestId: number; conversationId: string; agentId?: string; covers: string }
  /** P121 — the feed of activity, answered by `activityList`. Owner only; a member gets it empty. */
  | { type: "listActivity"; requestId: number }
  /** P123 — pauses, resumes or stops a task running on the hub (and the subtasks below it). Asks for the pairing key; answered by
   * `agentTaskList` or `taskError`. */
  | { type: "controlAgentTask"; requestId: number; pairingKey: string; taskId: string; action: AgentTaskAction }
  /** P120 — one change to the agents' organization. Asks for the pairing key; answered by `settingsSaved` or `settingsError`. */
  | { type: "editAgentOrg"; requestId: number; pairingKey: string; edit: OrgEdit }
  /** P36 — a one-use invite that pairs one device without the pairing key. Asks for the pairing key; answered by `deviceInvite` or `deviceError`. */
  | { type: "createDeviceInvite"; requestId: number; pairingKey: string; name: string; ttlMinutes: number }
  /** P36 — changes the hub's pairing key. Asks for the current one; answered by `pairingKeyRotated` or `deviceError`. */
  | { type: "rotatePairingKey"; requestId: number; pairingKey: string }
  /** P36 — the invites that wait (open to a paired device), answered by `deviceInviteList`; cancelling one asks for the pairing key. */
  | { type: "listDeviceInvites"; requestId: number }
  | { type: "cancelDeviceInvite"; requestId: number; pairingKey: string; name: string }
  | { type: "toolCallResult"; callId: number; result: unknown }
  | { type: "toolCallError"; callId: number; message: string }
  /** Skills management (P72) — `requestId` is echoed on the matching reply. */
  | { type: "listSkills"; requestId: number }
  | { type: "saveSkill"; requestId: number; skill: SkillDto; overwrite: boolean }
  | { type: "deleteSkill"; requestId: number; name: string }
  /** P140 — the gallery of ready-made skills; a skill that differs from the one in the vault is only replaced with `overwrite`. */
  | { type: "listSkillCatalog"; requestId: number }
  | { type: "installCatalogSkill"; requestId: number; name: string; overwrite: boolean }
  /** P40 — this device's persisted conversation, answered by `history`/`historyError` with the same
   * `requestId`. `limit` keeps only the most recent messages. */
  | { type: "requestHistory"; requestId: number; limit?: number; conversationId?: string }
  /** P78 — this device's conversations, answered by `conversationList`/`conversationOk`/`conversationError`. */
  | { type: "listConversations"; requestId: number }
  /** P121 — the id of an agent's channel, the one conversation it keeps with the person; answered by `agentChannel`/`conversationError`. */
  | { type: "openAgentChannel"; requestId: number; agentId: string }
  | { type: "renameConversation"; requestId: number; conversationId: string; title: string }
  | { type: "deleteConversation"; requestId: number; conversationId: string }
  /** P157 — pins or archives a conversation; a flag left out stays as it is. Archiving unpins, and pinning an archived one brings it
   * back. Answered by `conversationOk`/`conversationError`. */
  | { type: "setConversationFlags"; requestId: number; conversationId: string; pinned?: boolean; archived?: boolean }
  /** Phase 9.1 (redefined) — an unauthenticated presence probe, answered by `discoverAck` below.
   * No `authKey`/`deviceId` on purpose: the point is finding a hub before knowing its credential. */
  | { type: "discover" }
  | { type: "goodbye"; reason: string | null };

export function encode(message: ClientMessage): string {
  return JSON.stringify(message);
}

/** Mirrors `ChatEventDto` (P103 b, P124): what a code engine, or the tasks a manager gave it, does while a turn runs. The
 * turn's end is still `chatResponse`/`chatError`. */
export type ChatEventDto =
  /** `source` names the task the words are from when several are told at once. */
  | { type: "text"; text: string; source?: string }
  /** `callId` is the same for each stage of one call, so a client updates one line instead of adding one. */
  | { type: "tool"; callId: string; tool: string; title: string; status: "running" | "completed" | "failed" }
  | { type: "notice"; text: string };

/** P123 — the part of the hub's usage report the extension reads: what each agent spent in the ledger's window. An older hub sends no
 * `recent`, or none of `byAgent`; an empty `key` is a call with no agent chosen. */
export interface SpendReport {
  recent?: {
    windowHours: number;
    byAgent?: { key: string; costUsd: number; unpricedCalls: number }[];
    /** P126 — the dollars of the calls made through a combo; there is no bucket for the ones that were not. */
    byCombo?: { key: string; costUsd: number; unpricedCalls: number }[];
  } | null;
  /** P126 — the tokens of every conversation the hub keeps, and by agent, provider and combo; an older hub sends no breakdown. */
  total?: { totalTokens: number };
  messageCount?: number;
  byAgent?: UsageByKey[];
  byProvider?: UsageByKey[];
  byCombo?: UsageByKey[];
}

/** P126 — one bucket of a token breakdown. A `null` key is "no agent" / "the default provider". */
export interface UsageByKey {
  key: string | null;
  messageCount: number;
  usage: { totalTokens: number };
}

export type ServerMessage =
  /** `deviceToken` is present when this Hello paired with the pairing key (P36) — it replaces
   * whatever token this device held for the hub. */
  | { type: "helloAck"; serverName: string; deviceToken?: string }
  | { type: "authError"; reason: string }
  | { type: "pong"; nonce: number }
  /** `conversationId` (P78) — which conversation this answers; `chat` has no `requestId`. */
  | { type: "chatResponse"; content: string; usage: Usage | null; attachments: Attachment[]; conversationId?: string }
  | { type: "chatError"; message: string; conversationId?: string }
  | { type: "chatEvent"; conversationId: string; event: ChatEventDto }
  | { type: "toolCallRequest"; callId: number; tool: string; arguments: unknown }
  | { type: "skillList"; requestId: number; skills: SkillDto[] }
  | { type: "skillCatalog"; requestId: number; entries: CatalogSkillDto[] }
  | { type: "skillOk"; requestId: number }
  | { type: "skillError"; requestId: number; message: string }
  | { type: "history"; requestId: number; messages: HistoryMessage[] }
  | { type: "historyError"; requestId: number; message: string }
  | { type: "conversationList"; requestId: number; conversations: ConversationSummary[] }
  | { type: "conversationOk"; requestId: number }
  | { type: "agentChannel"; requestId: number; conversationId: string }
  | { type: "conversationError"; requestId: number; message: string }
  | { type: "dirList"; requestId: number; path: string; parent?: string; dirs: DirEntry[] }
  | { type: "dirError"; requestId: number; message: string }
  /** Reply to `requestSettings`, reduced to what this client uses (P87). */
  | ({ type: "settings"; requestId: number; agentIds: string[] } & HubAgents)
  /** Reply to a successful `editAgentOrg`, with the agents as the file holds them now. */
  | ({ type: "settingsSaved"; requestId: number } & HubAgents)
  /** `authRejected`: the pairing key was wrong; nothing was written. */
  | { type: "settingsError"; requestId: number; message: string; authRejected?: boolean }
  | { type: "agentTaskList"; requestId: number; tasks: AgentTask[] }
  /** Reply to `requestUsage`; only the part the extension reads is typed. */
  | { type: "usageReport"; requestId: number; report: SpendReport }
  | { type: "grantList"; requestId: number; grants: Grant[]; ttlDays?: number }
  | { type: "activityList"; requestId: number; events: ActivityEvent[] }
  | { type: "taskError"; requestId: number; message: string; authRejected?: boolean }
  /** Reply to `createDeviceInvite`: the `code` is shown once, the hub keeps only its hash. */
  | { type: "deviceInvite"; requestId: number; name: string; code: string; expiresMs: number }
  /** Reply to `rotatePairingKey`: the new key, shown once. */
  | { type: "pairingKeyRotated"; requestId: number; newKey: string }
  /** Reply to `listDeviceInvites` and `cancelDeviceInvite`: the invites that wait, by name; never a code. */
  | { type: "deviceInviteList"; requestId: number; invites: DeviceInviteInfo[] }
  /** `authRejected`: the pairing key was wrong; nothing was changed. */
  | { type: "deviceError"; requestId: number; message: string; authRejected?: boolean }
  | ({ type: "approvalRequest" } & ApprovalPrompt)
  | { type: "approvalCancelled"; approvalId: number }
  /** An agent left a note in one of this device's conversations, or answered one (P46 `message_agent`). */
  | { type: "conversationsChanged"; conversationId: string }
  /** Reply to `ClientMessage.discover` — just enough to let the operator recognize which machine
   * this is, never a secret. */
  /** `secureUrl` — set by a TLS-only hub (P36): the wss:// URL to connect to instead. */
  | { type: "discoverAck"; serverName: string; secureUrl?: string }
  | { type: "goodbye"; reason: string | null };

type RawAgent = Partial<Omit<AgentInfo, "id">> & { id: string };
interface RawSettings {
  agents?: RawAgent[];
  modelPolicies?: ModelPolicy[];
  providers?: Array<{ id: string }>;
  combos?: Array<{ id: string }>;
}

/** The agents and model policies out of a settings payload, with the optional fields this client reads filled in. */
function hubAgents(settings: RawSettings): HubAgents {
  return {
    agents: (settings.agents ?? []).map((a) => ({
      id: a.id,
      role: a.role ?? null,
      reportsTo: a.reportsTo ?? null,
      canDelegateToAgents: a.canDelegateToAgents ?? false,
      canManageAgents: a.canManageAgents ?? false,
      canMessageAgents: a.canMessageAgents ?? false,
      canManageTasks: a.canManageTasks ?? false,
      canStartTasks: a.canStartTasks ?? true,
      canCreateWorkers: a.canCreateWorkers ?? true,
      canMessageUser: a.canMessageUser ?? true,
      canChooseModels: a.canChooseModels ?? true,
      autonomy: a.autonomy ?? 4,
      approvalRequired: a.approvalRequired ?? [],
      delegationModels: a.delegationModels ?? [],
    })),
    modelPolicies: settings.modelPolicies ?? [],
    modelIds: [...(settings.providers ?? []), ...(settings.combos ?? [])].map((m) => m.id),
  };
}

/**
 * Decodes one `ServerMessage`. Throws on anything this client doesn't understand — explicit is
 * better than a silent `as` cast producing a message shape this client can't actually handle.
 */
export function decode(text: string): ServerMessage {
  const json = JSON.parse(text) as { type?: unknown };
  switch (json.type) {
    case "helloAck":
    case "authError":
    case "pong":
    case "chatError":
    case "chatEvent":
    case "discoverAck":
    case "goodbye":
      return json as ServerMessage;
    case "chatResponse": {
      const raw = json as { content: string; usage: Usage | null; attachments?: Attachment[]; conversationId?: string };
      return { type: "chatResponse", content: raw.content, usage: raw.usage, attachments: raw.attachments ?? [], conversationId: raw.conversationId };
    }
    case "skillList": {
      const raw = json as { requestId: number; skills: Array<Omit<SkillDto, "agents"> & { agents?: string[] }> };
      return { type: "skillList", requestId: raw.requestId, skills: raw.skills.map((skill) => ({ ...skill, agents: skill.agents ?? [] })) };
    }
    case "settings": {
      const raw = json as { requestId: number; settings: RawSettings };
      return { type: "settings", requestId: raw.requestId, agentIds: (raw.settings.agents ?? []).map((a) => a.id), ...hubAgents(raw.settings) };
    }
    case "settingsSaved": {
      const raw = json as { requestId: number; settings: RawSettings };
      return { type: "settingsSaved", requestId: raw.requestId, ...hubAgents(raw.settings) };
    }
    case "agentTaskList":
    case "usageReport":
    case "grantList":
    case "activityList":
    case "deviceInvite":
    case "pairingKeyRotated":
    case "deviceInviteList":
    case "deviceError":
    case "taskError":
    case "settingsError":
    case "approvalRequest":
    case "approvalCancelled":
    case "conversationsChanged":
    case "skillCatalog":
    case "skillOk":
    case "skillError":
    case "historyError":
    case "conversationList":
    case "conversationOk":
    case "agentChannel":
    case "conversationError":
    case "dirList":
    case "dirError":
      return json as ServerMessage;
    case "history": {
      const raw = json as { requestId: number; messages: Array<Omit<HistoryMessage, "attachments"> & { attachments?: Attachment[] }> };
      return {
        type: "history",
        requestId: raw.requestId,
        messages: raw.messages.map((m) => ({ ...m, attachments: m.attachments ?? [] })),
      };
    }
    case "toolCallRequest": {
      const raw = json as { callId: number; tool: string; arguments: unknown };
      return { type: "toolCallRequest", callId: raw.callId, tool: raw.tool, arguments: raw.arguments };
    }
    default:
      throw new Error(`unknown or unsupported ServerMessage type: ${String(json.type)}`);
  }
}
