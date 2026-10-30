import 'dart:convert';

/// Mirrors `crates/warden-server/src/protocol.rs`. Messages sent from this
/// client to a warden-server, over the WS + JSON protocol (Phase 9.2 / 7.2).
///
/// Wire shape: internally-tagged JSON with a `type` field, both the tag and
/// every field name are camelCase (`#[serde(tag = "type", rename_all =
/// "camelCase", rename_all_fields = "camelCase")]` on the Rust side).
sealed class ClientMessage {
  const ClientMessage();

  Map<String, dynamic> toJson();

  String encode() => jsonEncode(toJson());
}

final class HelloMessage extends ClientMessage {
  const HelloMessage({
    required this.deviceId,
    required this.deviceName,
    required this.authKey,
    this.deviceToken,
    this.tools = const [],
    this.username,
    this.password,
    this.recoveryCodes = true,
  });

  final String deviceId;
  final String deviceName;

  /// The hub's pairing key (P36) — only needed until this device holds a [deviceToken] for it.
  final String authKey;

  /// The per-device token this hub issued in an earlier `HelloAck` (P36), if any.
  final String? deviceToken;

  /// Local tools this client can run on request (Phase 7.4), e.g.
  /// `MobileFileTool.listFilesSpec`/`readFileSpec` — each a `{name, description, parameters}`
  /// map mirroring `warden_core::tool::ToolSpec`. Empty when nothing is configured (e.g. no
  /// root folder picked yet) — `warden-server` only builds the remote-tool-dispatch machinery
  /// when this is non-empty.
  final List<Map<String, dynamic>> tools;

  /// P84 — pairs as this member (with [password]) instead of with the pairing key. Like the key,
  /// only needed until the hub issues a token.
  final String? username;
  final String? password;

  /// P84 slice 4 — this app can show a recovery code, so the hub may turn a member's encryption on
  /// (a code nobody sees is a key nobody has). Always true: the screens for it are here.
  final bool recoveryCodes;

  @override
  Map<String, dynamic> toJson() => {
        'type': 'hello',
        'deviceId': deviceId,
        'deviceName': deviceName,
        'authKey': authKey,
        if (deviceToken != null) 'deviceToken': deviceToken,
        'tools': tools,
        if (username != null) 'username': username,
        if (username != null) 'password': password ?? '',
        if (recoveryCodes) 'recoveryCodes': true,
      };
}

/// P84 — the member on this connection picks their own password. Answered by
/// [PasswordChangedMessage] or [UserErrorMessage].
final class ChangePasswordMessage extends ClientMessage {
  const ChangePasswordMessage(this.requestId, this.oldPassword, this.newPassword, {this.recoveryCode});

  final int requestId;
  final String oldPassword;
  final String newPassword;

  /// P84 slice 4 — after the owner reset the password of someone whose data is encrypted, only the
  /// recovery code opens it again.
  final String? recoveryCode;

  @override
  Map<String, dynamic> toJson() => {
        'type': 'changePassword',
        'requestId': requestId,
        'oldPassword': oldPassword,
        'newPassword': newPassword,
        if (recoveryCode != null) 'recoveryCode': recoveryCode,
      };
}

/// P84 slice 4 — a new recovery code (the old one stops working), with the password. Answered by
/// [RecoveryCodeMessage] or [UserErrorMessage].
final class RegenerateRecoveryCodeMessage extends ClientMessage {
  const RegenerateRecoveryCodeMessage(this.requestId, this.password);

  final int requestId;
  final String password;

  @override
  Map<String, dynamic> toJson() => {'type': 'regenerateRecoveryCode', 'requestId': requestId, 'password': password};
}

/// P84 slice 4 parte B — yes to the workspace's recovery policy after a change to a weaker one, with
/// the password. Answered by [RecoveryPolicyAcceptedMessage] or [UserErrorMessage].
final class AcceptRecoveryPolicyMessage extends ClientMessage {
  const AcceptRecoveryPolicyMessage(this.requestId, this.password);

  final int requestId;
  final String password;

  @override
  Map<String, dynamic> toJson() => {'type': 'acceptRecoveryPolicy', 'requestId': requestId, 'password': password};
}

/// P84 slice 4 parte B — the member has seen the recoveries the owner made. Answered by
/// [RecoveryNoticesAckedMessage].
final class AckRecoveryNoticesMessage extends ClientMessage {
  const AckRecoveryNoticesMessage(this.requestId);

  final int requestId;

  @override
  Map<String, dynamic> toJson() => {'type': 'ackRecoveryNotices', 'requestId': requestId};
}

/// P84 slice 5 — links the member's TruthID with the owner's invite [code]. Answered by
/// [TruthIdLinkedMessage] or [UserErrorMessage].
final class RedeemInviteMessage extends ClientMessage {
  const RedeemInviteMessage(this.requestId, this.code, this.username);

  final int requestId;
  final String code;
  final String username;

  @override
  Map<String, dynamic> toJson() => {'type': 'redeemInvite', 'requestId': requestId, 'code': code, 'username': username};
}

final class PingMessage extends ClientMessage {
  const PingMessage(this.nonce);

  final int nonce;

  @override
  Map<String, dynamic> toJson() => {'type': 'ping', 'nonce': nonce};
}

final class GoodbyeMessage extends ClientMessage {
  const GoodbyeMessage([this.reason]);

  final String? reason;

  @override
  Map<String, dynamic> toJson() => {'type': 'goodbye', 'reason': reason};
}

/// A chat turn (Phase 7.3) — answered by the `Orchestrator` `warden-server` hosts, appended to one of
/// this device's conversations. [conversationId] picks which (P78): an id the hub has never seen
/// starts a new one; null is the device's default conversation. [agentId] (P46/P87) speaks as that
/// configured agent: its persona, skills and tools. [workdir] (P102) is the folder of the hub's machine
/// the new conversation works in — only read when the conversation is created, so it travels with its
/// first message.
final class ChatMessage extends ClientMessage {
  const ChatMessage(this.message, {this.conversationId, this.agentId, this.workdir, this.threadOf, this.replaceFrom});

  final String message;
  final String? conversationId;
  final String? agentId;
  final String? workdir;

  /// P125 — makes the conversation a thread of this message, when the hub creates it.
  final ThreadParent? threadOf;

  /// P157 — the id of the message this turn takes the place of, with everything after it: "regenerate" sends the last message of the
  /// person's again, "edit" sends it changed. The hub refuses it for a thread or a code project.
  final String? replaceFrom;

  @override
  Map<String, dynamic> toJson() => {
        'type': 'chat',
        'message': message,
        if (conversationId != null) 'conversationId': conversationId,
        if (agentId != null) 'agentId': agentId,
        if (workdir != null) 'workdir': workdir,
        if (threadOf != null) 'threadOf': threadOf!.toJson(),
        if (replaceFrom != null) 'replaceFrom': replaceFrom,
      };
}

/// P124, P157 — stops the turn a conversation is running: a code conversation's task, or an ordinary turn (the Stop button). The work
/// so far is kept — for an ordinary turn, what the model had written is the answer — and the turn ends with the usual reply;
/// harmless for a conversation that isn't running one.
final class CancelTurnMessage extends ClientMessage {
  const CancelTurnMessage(this.conversationId);

  final String conversationId;

  @override
  Map<String, dynamic> toJson() => {'type': 'cancelTurn', 'conversationId': conversationId};
}

/// P102 — asks for the folders inside [path] on the hub's machine; no path starts where the person may
/// (a member sees only the folders the owner allowed). Answered by [DirListMessage] or [DirErrorMessage]
/// with the same `requestId`.
final class ListDirsMessage extends ClientMessage {
  const ListDirsMessage(this.requestId, {this.path});

  final int requestId;
  final String? path;

  @override
  Map<String, dynamic> toJson() => {'type': 'listDirs', 'requestId': requestId, if (path != null) 'path': path};
}

/// P102 — asks for the nodes (machines other than the hub) so the owner can pick a folder on one. Answered by [NodeListMessage]
/// or [NodeErrorMessage] with the same `requestId`.
final class ListNodesMessage extends ClientMessage {
  const ListNodesMessage(this.requestId);

  final int requestId;

  @override
  Map<String, dynamic> toJson() => {'type': 'listNodes', 'requestId': requestId};
}

/// Asks for the hub's settings (P78) — here only for the configured agents' ids (P87), the same
/// way the web's agent selector gets them. Answered by [SettingsMessage] or [SettingsErrorMessage].
final class RequestSettingsMessage extends ClientMessage {
  const RequestSettingsMessage(this.requestId);

  final int requestId;

  @override
  Map<String, dynamic> toJson() => {'type': 'requestSettings', 'requestId': requestId};
}

/// P123 — the work agents delegated to each other in the background, newest first. Answered by
/// [AgentTaskListMessage].
final class ListAgentTasksMessage extends ClientMessage {
  const ListAgentTasksMessage(this.requestId);

  final int requestId;

  @override
  Map<String, dynamic> toJson() => {'type': 'listAgentTasks', 'requestId': requestId};
}

/// P123 — the usage and spending report; the app reads each agent's spending in it. [tzOffsetMinutes] is the offset from UTC of
/// the person's day (the hub splits days with it). Answered by [UsageReportMessage].
final class RequestUsageMessage extends ClientMessage {
  const RequestUsageMessage(this.requestId, this.tzOffsetMinutes);

  final int requestId;
  final int tzOffsetMinutes;

  @override
  Map<String, dynamic> toJson() => {'type': 'requestUsage', 'requestId': requestId, 'tzOffsetMinutes': tzOffsetMinutes};
}

/// P122 — what the person said "always" to on this connection. Answered by [GrantListMessage].
final class ListGrantsMessage extends ClientMessage {
  const ListGrantsMessage(this.requestId);

  final int requestId;

  @override
  Map<String, dynamic> toJson() => {'type': 'listGrants', 'requestId': requestId};
}

/// P122 — takes one "always" back, so the agent asks again. No pairing key: it only makes the agent more careful. Answered by the
/// new [GrantListMessage].
final class RevokeGrantMessage extends ClientMessage {
  const RevokeGrantMessage(this.requestId, this.grant);

  final int requestId;
  final Grant grant;

  @override
  Map<String, dynamic> toJson() => {
        'type': 'revokeGrant',
        'requestId': requestId,
        'conversationId': grant.conversationId,
        if (grant.agentId != null) 'agentId': grant.agentId,
        'covers': grant.covers,
      };
}

/// P121 — the feed of activity: what happened among the agents, newest first. Answered by [ActivityListMessage]. Owner only; a
/// member gets it empty.
final class ListActivityMessage extends ClientMessage {
  const ListActivityMessage(this.requestId);

  final int requestId;

  @override
  Map<String, dynamic> toJson() => {'type': 'listActivity', 'requestId': requestId};
}

/// P123 — pauses, resumes or stops ([action]: `pause`, `resume` or `cancel`) a task running on the hub, with the
/// subtasks below it. Asks for the pairing key every time. Answered by [AgentTaskListMessage] or [TaskErrorMessage].
final class ControlAgentTaskMessage extends ClientMessage {
  const ControlAgentTaskMessage(this.requestId, this.pairingKey, this.taskId, this.action);

  final int requestId;
  final String pairingKey;
  final String taskId;
  final String action;

  @override
  Map<String, dynamic> toJson() => {
        'type': 'controlAgentTask',
        'requestId': requestId,
        'pairingKey': pairingKey,
        'taskId': taskId,
        'action': action,
      };
}

/// P36 — a one-use invite that pairs one device without the pairing key. Asks for the pairing key every time. Answered by
/// [DeviceInviteMessage] or [DeviceErrorMessage].
final class CreateDeviceInviteMessage extends ClientMessage {
  const CreateDeviceInviteMessage(this.requestId, this.pairingKey, this.name, this.ttlMinutes);

  final int requestId;
  final String pairingKey;
  final String name;
  final int ttlMinutes;

  @override
  Map<String, dynamic> toJson() => {
        'type': 'createDeviceInvite',
        'requestId': requestId,
        'pairingKey': pairingKey,
        'name': name,
        'ttlMinutes': ttlMinutes,
      };
}

/// P36 — the invites that wait to be used. Open to a paired device (it shows names and deadlines, never a code). Answered by
/// [DeviceInviteListMessage].
final class ListDeviceInvitesMessage extends ClientMessage {
  const ListDeviceInvitesMessage(this.requestId);

  final int requestId;

  @override
  Map<String, dynamic> toJson() => {'type': 'listDeviceInvites', 'requestId': requestId};
}

/// P36 — cancels the waiting invite called [name]. Asks for the pairing key every time. Answered by [DeviceInviteListMessage]
/// or [DeviceErrorMessage].
final class CancelDeviceInviteMessage extends ClientMessage {
  const CancelDeviceInviteMessage(this.requestId, this.pairingKey, this.name);

  final int requestId;
  final String pairingKey;
  final String name;

  @override
  Map<String, dynamic> toJson() => {'type': 'cancelDeviceInvite', 'requestId': requestId, 'pairingKey': pairingKey, 'name': name};
}

/// P36 — changes the hub's pairing key. Asks for the current one. Answered by [PairingKeyRotatedMessage] or
/// [DeviceErrorMessage].
final class RotatePairingKeyMessage extends ClientMessage {
  const RotatePairingKeyMessage(this.requestId, this.pairingKey);

  final int requestId;
  final String pairingKey;

  @override
  Map<String, dynamic> toJson() => {'type': 'rotatePairingKey', 'requestId': requestId, 'pairingKey': pairingKey};
}

/// P120 — one change to the organization of the agents, with the pairing key. Answered by [SettingsSavedMessage] or
/// [SettingsErrorMessage].
final class EditAgentOrgMessage extends ClientMessage {
  const EditAgentOrgMessage(this.requestId, this.pairingKey, this.edit);

  /// A member the owner gave the `edit` access sends no key: their session is the authorization. They are answered by
  /// [AgentOrgListMessage].
  const EditAgentOrgMessage.asMember(this.requestId, this.edit) : pairingKey = null;

  final int requestId;
  final String? pairingKey;
  final OrgEdit edit;

  @override
  Map<String, dynamic> toJson() => {'type': 'editAgentOrg', 'requestId': requestId, if (pairingKey != null) 'pairingKey': pairingKey, 'edit': edit.toJson()};
}

/// P120 — the tree of the organization, for the owner or a member whose access is `view` or `edit`. Answered by
/// [AgentOrgListMessage], or by [SettingsErrorMessage] when the owner gave no access.
final class ListAgentOrgMessage extends ClientMessage {
  const ListAgentOrgMessage(this.requestId);

  final int requestId;

  @override
  Map<String, dynamic> toJson() => {'type': 'listAgentOrg', 'requestId': requestId};
}

/// P72 — asks the hub's model to write a draft of a skill from [description]. Nothing is saved; it spends like a chat
/// turn. Answered by [SkillDraftMessage], or by [SkillErrorMessage].
final class DraftSkillMessage extends ClientMessage {
  const DraftSkillMessage(this.requestId, this.description);

  final int requestId;
  final String description;

  @override
  Map<String, dynamic> toJson() => {'type': 'draftSkill', 'requestId': requestId, 'description': description};
}

/// One change made from the organization tree (P120), as the hub receives it (`AgentOrgEdit`). A blank [role] or
/// [reportsTo] means none and is left out.
sealed class OrgEdit {
  const OrgEdit();

  Map<String, dynamic> toJson();
}

/// Gives [id] this role and this superior (moving it, with everyone below it, when the superior changes).
final class SetPositionEdit extends OrgEdit {
  const SetPositionEdit(this.id, {this.role, this.reportsTo});

  final String id;
  final String? role;
  final String? reportsTo;

  @override
  Map<String, dynamic> toJson() => {
        'kind': 'setPosition',
        'id': id,
        if (role != null && role!.trim().isNotEmpty) 'role': role!.trim(),
        if (reportsTo != null && reportsTo!.isNotEmpty) 'reportsTo': reportsTo,
      };
}

/// A new agent under [reportsTo] (or at the top), careful by default (the hub decides what that means).
final class AddReportEdit extends OrgEdit {
  const AddReportEdit(this.id, this.persona, {this.role, this.reportsTo});

  final String id;
  final String persona;
  final String? role;
  final String? reportsTo;

  @override
  Map<String, dynamic> toJson() => {
        'kind': 'addReport',
        'id': id.trim(),
        'persona': persona.trim(),
        if (role != null && role!.trim().isNotEmpty) 'role': role!.trim(),
        if (reportsTo != null && reportsTo!.isNotEmpty) 'reportsTo': reportsTo,
      };
}

/// Removes [id]; whoever reported to it reports to its superior.
final class RemoveAgentEdit extends OrgEdit {
  const RemoveAgentEdit(this.id);

  final String id;

  @override
  Map<String, dynamic> toJson() => {'kind': 'remove', 'id': id};
}

/// P123 — the models [id] may pick when it delegates, in order: the first is the default of a delegation that names none, so a
/// list of one dictates the model. Empty leaves the choice open.
final class SetDelegationModelsEdit extends OrgEdit {
  const SetDelegationModelsEdit(this.id, this.models);

  final String id;
  final List<String> models;

  @override
  Map<String, dynamic> toJson() => {'kind': 'setDelegationModels', 'id': id, 'models': models};
}

/// P123 — replaces the named model policies with [policies].
final class SetModelPoliciesEdit extends OrgEdit {
  const SetModelPoliciesEdit(this.policies);

  final List<ModelPolicy> policies;

  @override
  Map<String, dynamic> toJson() => {
        'kind': 'setModelPolicies',
        'policies': [
          for (final p in policies) {'id': p.id, 'model': p.model, 'description': p.description ?? ''},
        ],
      };
}

/// P122 — what [id] may do on its own, all at once: the autonomy level (1 to 5), the four permissions and the kinds of action
/// ([approvalRequired], ids such as `spend_money`) that always need the person's yes. Only the owner makes this edit.
final class SetPowersEdit extends OrgEdit {
  const SetPowersEdit(
    this.id, {
    required this.autonomy,
    required this.canStartTasks,
    required this.canCreateWorkers,
    required this.canMessageUser,
    required this.canChooseModels,
    required this.approvalRequired,
  });

  final String id;
  final int autonomy;
  final bool canStartTasks;
  final bool canCreateWorkers;
  final bool canMessageUser;
  final bool canChooseModels;
  final List<String> approvalRequired;

  @override
  Map<String, dynamic> toJson() => {
        'kind': 'setPowers',
        'id': id,
        'autonomy': autonomy,
        'canStartTasks': canStartTasks,
        'canCreateWorkers': canCreateWorkers,
        'canMessageUser': canMessageUser,
        'canChooseModels': canChooseModels,
        'approvalRequired': approvalRequired,
      };
}

/// What the organization screen reads of an agent in the hub's settings (P120, P123); the rest of `AgentSettings` stays
/// out. A hub that doesn't send a field gets its default.
class AgentInfo {
  const AgentInfo({
    required this.id,
    this.role,
    this.reportsTo,
    this.canDelegateToAgents = false,
    this.canManageAgents = false,
    this.canMessageAgents = false,
    this.canManageTasks = false,
    this.canStartTasks = true,
    this.canCreateWorkers = true,
    this.canMessageUser = true,
    this.canChooseModels = true,
    this.autonomy = 4,
    this.approvalRequired = const [],
    this.delegationModels = const [],
  });

  final String id;
  final String? role;
  final String? reportsTo;
  final bool canDelegateToAgents;
  final bool canManageAgents;
  final bool canMessageAgents;
  final bool canManageTasks;

  /// On unless a person took them away (P122): starting background work, creating temporary agents, starting a message to the
  /// person, and picking the model of a delegated task.
  final bool canStartTasks;
  final bool canCreateWorkers;
  final bool canMessageUser;
  final bool canChooseModels;

  /// 1 only answers, 2 suggests, 3 asks before every change, 4 acts alone, 5 also manages its subordinates without asking.
  final int autonomy;
  final List<String> approvalRequired;

  /// The models the agent may pick when it delegates; empty is open.
  final List<String> delegationModels;

  static AgentInfo fromJson(dynamic json) {
    final map = json as Map<String, dynamic>;
    return AgentInfo(
      id: map['id'] as String,
      role: map['role'] as String?,
      reportsTo: map['reportsTo'] as String?,
      canDelegateToAgents: map['canDelegateToAgents'] as bool? ?? false,
      canManageAgents: map['canManageAgents'] as bool? ?? false,
      canMessageAgents: map['canMessageAgents'] as bool? ?? false,
      canManageTasks: map['canManageTasks'] as bool? ?? false,
      canStartTasks: map['canStartTasks'] as bool? ?? true,
      canCreateWorkers: map['canCreateWorkers'] as bool? ?? true,
      canMessageUser: map['canMessageUser'] as bool? ?? true,
      canChooseModels: map['canChooseModels'] as bool? ?? true,
      autonomy: map['autonomy'] as int? ?? 4,
      approvalRequired: [for (final c in (map['approvalRequired'] as List<dynamic>? ?? const [])) c as String],
      delegationModels: [for (final m in (map['delegationModels'] as List<dynamic>? ?? const [])) m as String],
    );
  }
}

/// A named model policy (P123): [id] ("fast", "reasoning"...) is answered by [model]; [description] tells an agent that
/// delegates when to pick it.
class ModelPolicy {
  const ModelPolicy({required this.id, required this.model, this.description});

  final String id;
  final String model;
  final String? description;

  static ModelPolicy fromJson(dynamic json) {
    final map = json as Map<String, dynamic>;
    return ModelPolicy(id: map['id'] as String, model: map['model'] as String, description: map['description'] as String?);
  }
}

/// One "always" a person gave (P122). [covers] is a risk category (`delete_data`) or a tool name; no [agentId] is the default
/// agent. [lastUsedMs] is when it was given or last saved the agent from asking (null from a hub that predates it; it is not part of
/// the identity of a grant). Mirrors `GrantDto`.
class Grant {
  const Grant({required this.conversationId, this.agentId, required this.covers, this.lastUsedMs});

  final String conversationId;
  final String? agentId;
  final String covers;
  final int? lastUsedMs;

  static Grant fromJson(dynamic json) {
    final map = json as Map<String, dynamic>;
    return Grant(
      conversationId: map['conversationId'] as String,
      agentId: map['agentId'] as String?,
      covers: map['covers'] as String,
      lastUsedMs: (map['lastUsedMs'] as num?)?.toInt(),
    );
  }

  @override
  bool operator ==(Object other) => other is Grant && other.conversationId == conversationId && other.agentId == agentId && other.covers == covers;

  @override
  int get hashCode => Object.hash(conversationId, agentId, covers);
}

/// One task an agent delegated in the background (P123), as the hub's log has it. [group] is shared by the tasks one turn
/// started. Mirrors `AgentTaskDto`.
class AgentTask {
  const AgentTask({
    required this.id,
    required this.group,
    required this.assignee,
    required this.objective,
    required this.channel,
    required this.state,
    required this.createdAtMs,
    this.owner,
    this.parentId,
    this.model,
    this.result,
    this.error,
    this.totalTokens,
    this.startedAtMs,
    this.finishedAtMs,
    this.controllable = false,
    this.pausable = false,
  });

  final String id;
  final String group;

  /// The agent that delegated.
  final String? owner;

  /// Who does the work, or the name given to a temporary helper.
  final String assignee;

  /// The task this one is a subtask of.
  final String? parentId;
  final String objective;
  final String? model;
  final String channel;

  /// `pending`, `running`, `waiting`, `paused`, `done`, `failed` or `cancelled`; an unknown one counts as pending.
  final String state;
  final String? result;
  final String? error;
  final int? totalTokens;
  final int createdAtMs;
  final int? startedAtMs;
  final int? finishedAtMs;

  /// Running in the hub process that answered, so it can be paused, resumed or stopped from here.
  final bool controllable;

  /// Among the controllable ones, those that can also be paused (every one, since a delegation the agent waits on can be paused too).
  final bool pausable;

  static AgentTask fromJson(dynamic json) {
    final map = json as Map<String, dynamic>;
    return AgentTask(
      id: map['id'] as String,
      group: map['group'] as String,
      owner: map['owner'] as String?,
      assignee: map['assignee'] as String,
      parentId: map['parentId'] as String?,
      objective: map['objective'] as String,
      model: map['model'] as String?,
      channel: map['channel'] as String,
      state: map['state'] as String,
      result: map['result'] as String?,
      error: map['error'] as String?,
      totalTokens: map['totalTokens'] as int?,
      createdAtMs: map['createdAtMs'] as int,
      startedAtMs: map['startedAtMs'] as int?,
      finishedAtMs: map['finishedAtMs'] as int?,
      controllable: map['controllable'] as bool? ?? false,
      pausable: map['pausable'] as bool? ?? false,
    );
  }
}

/// Something that happened among the agents (P121), for the feed of activity. [kind] is `delegated`, `started`, `done`, `failed`,
/// `cancelled` (the delegated tasks, with [taskId]), `note`, `reply` (agents writing each other) or `messaged_user` (an agent writing first
/// to the person); the last three carry the [conversationId]. An empty [actor] is the assistant that answers with no agent picked.
class ActivityEvent {
  const ActivityEvent({
    required this.id,
    required this.atMs,
    required this.kind,
    required this.actor,
    required this.text,
    this.target,
    this.taskId,
    this.conversationId,
  });

  final String id;
  final int atMs;
  final String kind;
  final String actor;
  final String? target;
  final String text;
  final String? taskId;
  final String? conversationId;

  static ActivityEvent fromJson(dynamic json) {
    final map = json as Map<String, dynamic>;
    return ActivityEvent(
      id: map['id'] as String,
      atMs: map['atMs'] as int,
      kind: map['kind'] as String,
      actor: map['actor'] as String,
      target: map['target'] as String?,
      text: map['text'] as String,
      taskId: map['taskId'] as String?,
      conversationId: map['conversationId'] as String?,
    );
  }
}

/// The person's answer to an [ApprovalRequestMessage] (P87).
final class ResolveApprovalMessage extends ClientMessage {
  const ResolveApprovalMessage(this.approvalId, this.approved);

  final int approvalId;
  final bool approved;

  @override
  Map<String, dynamic> toJson() => {'type': 'resolveApproval', 'approvalId': approvalId, 'approved': approved};
}

/// The result of a `ToolCallRequestMessage` this client was asked to run (Phase 7.4).
final class ToolCallResultMessage extends ClientMessage {
  const ToolCallResultMessage(this.callId, this.result);

  final int callId;
  final dynamic result;

  @override
  Map<String, dynamic> toJson() => {'type': 'toolCallResult', 'callId': callId, 'result': result};
}

/// This client failed to run a requested tool call (Phase 7.4).
final class ToolCallErrorMessage extends ClientMessage {
  const ToolCallErrorMessage(this.callId, this.message);

  final int callId;
  final String message;

  @override
  Map<String, dynamic> toJson() => {'type': 'toolCallError', 'callId': callId, 'message': message};
}

/// Asks for one of this device's persisted conversations (P40) — answered by a
/// [HistoryServerMessage] or [HistoryErrorMessage] carrying the same `requestId`. `limit` keeps
/// only the most recent messages; null asks for all of them. A null [conversationId] is the
/// default conversation, same as [ChatMessage].
final class RequestHistoryMessage extends ClientMessage {
  const RequestHistoryMessage(this.requestId, {this.limit, this.conversationId});

  final int requestId;
  final int? limit;
  final String? conversationId;

  @override
  Map<String, dynamic> toJson() => {
        'type': 'requestHistory',
        'requestId': requestId,
        'limit': limit,
        if (conversationId != null) 'conversationId': conversationId,
      };
}

/// P78 — lists this device's conversations; answered by [ConversationListMessage] or
/// [ConversationErrorMessage] with the same `requestId`.
final class ListConversationsMessage extends ClientMessage {
  const ListConversationsMessage(this.requestId);

  final int requestId;

  @override
  Map<String, dynamic> toJson() => {'type': 'listConversations', 'requestId': requestId};
}

/// P121 — asks for the id of an agent's channel, the one conversation it keeps with the person; answered by
/// [AgentChannelMessage] or [ConversationErrorMessage] with the same `requestId`.
final class OpenAgentChannelMessage extends ClientMessage {
  const OpenAgentChannelMessage(this.requestId, this.agentId);

  final int requestId;
  final String agentId;

  @override
  Map<String, dynamic> toJson() => {'type': 'openAgentChannel', 'requestId': requestId, 'agentId': agentId};
}

/// P78 — answered by [ConversationOkMessage] or [ConversationErrorMessage].
final class RenameConversationMessage extends ClientMessage {
  const RenameConversationMessage(this.requestId, this.conversationId, this.title);

  final int requestId;
  final String conversationId;
  final String title;

  @override
  Map<String, dynamic> toJson() =>
      {'type': 'renameConversation', 'requestId': requestId, 'conversationId': conversationId, 'title': title};
}

/// P78 — answered by [ConversationOkMessage] or [ConversationErrorMessage].
final class DeleteConversationMessage extends ClientMessage {
  const DeleteConversationMessage(this.requestId, this.conversationId);

  final int requestId;
  final String conversationId;

  @override
  Map<String, dynamic> toJson() => {'type': 'deleteConversation', 'requestId': requestId, 'conversationId': conversationId};
}

/// P157 — pins or archives a conversation; a flag left null stays as it is. Archiving unpins, and pinning an archived one brings
/// it back. Answered by [ConversationOkMessage] or [ConversationErrorMessage].
final class SetConversationFlagsMessage extends ClientMessage {
  const SetConversationFlagsMessage(this.requestId, this.conversationId, {this.pinned, this.archived});

  final int requestId;
  final String conversationId;
  final bool? pinned;
  final bool? archived;

  @override
  Map<String, dynamic> toJson() => {
        'type': 'setConversationFlags',
        'requestId': requestId,
        'conversationId': conversationId,
        if (pinned != null) 'pinned': pinned,
        if (archived != null) 'archived': archived,
      };
}

/// One of this device's conversations in a [ConversationListMessage] (P78). Mirrors
/// `warden_server_protocol::protocol::ConversationSummary`.
class ConversationSummary {
  const ConversationSummary({
    required this.id,
    required this.title,
    required this.createdAt,
    required this.updatedAt,
    this.agentId,
    this.projectId,
    this.code = false,
    this.workdir,
    this.parent,
    this.replies = 0,
    this.pinned = false,
    this.archived = false,
  });

  final String id;
  final String title;
  final int createdAt;
  final int updatedAt;

  /// The agent this conversation last spoke with (P46/P87), restored when it's opened.
  final String? agentId;

  /// The project (P103) this conversation belongs to; null outside any project.
  final String? projectId;

  /// P124 — the project is a code project: a turn of this conversation is a task for the code engine, which can be stopped.
  final bool code;

  /// The folder of the hub's machine this conversation works in (P102), fixed when it began.
  final String? workdir;

  /// P125 — set on a thread: the conversation and the message it came from.
  final ThreadParent? parent;

  /// P125 — for a thread, how many messages the person sent in it.
  final int replies;

  /// P157 — kept at the top of the list. Absent from the hub's JSON when not.
  final bool pinned;

  /// P157 — put away: listed apart. Never both pinned and archived.
  final bool archived;

  static ConversationSummary fromJson(dynamic json) {
    final map = json as Map<String, dynamic>;
    return ConversationSummary(
      id: map['id'] as String,
      title: map['title'] as String,
      createdAt: map['createdAt'] as int,
      updatedAt: map['updatedAt'] as int,
      agentId: map['agentId'] as String?,
      projectId: map['projectId'] as String?,
      code: map['code'] as bool? ?? false,
      workdir: map['workdir'] as String?,
      parent: map['parent'] == null ? null : ThreadParent.fromJson(map['parent']),
      replies: map['replies'] as int? ?? 0,
      pinned: map['pinned'] as bool? ?? false,
      archived: map['archived'] as bool? ?? false,
    );
  }
}

/// P125 — what a thread hangs from: a message of another conversation. Mirrors `ThreadParentDto`.
class ThreadParent {
  const ThreadParent({required this.conversationId, required this.messageId});

  final String conversationId;
  final String messageId;

  static ThreadParent fromJson(dynamic json) {
    final map = json as Map<String, dynamic>;
    return ThreadParent(conversationId: map['conversationId'] as String, messageId: map['messageId'] as String);
  }

  Map<String, dynamic> toJson() => {'conversationId': conversationId, 'messageId': messageId};
}

/// One folder in a [DirListMessage] (P102): its name and the path to open or pick.
class DirEntry {
  const DirEntry({required this.name, required this.path});

  final String name;
  final String path;

  static DirEntry fromJson(dynamic json) {
    final map = json as Map<String, dynamic>;
    return DirEntry(name: map['name'] as String, path: map['path'] as String);
  }
}

/// A machine other than the hub (P93) — here only what the folder picker needs: its id and name, and whether a folder can be picked on it
/// now (online, approved, switched on and lending a folder).
class NodeInfo {
  const NodeInfo({required this.deviceId, required this.name, this.online = false, this.approved = false, this.enabled = false, this.sharesFiles = false});

  final String deviceId;
  final String name;
  final bool online;
  final bool approved;
  final bool enabled;

  /// The node offered to read and write files in one folder (`offer.files`).
  final bool sharesFiles;

  /// A folder can be picked on it right now.
  bool get hasFolders => online && approved && enabled && sharesFiles;

  static NodeInfo fromJson(dynamic json) {
    final map = json as Map<String, dynamic>;
    final offer = map['offer'];
    return NodeInfo(
      deviceId: map['deviceId'] as String? ?? '',
      name: map['name'] as String? ?? '',
      online: map['online'] as bool? ?? false,
      approved: map['approved'] as bool? ?? false,
      enabled: map['enabled'] as bool? ?? false,
      sharesFiles: offer is Map<String, dynamic> && (offer['files'] as bool? ?? false),
    );
  }
}

/// Token usage for one chat turn, when the provider reported it. Mirrors
/// `warden_core::model::Usage`.
class Usage {
  const Usage({
    required this.promptTokens,
    required this.completionTokens,
    required this.totalTokens,
  });

  final int promptTokens;
  final int completionTokens;
  final int totalTokens;

  static Usage? fromJson(Map<String, dynamic>? json) {
    if (json == null) return null;
    return Usage(
      promptTokens: json['promptTokens'] as int,
      completionTokens: json['completionTokens'] as int,
      totalTokens: json['totalTokens'] as int,
    );
  }
}

/// Media extracted from an MCP tool result during a turn (P64 frente 2 slice 3). Mirrors
/// `warden_core::model::Attachment` — `data` is base64 with no `data:...;base64,` prefix.
class Attachment {
  const Attachment({required this.mimeType, required this.data});

  final String mimeType;
  final String data;

  static Attachment fromJson(dynamic json) {
    final map = json as Map<String, dynamic>;
    return Attachment(mimeType: map['mimeType'] as String, data: map['data'] as String);
  }
}

/// One persisted message in a [HistoryServerMessage] (P40). Mirrors
/// `warden_server_protocol::protocol::HistoryMessage`.
class HistoryEntry {
  const HistoryEntry({required this.fromUser, required this.content, this.attachments = const [], this.id});

  /// P125 — the hub's id for the message, which a thread hangs from; null from a hub that predates threads.
  final String? id;

  /// `role` on the wire is only ever `user` or `assistant`.
  final bool fromUser;
  final String content;
  final List<Attachment> attachments;

  static HistoryEntry fromJson(dynamic json) {
    final map = json as Map<String, dynamic>;
    return HistoryEntry(
      id: map['id'] as String?,
      fromUser: map['role'] == 'user',
      content: map['content'] as String,
      attachments: (map['attachments'] as List<dynamic>?)?.map(Attachment.fromJson).toList() ?? const [],
    );
  }
}

/// Messages sent from a warden-server to this client.
sealed class ServerMessage {
  const ServerMessage();

  static ServerMessage fromJson(Map<String, dynamic> json) {
    return switch (json['type']) {
      'helloAck' => HelloAckMessage(
          json['serverName'] as String,
          deviceToken: json['deviceToken'] as String?,
          user: json['user'] == null ? null : UserInfo.fromJson(json['user'] as Map<String, dynamic>),
        ),
      'passwordChanged' => PasswordChangedMessage(json['requestId'] as int, recoveryCode: json['recoveryCode'] as String?),
      'recoveryCode' => RecoveryCodeMessage(json['requestId'] as int, json['code'] as String),
      'recoveryPolicyAccepted' => RecoveryPolicyAcceptedMessage(json['requestId'] as int, recoveryCode: json['recoveryCode'] as String?),
      'recoveryNoticesAcked' => RecoveryNoticesAckedMessage(json['requestId'] as int),
      'truthIdLinked' => TruthIdLinkedMessage(json['requestId'] as int, json['username'] as String),
      'userError' => UserErrorMessage(json['requestId'] as int, json['message'] as String, authRejected: json['authRejected'] as bool? ?? false),
      'authError' => AuthErrorMessage(json['reason'] as String),
      'pong' => PongMessage(json['nonce'] as int),
      'chatResponse' => ChatResponseMessage(
          json['content'] as String,
          Usage.fromJson(json['usage'] as Map<String, dynamic>?),
          attachments: (json['attachments'] as List<dynamic>?)?.map(Attachment.fromJson).toList() ?? const [],
          conversationId: json['conversationId'] as String?,
        ),
      'chatError' => ChatErrorMessage(json['message'] as String, conversationId: json['conversationId'] as String?, topUp: json['topUp'] as bool? ?? false),
      'toolCallRequest' => ToolCallRequestMessage(
          json['callId'] as int,
          json['tool'] as String,
          json['arguments'] as Map<String, dynamic>,
        ),
      'history' => HistoryServerMessage(
          json['requestId'] as int,
          (json['messages'] as List<dynamic>).map(HistoryEntry.fromJson).toList(),
        ),
      'historyError' => HistoryErrorMessage(json['requestId'] as int, json['message'] as String),
      'conversationList' => ConversationListMessage(
          json['requestId'] as int,
          (json['conversations'] as List<dynamic>).map(ConversationSummary.fromJson).toList(),
        ),
      'dirList' => DirListMessage(
          json['requestId'] as int,
          path: json['path'] as String,
          parent: json['parent'] as String?,
          dirs: (json['dirs'] as List<dynamic>).map(DirEntry.fromJson).toList(),
        ),
      'dirError' => DirErrorMessage(json['requestId'] as int, json['message'] as String),
      'nodeList' => NodeListMessage(
          json['requestId'] as int,
          [for (final node in (json['nodes'] as List<dynamic>? ?? const [])) NodeInfo.fromJson(node)],
        ),
      'nodeError' => NodeErrorMessage(json['requestId'] as int, json['message'] as String? ?? ''),
      'conversationOk' => ConversationOkMessage(json['requestId'] as int),
      'agentChannel' => AgentChannelMessage(json['requestId'] as int, json['conversationId'] as String),
      'conversationError' => ConversationErrorMessage(json['requestId'] as int, json['message'] as String),
      'goodbye' => GoodbyeServerMessage(json['reason'] as String?),
      'settings' => SettingsMessage(
          json['requestId'] as int,
          [
            for (final agent in ((json['settings'] as Map<String, dynamic>)['agents'] as List<dynamic>? ?? const []))
              (agent as Map<String, dynamic>)['id'] as String,
          ],
          agents: _agentsOf(json['settings'] as Map<String, dynamic>),
          modelPolicies: _policiesOf(json['settings'] as Map<String, dynamic>),
          modelIds: _modelIdsOf(json['settings'] as Map<String, dynamic>),
        ),
      'settingsSaved' => SettingsSavedMessage(
          json['requestId'] as int,
          agents: _agentsOf(json['settings'] as Map<String, dynamic>),
          modelPolicies: _policiesOf(json['settings'] as Map<String, dynamic>),
          modelIds: _modelIdsOf(json['settings'] as Map<String, dynamic>),
        ),
      'agentOrgList' => AgentOrgListMessage(
          json['requestId'] as int,
          [for (final agent in (json['agents'] as List<dynamic>? ?? const [])) AgentInfo.fromJson(agent)],
          json['access'] as String? ?? '',
        ),
      'settingsError' => SettingsErrorMessage(json['requestId'] as int, json['message'] as String, authRejected: json['authRejected'] as bool? ?? false),
      'skillDraft' => SkillDraftMessage(json['requestId'] as int, SkillDraft.fromJson(json['skill'] as Map<String, dynamic>)),
      'skillError' => SkillErrorMessage(json['requestId'] as int, json['message'] as String),
      'agentTaskList' => AgentTaskListMessage(
          json['requestId'] as int,
          (json['tasks'] as List<dynamic>).map(AgentTask.fromJson).toList(),
        ),
      'usageReport' => UsageReportMessage(
          json['requestId'] as int,
          RecentSpend.fromJson((json['report'] as Map<String, dynamic>?)?['recent']),
          summary: UsageSummary.fromJson((json['report'] as Map<String, dynamic>?) ?? const {}),
        ),
      'grantList' => GrantListMessage(
          json['requestId'] as int,
          ((json['grants'] as List<dynamic>?) ?? const []).map(Grant.fromJson).toList(),
          ttlDays: (json['ttlDays'] as num?)?.toInt(),
        ),
      'activityList' => ActivityListMessage(
          json['requestId'] as int,
          ((json['events'] as List<dynamic>?) ?? const []).map(ActivityEvent.fromJson).toList(),
        ),
      'usageError' => UsageErrorMessage(json['requestId'] as int, json['message'] as String),
      'taskError' => TaskErrorMessage(json['requestId'] as int, json['message'] as String, authRejected: json['authRejected'] as bool? ?? false),
      'deviceInvite' => DeviceInviteMessage(
          json['requestId'] as int,
          json['name'] as String,
          json['code'] as String,
          (json['expiresMs'] as num).toInt(),
        ),
      'deviceInviteList' => DeviceInviteListMessage(
          json['requestId'] as int,
          ((json['invites'] as List<dynamic>?) ?? const []).map((i) => PendingDeviceInvite.fromJson(i as Map<String, dynamic>)).toList(),
        ),
      'pairingKeyRotated' => PairingKeyRotatedMessage(json['requestId'] as int, json['newKey'] as String),
      'deviceError' => DeviceErrorMessage(json['requestId'] as int, json['message'] as String, authRejected: json['authRejected'] as bool? ?? false),
      'approvalRequest' => ApprovalRequestMessage(
          json['approvalId'] as int,
          target: json['target'] as String,
          action: json['action'] as String,
          detail: json['detail'] as String,
          category: json['category'] as String?,
        ),
      'approvalCancelled' => ApprovalCancelledMessage(json['approvalId'] as int),
      'conversationsChanged' => ConversationsChangedMessage(json['conversationId'] as String),
      'chatEvent' => switch (ChatEvent.fromJson(json['event'] as Map<String, dynamic>)) {
          final ChatEvent event => ChatEventMessage(json['conversationId'] as String, event),
          null => const UnknownServerMessage('chatEvent'),
        },
      'orgAccessChanged' => OrgAccessChangedMessage(json['access'] as String? ?? ''),
      // A message this app doesn't know yet (a newer hub) is skipped, not a broken connection.
      final String other => UnknownServerMessage(other),
      _ => throw const FormatException('ServerMessage without a type'),
    };
  }

  static ServerMessage decode(String text) =>
      fromJson(jsonDecode(text) as Map<String, dynamic>);

  static List<AgentInfo> _agentsOf(Map<String, dynamic> settings) =>
      (settings['agents'] as List<dynamic>? ?? const []).map(AgentInfo.fromJson).toList();

  static List<ModelPolicy> _policiesOf(Map<String, dynamic> settings) =>
      (settings['modelPolicies'] as List<dynamic>? ?? const []).map(ModelPolicy.fromJson).toList();

  /// The ids of the providers and combos: what a policy can be answered by.
  static List<String> _modelIdsOf(Map<String, dynamic> settings) => [
        for (final key in const ['providers', 'combos'])
          for (final entry in (settings[key] as List<dynamic>? ?? const [])) (entry as Map<String, dynamic>)['id'] as String,
      ];
}

final class HelloAckMessage extends ServerMessage {
  const HelloAckMessage(this.serverName, {this.deviceToken, this.user});

  final String serverName;

  /// A newly issued device token (P36) — present when this Hello paired with the pairing key.
  /// Replaces whatever token this device held for the hub, which no longer works.
  final String? deviceToken;

  /// P84 — the member this device belongs to; null for the workspace's owner.
  final UserInfo? user;
}

/// Mirrors `UserInfoDto` (P84): a member of the workspace, never their password.
final class UserInfo {
  const UserInfo({
    required this.id,
    required this.name,
    required this.mustChangePassword,
    this.encrypted = false,
    this.needsRecovery = false,
    this.locked = false,
    this.memberPolicy = '',
    this.policyPending = false,
    this.recoveryPolicy = '',
    this.recoveries = const [],
    this.truthid = '',
    this.orgAccess = '',
  });

  /// The username.
  final String id;
  final String name;

  /// Still on the provisional password the owner gave them: the hub only lets them change it.
  final bool mustChangePassword;

  /// P84 slice 4 — their data is encrypted on the hub with a key only they (or their recovery code) open.
  final bool encrypted;

  /// The owner reset their password, so the data opens only with the recovery code.
  final bool needsRecovery;

  /// The hub doesn't hold their key (it restarted): the data is shut until they sign in with the password.
  final bool locked;

  /// Who besides them may open their data right now — `private`, `consent` or `company` — empty while it
  /// isn't encrypted (parte B).
  final String memberPolicy;

  /// The workspace's policy changed to a weaker one they haven't accepted yet.
  final bool policyPending;

  /// The workspace's recovery policy.
  final String recoveryPolicy;

  /// Every time the owner recovered their data with the workspace's recovery key.
  final List<RecoveryEvent> recoveries;

  /// P84 slice 5 — the TruthID username they linked; empty if none.
  final String truthid;

  /// P120 — what the owner lets them do with the organization of the agents: `view` or `edit`; empty is none.
  final String orgAccess;

  UserInfo withTruthId(String username) => UserInfo(
        id: id,
        name: name,
        mustChangePassword: mustChangePassword,
        encrypted: encrypted,
        needsRecovery: needsRecovery,
        locked: locked,
        memberPolicy: memberPolicy,
        policyPending: policyPending,
        recoveryPolicy: recoveryPolicy,
        recoveries: recoveries,
        truthid: username,
        orgAccess: orgAccess,
      );

  /// The recoveries they haven't been told about yet.
  List<RecoveryEvent> get unseenRecoveries => [for (final event in recoveries) if (!event.seen) event];

  static UserInfo fromJson(Map<String, dynamic> json) => UserInfo(
        id: json['id'] as String,
        name: json['name'] as String,
        mustChangePassword: json['mustChangePassword'] as bool? ?? false,
        encrypted: json['encrypted'] as bool? ?? false,
        needsRecovery: json['needsRecovery'] as bool? ?? false,
        locked: json['locked'] as bool? ?? false,
        memberPolicy: json['memberPolicy'] as String? ?? '',
        policyPending: json['policyPending'] as bool? ?? false,
        recoveryPolicy: json['recoveryPolicy'] as String? ?? '',
        recoveries: [for (final e in (json['recoveries'] as List<dynamic>? ?? const [])) RecoveryEvent.fromJson(e as Map<String, dynamic>)],
        truthid: json['truthid'] as String? ?? '',
        orgAccess: json['orgAccess'] as String? ?? '',
      );

  /// After a password change: the same person, no longer on the provisional password and (if the owner
  /// had reset it) with their data opened again.
  UserInfo withOwnPassword({bool? encrypted}) => UserInfo(
        id: id,
        name: name,
        mustChangePassword: false,
        encrypted: encrypted ?? this.encrypted,
        memberPolicy: memberPolicy,
        policyPending: policyPending,
        recoveryPolicy: recoveryPolicy,
        recoveries: recoveries,
        truthid: truthid,
        orgAccess: orgAccess,
      );
}

/// Mirrors `RecoveryEventDto` (P84 slice 4 parte B): the owner recovered someone's data.
final class RecoveryEvent {
  const RecoveryEvent({required this.atMs, required this.kind, required this.seen});

  /// Milliseconds since the epoch.
  final int atMs;

  /// The policy it was done under: `consent` or `company`.
  final String kind;

  /// The person has seen it.
  final bool seen;

  static RecoveryEvent fromJson(Map<String, dynamic> json) => RecoveryEvent(
        atMs: json['atMs'] as int,
        kind: json['kind'] as String,
        seen: json['seen'] as bool? ?? false,
      );
}

/// P84 — the password change went through.
final class PasswordChangedMessage extends ServerMessage {
  const PasswordChangedMessage(this.requestId, {this.recoveryCode});

  final int requestId;

  /// P84 slice 4 — this change turned encryption on for their data: shown once, they have to write it down.
  final String? recoveryCode;
}

/// A recovery code, shown once: the answer to [RegenerateRecoveryCodeMessage], or ([requestId] 0) sent right
/// after the `HelloAck` when signing in turned encryption on for a member from before.
final class RecoveryCodeMessage extends ServerMessage {
  const RecoveryCodeMessage(this.requestId, this.code);

  final int requestId;
  final String code;
}

/// P84 slice 4 parte B — the member accepted the policy. [recoveryCode]: entering or leaving `consent`
/// made a new one, shown once.
final class RecoveryPolicyAcceptedMessage extends ServerMessage {
  const RecoveryPolicyAcceptedMessage(this.requestId, {this.recoveryCode});

  final int requestId;
  final String? recoveryCode;
}

final class RecoveryNoticesAckedMessage extends ServerMessage {
  const RecoveryNoticesAckedMessage(this.requestId);

  final int requestId;
}

/// P84 slice 5 — the member's TruthID is linked.
final class TruthIdLinkedMessage extends ServerMessage {
  const TruthIdLinkedMessage(this.requestId, this.username);

  final int requestId;
  final String username;
}

/// P84 — a people request failed. [authRejected]: the current password was wrong.
final class UserErrorMessage extends ServerMessage {
  const UserErrorMessage(this.requestId, this.message, {this.authRejected = false});

  final int requestId;
  final String message;
  final bool authRejected;
}

final class AuthErrorMessage extends ServerMessage {
  const AuthErrorMessage(this.reason);

  final String reason;
}

final class PongMessage extends ServerMessage {
  const PongMessage(this.nonce);

  // Dart's `int` is a real 64-bit signed integer on the VM (Android/iOS),
  // matching Rust's u64 for any realistic monotonic counter value. This
  // stops holding if Flutter Web is ever targeted (`int` becomes a JS
  // double there) — not a concern for the mobile-only scope of Phase 7.
  final int nonce;
}

final class ChatResponseMessage extends ServerMessage {
  const ChatResponseMessage(this.content, this.usage, {this.attachments = const [], this.conversationId});

  final String content;
  final Usage? usage;
  final List<Attachment> attachments;

  /// Which conversation this answers (P78) — `chat` carries no request id.
  final String? conversationId;
}

final class ChatErrorMessage extends ServerMessage {
  const ChatErrorMessage(this.message, {this.conversationId, this.topUp = false});

  final String message;

  /// Same as [ChatResponseMessage.conversationId].
  final String? conversationId;

  /// P168 — the hosted credit ran out: the chat offers the balance screen under the error.
  final bool topUp;
}

/// Asks this client to run one of the tools it advertised in `Hello.tools` (Phase 7.4).
final class ToolCallRequestMessage extends ServerMessage {
  const ToolCallRequestMessage(this.callId, this.tool, this.arguments);

  final int callId;
  final String tool;
  final Map<String, dynamic> arguments;
}

/// Reply to a [RequestHistoryMessage] (P40), oldest message first.
final class HistoryServerMessage extends ServerMessage {
  const HistoryServerMessage(this.requestId, this.messages);

  final int requestId;
  final List<HistoryEntry> messages;
}

/// The server couldn't read this device's conversation (P40).
final class HistoryErrorMessage extends ServerMessage {
  const HistoryErrorMessage(this.requestId, this.message);

  final int requestId;
  final String message;
}

/// Reply to a [ListConversationsMessage] (P78), newest-updated first.
final class ConversationListMessage extends ServerMessage {
  const ConversationListMessage(this.requestId, this.conversations);

  final int requestId;
  final List<ConversationSummary> conversations;
}

/// Reply to [ListDirsMessage] (P102): the folders in [path] (empty for a member's list of allowed folders), and the
/// one above it ([parent] is null at the top of what the person may see).
final class DirListMessage extends ServerMessage {
  const DirListMessage(this.requestId, {required this.path, this.parent, required this.dirs});

  final int requestId;
  final String path;
  final String? parent;
  final List<DirEntry> dirs;
}

/// A [ListDirsMessage] failed (P102): not a folder, outside what the person may see, unreadable.
final class DirErrorMessage extends ServerMessage {
  const DirErrorMessage(this.requestId, this.message);

  final int requestId;
  final String message;
}

/// Reply to [ListNodesMessage] (P102).
final class NodeListMessage extends ServerMessage {
  const NodeListMessage(this.requestId, this.nodes);

  final int requestId;
  final List<NodeInfo> nodes;
}

/// A [ListNodesMessage] failed (P102).
final class NodeErrorMessage extends ServerMessage {
  const NodeErrorMessage(this.requestId, this.message);

  final int requestId;
  final String message;
}

/// Reply to a successful rename/delete (P78).
final class ConversationOkMessage extends ServerMessage {
  const ConversationOkMessage(this.requestId);

  final int requestId;
}

/// Reply to an [OpenAgentChannelMessage] (P121): the conversation id of that agent's channel.
final class AgentChannelMessage extends ServerMessage {
  const AgentChannelMessage(this.requestId, this.conversationId);

  final int requestId;
  final String conversationId;
}

/// A list/rename/delete failed (P78) — the raw error text.
final class ConversationErrorMessage extends ServerMessage {
  const ConversationErrorMessage(this.requestId, this.message);

  final int requestId;
  final String message;
}

final class GoodbyeServerMessage extends ServerMessage {
  const GoodbyeServerMessage(this.reason);

  final String? reason;
}

/// Reply to [RequestSettingsMessage], reduced to what this app uses: the agents' ids (P87), the agents with their
/// organization and model limits (P120, P123) and the model policies (P123).
final class SettingsMessage extends ServerMessage {
  const SettingsMessage(this.requestId, this.agentIds, {this.agents = const [], this.modelPolicies = const [], this.modelIds = const []});

  final int requestId;
  final List<String> agentIds;
  final List<AgentInfo> agents;
  final List<ModelPolicy> modelPolicies;

  /// The providers' and combos' ids.
  final List<String> modelIds;
}

/// Reply to a successful [EditAgentOrgMessage], with the agents as the hub holds them now.
final class SettingsSavedMessage extends ServerMessage {
  const SettingsSavedMessage(this.requestId, {this.agents = const [], this.modelPolicies = const [], this.modelIds = const []});

  final int requestId;
  final List<AgentInfo> agents;
  final List<ModelPolicy> modelPolicies;
  final List<String> modelIds;
}

/// P120 — the tree as a member sees it: only the id, the role and the superior of each agent of the owner. [access] is `view` or `edit`.
/// The reply to [ListAgentOrgMessage] and to a member's [EditAgentOrgMessage.asMember].
final class AgentOrgListMessage extends ServerMessage {
  const AgentOrgListMessage(this.requestId, this.agents, this.access);

  final int requestId;
  final List<AgentInfo> agents;
  final String access;
}

/// [authRejected]: the pairing key was wrong; nothing was written.
final class SettingsErrorMessage extends ServerMessage {
  const SettingsErrorMessage(this.requestId, this.message, {this.authRejected = false});

  final int requestId;
  final String message;
  final bool authRejected;
}

/// The skill the hub's model wrote (P72), not saved anywhere: the screen loads it into its form for review. The agents
/// the hub may send with it are left out — a skill made from a draft starts global.
class SkillDraft {
  const SkillDraft({required this.name, required this.description, required this.body});

  factory SkillDraft.fromJson(Map<String, dynamic> json) =>
      SkillDraft(name: json['name'] as String, description: json['description'] as String, body: json['body'] as String);

  final String name;
  final String description;
  final String body;
}

/// Reply to [DraftSkillMessage] (P72).
final class SkillDraftMessage extends ServerMessage {
  const SkillDraftMessage(this.requestId, this.draft);

  final int requestId;
  final SkillDraft draft;
}

/// A skill request failed (P72): an empty or too long description, the spending limit, a model that wrote nothing usable.
/// The raw text of the hub.
final class SkillErrorMessage extends ServerMessage {
  const SkillErrorMessage(this.requestId, this.message);

  final int requestId;
  final String message;
}

/// Reply to [ListAgentTasksMessage] and to a successful [ControlAgentTaskMessage] (P123): the delegated tasks, newest first.
final class AgentTaskListMessage extends ServerMessage {
  const AgentTaskListMessage(this.requestId, this.tasks);

  final int requestId;
  final List<AgentTask> tasks;
}

/// What one agent spent, from the hub's ledger (P123): the dollars of the calls that have a price, and how many calls had none.
class SpendBucket {
  const SpendBucket({required this.key, required this.costUsd, required this.unpricedCalls});

  /// The agent's id; empty for calls with no agent chosen.
  final String key;
  final double costUsd;
  final int unpricedCalls;

  /// The buckets the hub sent under [field] of `recent` (`byAgent`, `byCombo`); none for a hub from before the field.
  static List<SpendBucket> listFrom(Map<String, dynamic> recent, String field) => [
        for (final bucket in (recent[field] as List<dynamic>? ?? const []))
          if (bucket is Map<String, dynamic>)
            SpendBucket(
              key: bucket['key'] as String? ?? '',
              costUsd: (bucket['costUsd'] as num?)?.toDouble() ?? 0,
              unpricedCalls: (bucket['unpricedCalls'] as num?)?.toInt() ?? 0,
            ),
      ];
}

/// The part of the usage report the app reads (P123): what each agent spent in the last [windowHours], which is all the ledger keeps.
class RecentSpend {
  const RecentSpend({required this.windowHours, required this.byAgent, this.byCombo = const []});

  final int windowHours;
  final List<SpendBucket> byAgent;

  /// The dollars of the calls made through a combo (P126); there is no bucket for the ones that were not.
  final List<SpendBucket> byCombo;

  /// Null for a hub that sent no `recent` (spending limits off, or an older hub); a hub from before `byAgent` gives an empty split.
  static RecentSpend? fromJson(dynamic json) {
    if (json is! Map<String, dynamic>) return null;
    return RecentSpend(
      windowHours: (json['windowHours'] as num?)?.toInt() ?? 0,
      byAgent: SpendBucket.listFrom(json, 'byAgent'),
      byCombo: SpendBucket.listFrom(json, 'byCombo'),
    );
  }
}

/// The hub could not make the usage report (P99): a member's data is locked until they sign in with their password.
final class UsageErrorMessage extends ServerMessage {
  const UsageErrorMessage(this.requestId, this.message);

  final int requestId;
  final String message;
}

/// Tokens on one day of the usage report (P99): [date] is `YYYY-MM-DD` in the person's day.
class UsageDay {
  const UsageDay({required this.date, required this.calls, required this.tokens, this.costUsd});

  final String date;
  final int calls;
  final int tokens;

  /// Dollars of the day's priced calls, from the hub's ledger; null on the days the ledger has nothing for.
  final double? costUsd;
}

/// A spending limit as the hub measures it (P99): the meter of one ceiling. A member sees only the limits written for them.
class LimitMeter {
  const LimitMeter({
    required this.id,
    required this.windowHours,
    required this.usedTokens,
    required this.usedCostUsd,
    required this.fraction,
    required this.warn,
    required this.exceeded,
    this.maxTokens,
    this.maxCostUsd,
  });

  final String id;
  final int windowHours;
  final int usedTokens;
  final int? maxTokens;
  final double usedCostUsd;
  final double? maxCostUsd;

  /// The closer ceiling's share used; 1 or more is exhausted.
  final double fraction;
  final bool warn;
  final bool exceeded;
}

/// One bucket of a token breakdown (P126): what an agent, a provider or a combo used. A null [key] is "no agent" / "the default provider".
class UsageBucket {
  const UsageBucket({required this.key, required this.calls, required this.tokens});

  final String? key;

  /// Model calls (assistant messages that carried usage).
  final int calls;
  final int tokens;

  /// The buckets the hub sent under [field], in its order (biggest first); none from a hub that does not tell them.
  static List<UsageBucket> listFrom(Map<String, dynamic> report, String field) => [
        for (final bucket in (report[field] as List<dynamic>? ?? const []))
          if (bucket is Map<String, dynamic>)
            UsageBucket(
              key: bucket['key'] as String?,
              calls: (bucket['messageCount'] as num?)?.toInt() ?? 0,
              tokens: ((bucket['usage'] as Map<String, dynamic>?)?['totalTokens'] as num?)?.toInt() ?? 0,
            ),
      ];
}

/// What the usage screen reads from the report (P99): the person's tokens and calls, a series per day with the dollars the ledger
/// still holds, and the limits that apply to them.
/// The credit of a hosted account, in micro-USD (the hub keeps the exact integer): the report's `credit`, only from a hub that has a
/// `cloud` provider and only in the owner's report.
class HostedCredit {
  const HostedCredit({required this.balanceMicroUsd, required this.availableMicroUsd});

  final int balanceMicroUsd;

  /// The balance minus what the calls in progress hold.
  final int availableMicroUsd;

  double get balanceUsd => balanceMicroUsd / 1000000;
  double get availableUsd => availableMicroUsd / 1000000;

  /// No credit left for a new call.
  bool get isOut => availableMicroUsd <= 0;

  /// Calls in progress hold part of the balance; only then is the available amount worth showing next to it.
  bool get hasHolds => availableMicroUsd != balanceMicroUsd;

  static HostedCredit? fromJson(Object? json) {
    if (json is! Map<String, dynamic>) return null;
    return HostedCredit(
      balanceMicroUsd: (json['balanceMicroUsd'] as num?)?.toInt() ?? 0,
      availableMicroUsd: (json['availableMicroUsd'] as num?)?.toInt() ?? 0,
    );
  }
}

class UsageSummary {
  const UsageSummary({
    required this.totalTokens,
    required this.calls,
    required this.days,
    required this.limitsEnabled,
    required this.limits,
    required this.windowHours,
    required this.costInWindowUsd,
    this.byAgent = const [],
    this.byProvider = const [],
    this.byCombo = const [],
    this.comboSpend = const [],
    this.credit,
    this.topupUrl,
  });

  final int totalTokens;
  final int calls;

  /// Oldest first.
  final List<UsageDay> days;
  final bool limitsEnabled;
  final List<LimitMeter> limits;

  /// How far back the ledger reaches (the longest limit), 0 when the hub keeps no ledger.
  final int windowHours;

  /// Dollars of the priced calls in that window; zero with no ledger.
  final double costInWindowUsd;

  /// Tokens by the agent a turn started with, by the provider that answered and by the combo it went through (P126); empty from an older hub.
  final List<UsageBucket> byAgent;
  final List<UsageBucket> byProvider;
  final List<UsageBucket> byCombo;

  /// Dollars by combo in the ledger's [windowHours] (P126), which is not the span of the token breakdown above; empty from an older hub.
  final List<SpendBucket> comboSpend;

  /// The hosted credit; null without a `cloud` provider and from a member's report.
  final HostedCredit? credit;

  /// Where to buy more of it (the hub's `topup_url`, P168), sent only with [credit]. Check it with `topUpUri` before opening.
  final String? topupUrl;

  static UsageSummary fromJson(Map<String, dynamic> report) {
    final recent = report['recent'] as Map<String, dynamic>?;
    final costs = <String, double>{
      for (final day in (report['dailyCost'] as List<dynamic>? ?? const []))
        if (day is Map<String, dynamic>) day['date'] as String: (day['costUsd'] as num?)?.toDouble() ?? 0,
    };
    final days = [
      for (final day in (report['daily'] as List<dynamic>? ?? const []))
        if (day is Map<String, dynamic>)
          UsageDay(
            date: day['date'] as String? ?? '',
            calls: (day['calls'] as num?)?.toInt() ?? 0,
            tokens: (day['tokens'] as num?)?.toInt() ?? 0,
            costUsd: costs[day['date']],
          ),
    ];
    return UsageSummary(
      totalTokens: ((report['total'] as Map<String, dynamic>?)?['totalTokens'] as num?)?.toInt() ?? 0,
      calls: (report['messageCount'] as num?)?.toInt() ?? 0,
      days: days,
      limitsEnabled: report['limitsEnabled'] as bool? ?? false,
      limits: [
        for (final limit in (report['limits'] as List<dynamic>? ?? const []))
          if (limit is Map<String, dynamic>)
            LimitMeter(
              id: limit['id'] as String? ?? '',
              windowHours: (limit['windowHours'] as num?)?.toInt() ?? 0,
              usedTokens: (limit['usedTokens'] as num?)?.toInt() ?? 0,
              maxTokens: (limit['maxTokens'] as num?)?.toInt(),
              usedCostUsd: (limit['usedCostUsd'] as num?)?.toDouble() ?? 0,
              maxCostUsd: (limit['maxCostUsd'] as num?)?.toDouble(),
              fraction: (limit['fraction'] as num?)?.toDouble() ?? 0,
              warn: limit['warn'] as bool? ?? false,
              exceeded: limit['exceeded'] as bool? ?? false,
            ),
      ],
      windowHours: (recent?['windowHours'] as num?)?.toInt() ?? 0,
      costInWindowUsd: costs.values.fold(0.0, (sum, cost) => sum + cost),
      byAgent: UsageBucket.listFrom(report, 'byAgent'),
      byProvider: UsageBucket.listFrom(report, 'byProvider'),
      byCombo: UsageBucket.listFrom(report, 'byCombo'),
      comboSpend: recent == null ? const [] : SpendBucket.listFrom(recent, 'byCombo'),
      credit: HostedCredit.fromJson(report['credit']),
      topupUrl: report['credit'] is Map<String, dynamic> ? report['topupUrl'] as String? : null,
    );
  }
}

/// Reply to [RequestUsageMessage] (P123): the report's recent spending, null when the hub says none. [summary] is the rest of the report
/// for the usage screen (P99).
final class UsageReportMessage extends ServerMessage {
  const UsageReportMessage(this.requestId, this.recent, {this.summary});

  final int requestId;
  final RecentSpend? recent;
  final UsageSummary? summary;
}

/// Reply to [ListGrantsMessage] and [RevokeGrantMessage] (P122): every "always" given on this connection.
final class GrantListMessage extends ServerMessage {
  const GrantListMessage(this.requestId, this.grants, {this.ttlDays});

  final int requestId;
  final List<Grant> grants;

  /// Days an "always" may go unused before it lapses. Null when the hub keeps none in a file (they end with the connection) or predates it.
  final int? ttlDays;
}

/// Reply to [ListActivityMessage] (P121): the events of the feed, newest first.
final class ActivityListMessage extends ServerMessage {
  const ActivityListMessage(this.requestId, this.events);

  final int requestId;
  final List<ActivityEvent> events;
}

/// A task request failed (P123). [authRejected]: the pairing key was wrong; nothing changed.
final class TaskErrorMessage extends ServerMessage {
  const TaskErrorMessage(this.requestId, this.message, {this.authRejected = false});

  final int requestId;
  final String message;
  final bool authRejected;
}

/// Reply to [CreateDeviceInviteMessage] (P36): the [code] is shown once, the hub keeps only its hash.
final class DeviceInviteMessage extends ServerMessage {
  const DeviceInviteMessage(this.requestId, this.name, this.code, this.expiresMs);

  final int requestId;
  final String name;
  final String code;
  final int expiresMs;
}

/// Reply to [ListDeviceInvitesMessage] and to a successful [CancelDeviceInviteMessage] (P36): the invites that wait, by name.
/// The hub never sends their codes.
final class DeviceInviteListMessage extends ServerMessage {
  const DeviceInviteListMessage(this.requestId, this.invites);

  final int requestId;
  final List<PendingDeviceInvite> invites;
}

/// An invite that waits to be used (P36): who it is for, when it was made and when it lapses.
class PendingDeviceInvite {
  const PendingDeviceInvite({required this.name, required this.createdMs, required this.expiresMs});

  factory PendingDeviceInvite.fromJson(Map<String, dynamic> json) => PendingDeviceInvite(
        name: json['name'] as String,
        createdMs: (json['createdMs'] as num).toInt(),
        expiresMs: (json['expiresMs'] as num).toInt(),
      );

  final String name;
  final int createdMs;
  final int expiresMs;
}

/// Reply to [RotatePairingKeyMessage] (P36): the new key, shown once.
final class PairingKeyRotatedMessage extends ServerMessage {
  const PairingKeyRotatedMessage(this.requestId, this.newKey);

  final int requestId;
  final String newKey;
}

/// A device request failed (P36). [authRejected]: the pairing key was wrong; nothing changed.
final class DeviceErrorMessage extends ServerMessage {
  const DeviceErrorMessage(this.requestId, this.message, {this.authRejected = false});

  final int requestId;
  final String message;
  final bool authRejected;
}

/// A tool in this device's turn needs the person's yes (P46/P87: an agent creating or editing
/// another, an SSH host with approval, a spending-limit pause). No answer before the hub's
/// deadline (120 s) counts as no.
final class ApprovalRequestMessage extends ServerMessage {
  const ApprovalRequestMessage(this.approvalId, {required this.target, required this.action, required this.detail, this.category});

  final int approvalId;
  final String target;
  final String action;
  final String detail;

  /// P122: the kind of action the agent has to get approved (`critical_infra`...), when the ask comes from that rule;
  /// null from a hub that doesn't send it.
  final String? category;
}

/// The hub stopped waiting for [approvalId]: its dialog closes.
final class ApprovalCancelledMessage extends ServerMessage {
  const ApprovalCancelledMessage(this.approvalId);

  final int approvalId;
}

/// P124 — what a code engine does while a turn runs, mirroring `ChatEventDto`. The turn's end is still a
/// [ChatResponseMessage] or [ChatErrorMessage].
sealed class ChatEvent {
  const ChatEvent();

  /// Null for an event this app doesn't know yet (a newer hub): it is skipped, not a broken connection.
  static ChatEvent? fromJson(Map<String, dynamic> json) => switch (json['type']) {
        'text' => ChatEventText(json['text'] as String, source: json['source'] as String?),
        'tool' => ChatEventTool(
            callId: json['callId'] as String,
            tool: json['tool'] as String,
            title: json['title'] as String,
            status: switch (json['status']) {
              'completed' => ToolStatus.completed,
              'failed' => ToolStatus.failed,
              _ => ToolStatus.running,
            },
          ),
        'notice' => ChatEventNotice(json['text'] as String),
        _ => null,
      };
}

final class ChatEventText extends ChatEvent {
  const ChatEventText(this.text, {this.source});

  final String text;

  /// Names the task the words are from when several are told at once (a manager's code tasks).
  final String? source;
}

enum ToolStatus { running, completed, failed }

final class ChatEventTool extends ChatEvent {
  const ChatEventTool({required this.callId, required this.tool, required this.title, required this.status});

  /// The same for each stage of one call, so a client updates one line instead of adding one.
  final String callId;
  final String tool;
  final String title;
  final ToolStatus status;
}

final class ChatEventNotice extends ChatEvent {
  const ChatEventNotice(this.text);

  final String text;
}

final class ChatEventMessage extends ServerMessage {
  const ChatEventMessage(this.conversationId, this.event);

  final String conversationId;
  final ChatEvent event;
}

/// One of this device's conversations changed outside a chat reply — an agent left a note for
/// another, or answered one (P46 `message_agent`).
final class ConversationsChangedMessage extends ServerMessage {
  const ConversationsChangedMessage(this.conversationId);

  final String conversationId;
}

/// P120 — the owner changed what this member may do with the organization of the agents. [access] is `none`, `view` or `edit`,
/// as the hub saved it. Pushed, not an answer to a request.
final class OrgAccessChangedMessage extends ServerMessage {
  const OrgAccessChangedMessage(this.access);

  final String access;
}

/// A message type this app doesn't handle.
final class UnknownServerMessage extends ServerMessage {
  const UnknownServerMessage(this.type);

  final String type;
}
