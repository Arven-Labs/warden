import 'dart:async';

import 'package:flutter/material.dart';

import '../l10n/generated/app_localizations.dart';
import '../protocol/messages.dart';
import '../services/activity.dart';
import '../services/agent_work.dart';
import '../services/approval_categories.dart';
import '../services/server_connection.dart' show HubRequestException;
import 'member_org_tab.dart';

/// P120, P123 — the organization of the agents and the work they hand each other, on the phone: the tree (a role and a
/// superior for each agent, add or remove one, open a chat with it or its tasks) and the background tasks (state, progress,
/// cost, pause, resume, stop). Every change asks for the hub's pairing key, which is never kept. The model policies and the
/// models limit of each agent are edited here too, through the same narrow edit as the tree. Mirrors the web's two screens.
class AgentsScreen extends StatefulWidget {
  const AgentsScreen({
    super.key,
    required this.backend,
    this.onOpenChat,
    this.onOpenConversation,
    this.onOpenChannel,
    this.memberOrg,
    this.memberOrgAccess = OrgAccess.none,
    this.refreshEvery = const Duration(seconds: 3),
  });

  final AgentsBackend backend;

  /// A member of the hub (P120): the Organization tab shows the owner's tree as the access they were given allows, by their own
  /// session, instead of the owner's tab. Null for the owner.
  final MemberOrgBackend? memberOrg;

  /// The access the hub sent at sign-in, shown until the tab has asked again.
  final OrgAccess memberOrgAccess;

  /// Starts a conversation with this agent; the screen closes after it. Null hides the action.
  final void Function(String agentId)? onOpenChat;

  /// Opens a conversation by id (an event of the feed, P121); the screen closes after it. Null leaves those events without a tap.
  final void Function(String conversationId)? onOpenConversation;

  /// Opens the channel of this agent (an event of the feed, P121); the screen closes after it. Null leaves those events without a tap.
  final void Function(String agentId)? onOpenChannel;

  /// How often the task list reloads while it is open. Null never reloads (tests).
  final Duration? refreshEvery;

  @override
  State<AgentsScreen> createState() => _AgentsScreenState();
}

class _AgentsScreenState extends State<AgentsScreen> with SingleTickerProviderStateMixin {
  late final TabController _tabs = TabController(length: 3, vsync: this);

  /// The agent whose tasks the Tasks tab shows (null: all).
  String? _taskAgent;

  @override
  void dispose() {
    _tabs.dispose();
    super.dispose();
  }

  void _openTasks(String agent) {
    setState(() => _taskAgent = agent);
    _tabs.animateTo(1);
  }

  @override
  Widget build(BuildContext context) {
    final l10n = AppLocalizations.of(context);
    return Scaffold(
      appBar: AppBar(
        title: Text(l10n.agentsTitle),
        bottom: TabBar(
          controller: _tabs,
          tabs: [
            Tab(key: const Key('tab-organization'), text: l10n.agentsTabOrganization),
            Tab(key: const Key('tab-tasks'), text: l10n.agentsTabTasks),
            Tab(key: const Key('tab-activity'), text: l10n.agentsTabActivity),
          ],
        ),
      ),
      body: TabBarView(
        controller: _tabs,
        children: [
          if (widget.memberOrg case final member?)
            MemberOrganizationTab(backend: member, initialAccess: widget.memberOrgAccess)
          else
            OrganizationTab(
              backend: widget.backend,
              onOpenChat: widget.onOpenChat == null
                  ? null
                  : (id) {
                      Navigator.of(context).pop();
                      widget.onOpenChat!(id);
                    },
              onOpenTasks: _openTasks,
            ),
          AgentTasksTab(
            backend: widget.backend,
            agent: _taskAgent,
            onClearAgent: () => setState(() => _taskAgent = null),
            refreshEvery: widget.refreshEvery,
          ),
          ActivityTab(
            backend: widget.backend,
            refreshEvery: widget.refreshEvery,
            onOpen: (to) {
              switch (to.target) {
                case ActivityTarget.tasks:
                  _openTasks(to.agent!);
                case ActivityTarget.conversation:
                  final open = widget.onOpenConversation;
                  if (open != null) {
                    Navigator.of(context).pop();
                    open(to.conversationId!);
                  }
                case ActivityTarget.channel:
                  final open = widget.onOpenChannel;
                  if (open != null) {
                    Navigator.of(context).pop();
                    open(to.agent!);
                  }
              }
            },
          ),
        ],
      ),
    );
  }
}

/// Asks for the pairing key and runs [submit] with it. A wrong key keeps the dialog open with a message; any other failure
/// closes it and goes to [onError]. Resolves true when [submit] succeeded.
Future<bool> askPairingKey(
  BuildContext context, {
  required String title,
  required String message,
  required Future<void> Function(String pairingKey) submit,
  required void Function(String error) onError,
}) async {
  final done = await showDialog<bool>(
    context: context,
    barrierDismissible: false,
    builder: (_) => _PairingKeyDialog(title: title, message: message, submit: submit, onError: onError),
  );
  return done ?? false;
}

class _PairingKeyDialog extends StatefulWidget {
  const _PairingKeyDialog({required this.title, required this.message, required this.submit, required this.onError});

  final String title;
  final String message;
  final Future<void> Function(String pairingKey) submit;
  final void Function(String error) onError;

  @override
  State<_PairingKeyDialog> createState() => _PairingKeyDialogState();
}

class _PairingKeyDialogState extends State<_PairingKeyDialog> {
  final _controller = TextEditingController();
  String? _keyError;
  bool _busy = false;

  @override
  void dispose() {
    _controller.dispose();
    super.dispose();
  }

  Future<void> _confirm() async {
    setState(() {
      _busy = true;
      _keyError = null;
    });
    try {
      await widget.submit(_controller.text);
      if (mounted) Navigator.of(context).pop(true);
    } on HubRequestException catch (e) {
      if (!mounted) return;
      if (e.authRejected) {
        setState(() {
          _busy = false;
          _keyError = AppLocalizations.of(context).pairingWrong;
        });
      } else {
        Navigator.of(context).pop(false);
        widget.onError(e.message);
      }
    } catch (e) {
      if (!mounted) return;
      Navigator.of(context).pop(false);
      widget.onError(e.toString());
    }
  }

  @override
  Widget build(BuildContext context) {
    final l10n = AppLocalizations.of(context);
    return AlertDialog(
      title: Text(widget.title),
      content: Column(
        mainAxisSize: MainAxisSize.min,
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          Text(widget.message),
          const SizedBox(height: 12),
          TextField(
            key: const Key('pairing-key-field'),
            controller: _controller,
            autofocus: true,
            obscureText: true,
            enabled: !_busy,
            decoration: InputDecoration(labelText: l10n.pairingLabel, helperText: l10n.pairingHelper, errorText: _keyError),
            onSubmitted: (_) => _controller.text.trim().isEmpty || _busy ? null : _confirm(),
          ),
        ],
      ),
      actions: [
        TextButton(onPressed: _busy ? null : () => Navigator.of(context).pop(false), child: Text(l10n.commonCancel)),
        FilledButton(
          key: const Key('pairing-key-confirm'),
          onPressed: _busy ? null : () => _controller.text.trim().isEmpty ? null : _confirm(),
          child: Text(_busy ? l10n.commonWait : l10n.commonConfirm),
        ),
      ],
    );
  }
}

// ---- Organization ----

/// The role and the superior of an agent. Owns its controller, so it is disposed only after the dialog's exit
/// animation (disposing it right after `showDialog` returns would break that animation).
class PositionDialog extends StatefulWidget {
  const PositionDialog({super.key, required this.agent, required this.choices});

  final AgentInfo agent;
  final List<AgentInfo> choices;

  @override
  State<PositionDialog> createState() => _PositionDialogState();
}

class _PositionDialogState extends State<PositionDialog> {
  late final TextEditingController _role = TextEditingController(text: widget.agent.role ?? '');
  late String _superior = widget.choices.any((a) => a.id == widget.agent.reportsTo) ? widget.agent.reportsTo! : '';

  @override
  void dispose() {
    _role.dispose();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    final l10n = AppLocalizations.of(context);
    return AlertDialog(
      title: Text(l10n.positionTitle(widget.agent.id)),
      content: Column(
        mainAxisSize: MainAxisSize.min,
        children: [
          TextField(
            key: const Key('role-field'),
            controller: _role,
            maxLength: 80,
            decoration: InputDecoration(labelText: l10n.positionRole, hintText: l10n.positionRoleHint),
          ),
          DropdownButtonFormField<String>(
            key: const Key('superior-field'),
            initialValue: _superior,
            decoration: InputDecoration(labelText: l10n.positionReportsTo, helperText: l10n.positionReportsHelper(widget.agent.id)),
            items: [
              DropdownMenuItem(value: '', child: Text(l10n.positionNobody)),
              for (final a in widget.choices) DropdownMenuItem(value: a.id, child: Text(a.id)),
            ],
            onChanged: (value) => setState(() => _superior = value ?? ''),
          ),
        ],
      ),
      actions: [
        TextButton(onPressed: () => Navigator.of(context).pop(), child: Text(l10n.commonCancel)),
        FilledButton(onPressed: () => Navigator.of(context).pop((role: _role.text, superior: _superior)), child: Text(l10n.commonSave)),
      ],
    );
  }
}

/// The models an agent may pick when it delegates (P123): checked, with a default (the first). Nothing checked leaves the choice open.
/// What [agent] may do on its own (P122): the autonomy level, the four permissions and the kinds of action that always ask for the
/// person's yes. One edit with one Save, because each edit restarts the hub and asks for the pairing key. Starts from what the agent
/// has now. Pops the [SetPowersEdit], or null on Cancel.
class _PowersDialog extends StatefulWidget {
  const _PowersDialog({required this.agent});

  final AgentInfo agent;

  @override
  State<_PowersDialog> createState() => _PowersDialogState();
}

class _PowersDialogState extends State<_PowersDialog> {
  late int _autonomy = widget.agent.autonomy;
  late bool _startTasks = widget.agent.canStartTasks;
  late bool _createWorkers = widget.agent.canCreateWorkers;
  late bool _messageUser = widget.agent.canMessageUser;
  late bool _chooseModels = widget.agent.canChooseModels;
  late final Set<String> _categories = {...widget.agent.approvalRequired};

  Widget _switch(String key, String label, bool value, ValueChanged<bool> onChanged) => SwitchListTile(
        key: Key(key),
        dense: true,
        contentPadding: EdgeInsets.zero,
        title: Text(label),
        value: value,
        onChanged: (on) => setState(() => onChanged(on)),
      );

  @override
  Widget build(BuildContext context) {
    final agent = widget.agent;
    final l10n = AppLocalizations.of(context);
    // An id only a newer hub knows is shown by its own id, so it does not vanish on Save.
    final unknown = [for (final id in _categories) if (!approvalCategoryIds.contains(id)) id];
    return AlertDialog(
      title: Text(l10n.powersTitle(agent.id)),
      content: SingleChildScrollView(
        child: Column(
          mainAxisSize: MainAxisSize.min,
          crossAxisAlignment: CrossAxisAlignment.start,
          children: [
            DropdownButtonFormField<int>(
              key: const Key('powers-autonomy'),
              initialValue: _autonomy,
              isExpanded: true,
              decoration: InputDecoration(labelText: l10n.powersAutonomy),
              items: [for (final n in autonomyLevels) DropdownMenuItem(value: n, child: Text(autonomyLabel(n, l10n), overflow: TextOverflow.ellipsis))],
              onChanged: (value) => setState(() => _autonomy = value ?? _autonomy),
            ),
            if (_autonomy == 5 && !agent.canManageAgents)
              Padding(
                padding: const EdgeInsets.only(top: 4),
                child: Text(l10n.powersLevel5(agent.id), style: Theme.of(context).textTheme.bodySmall),
              ),
            const SizedBox(height: 8),
            _switch('powers-start-tasks', l10n.powersStartTasks, _startTasks, (on) => _startTasks = on),
            _switch('powers-create-workers', l10n.powersCreateWorkers, _createWorkers, (on) => _createWorkers = on),
            _switch('powers-message-user', l10n.powersMessageUser, _messageUser, (on) => _messageUser = on),
            _switch('powers-choose-models', l10n.powersChooseModels, _chooseModels, (on) => _chooseModels = on),
            const SizedBox(height: 8),
            Text(l10n.powersAsks, style: Theme.of(context).textTheme.bodySmall),
            for (final id in approvalCategoryIds)
              CheckboxListTile(
                key: Key('powers-category-$id'),
                dense: true,
                contentPadding: EdgeInsets.zero,
                controlAffinity: ListTileControlAffinity.leading,
                title: Text(approvalCategoryLabel(id, l10n)),
                value: _categories.contains(id),
                onChanged: (on) => setState(() => on == true ? _categories.add(id) : _categories.remove(id)),
              ),
            for (final id in unknown)
              CheckboxListTile(
                dense: true,
                contentPadding: EdgeInsets.zero,
                controlAffinity: ListTileControlAffinity.leading,
                title: Text(approvalCategoryLabel(id, l10n)),
                value: true,
                onChanged: (_) => setState(() => _categories.remove(id)),
              ),
          ],
        ),
      ),
      actions: [
        TextButton(onPressed: () => Navigator.of(context).pop(), child: Text(l10n.commonCancel)),
        FilledButton(
          key: const Key('powers-save'),
          onPressed: () => Navigator.of(context).pop(
            SetPowersEdit(
              agent.id,
              autonomy: _autonomy,
              canStartTasks: _startTasks,
              canCreateWorkers: _createWorkers,
              canMessageUser: _messageUser,
              canChooseModels: _chooseModels,
              approvalRequired: orderedCategories(_categories),
            ),
          ),
          child: Text(l10n.commonSave),
        ),
      ],
    );
  }
}

class _LimitDialog extends StatefulWidget {
  const _LimitDialog({required this.agent, required this.candidates, required this.policyIds});

  final AgentInfo agent;
  final List<String> candidates;
  final Set<String> policyIds;

  @override
  State<_LimitDialog> createState() => _LimitDialogState();
}

class _LimitDialogState extends State<_LimitDialog> {
  late final Set<String> _checked = {...widget.agent.delegationModels};
  late String _default = widget.agent.delegationModels.isEmpty ? '' : widget.agent.delegationModels.first;

  List<String> get _chosen => [
        for (final id in widget.candidates)
          if (_checked.contains(id)) id,
      ];

  @override
  Widget build(BuildContext context) {
    final chosen = _chosen;
    final shownDefault = chosen.contains(_default) ? _default : (chosen.isEmpty ? '' : chosen.first);
    final l10n = AppLocalizations.of(context);
    return AlertDialog(
      title: Text(l10n.limitTitle(widget.agent.id)),
      content: SingleChildScrollView(
        child: Column(
          mainAxisSize: MainAxisSize.min,
          crossAxisAlignment: CrossAxisAlignment.start,
          children: [
            Text(l10n.limitHint, style: Theme.of(context).textTheme.bodySmall),
            if (widget.candidates.isEmpty) Text(l10n.limitNone),
            for (final id in widget.candidates)
              CheckboxListTile(
                key: Key('limit-$id'),
                dense: true,
                contentPadding: EdgeInsets.zero,
                controlAffinity: ListTileControlAffinity.leading,
                title: Text(widget.policyIds.contains(id) ? l10n.limitPolicy(id) : id),
                value: _checked.contains(id),
                onChanged: (on) => setState(() => on == true ? _checked.add(id) : _checked.remove(id)),
              ),
            if (chosen.length > 1)
              DropdownButtonFormField<String>(
                key: const Key('limit-default'),
                initialValue: shownDefault,
                decoration: InputDecoration(labelText: l10n.limitDefault, helperText: l10n.limitDefaultHelper(widget.agent.id)),
                items: [for (final id in chosen) DropdownMenuItem(value: id, child: Text(id))],
                onChanged: (value) => setState(() => _default = value ?? ''),
              ),
          ],
        ),
      ),
      actions: [
        TextButton(onPressed: () => Navigator.of(context).pop(), child: Text(l10n.commonCancel)),
        FilledButton(
          key: const Key('limit-save'),
          onPressed: () => Navigator.of(context).pop(limitModels(widget.candidates, _checked, shownDefault)),
          child: Text(l10n.commonSave),
        ),
      ],
    );
  }
}

/// A model policy (P123): a name an agent that delegates can say, what answers it, and when to pick it. Owns its controllers, so they are
/// disposed after the dialog's exit animation.
class _PolicyDialog extends StatefulWidget {
  const _PolicyDialog({required this.original, required this.modelIds, required this.initialId});

  final ModelPolicy? original;
  final List<String> modelIds;
  final String initialId;

  @override
  State<_PolicyDialog> createState() => _PolicyDialogState();
}

class _PolicyDialogState extends State<_PolicyDialog> {
  late final TextEditingController _id = TextEditingController(text: widget.initialId);
  late final TextEditingController _description = TextEditingController(text: widget.original?.description ?? '');
  late String _model = widget.modelIds.contains(widget.original?.model) ? widget.original!.model : (widget.modelIds.isEmpty ? '' : widget.modelIds.first);

  @override
  void dispose() {
    _id.dispose();
    _description.dispose();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    final ready = _id.text.trim().isNotEmpty && _model.isNotEmpty;
    final l10n = AppLocalizations.of(context);
    return AlertDialog(
      title: Text(widget.original == null ? l10n.policyNew : l10n.policyTitle(widget.original!.id)),
      content: SingleChildScrollView(
        child: Column(
          mainAxisSize: MainAxisSize.min,
          children: [
            TextField(
              key: const Key('policy-name-field'),
              controller: _id,
              maxLength: 64,
              decoration: InputDecoration(labelText: l10n.policyName),
              onChanged: (_) => setState(() {}),
            ),
            DropdownButtonFormField<String>(
              key: const Key('policy-model-field'),
              initialValue: _model.isEmpty ? null : _model,
              decoration: InputDecoration(labelText: l10n.policyAnsweredBy),
              items: [for (final id in widget.modelIds) DropdownMenuItem(value: id, child: Text(id))],
              onChanged: (value) => setState(() => _model = value ?? ''),
            ),
            TextField(
              key: const Key('policy-description-field'),
              controller: _description,
              maxLength: 200,
              decoration: InputDecoration(labelText: l10n.policyWhen, hintText: l10n.policyWhenHint),
            ),
          ],
        ),
      ),
      actions: [
        TextButton(onPressed: () => Navigator.of(context).pop(), child: Text(l10n.commonCancel)),
        FilledButton(
          key: const Key('policy-save'),
          onPressed: ready ? () => Navigator.of(context).pop(ModelPolicy(id: _id.text, model: _model, description: _description.text)) : null,
          child: Text(l10n.policySave),
        ),
      ],
    );
  }
}

/// A new agent: name, role and what it does. Starts careful (the hub decides what that means).
class NewAgentDialog extends StatefulWidget {
  const NewAgentDialog({super.key, required this.under});

  final String? under;

  @override
  State<NewAgentDialog> createState() => _NewAgentDialogState();
}

class _NewAgentDialogState extends State<NewAgentDialog> {
  final _id = TextEditingController();
  final _role = TextEditingController();
  final _persona = TextEditingController();

  @override
  void dispose() {
    _id.dispose();
    _role.dispose();
    _persona.dispose();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    final ready = _id.text.trim().isNotEmpty && _persona.text.trim().isNotEmpty;
    final l10n = AppLocalizations.of(context);
    return AlertDialog(
      title: Text(widget.under == null ? l10n.newAgentTop : l10n.newAgentUnder(widget.under!)),
      content: SingleChildScrollView(
        child: Column(
          mainAxisSize: MainAxisSize.min,
          children: [
            TextField(
              key: const Key('new-name-field'),
              controller: _id,
              maxLength: 64,
              decoration: InputDecoration(labelText: l10n.newAgentName, hintText: l10n.newAgentNameHint),
              onChanged: (_) => setState(() {}),
            ),
            TextField(
              key: const Key('new-role-field'),
              controller: _role,
              maxLength: 80,
              decoration: InputDecoration(labelText: l10n.newAgentRole),
            ),
            TextField(
              key: const Key('new-persona-field'),
              controller: _persona,
              minLines: 2,
              maxLines: 5,
              decoration: InputDecoration(
                labelText: l10n.newAgentDoes,
                helperText: l10n.newAgentHelper,
                helperMaxLines: 4,
              ),
              onChanged: (_) => setState(() {}),
            ),
          ],
        ),
      ),
      actions: [
        TextButton(onPressed: () => Navigator.of(context).pop(), child: Text(l10n.commonCancel)),
        FilledButton(
          key: const Key('new-agent-confirm'),
          onPressed: ready ? () => Navigator.of(context).pop((id: _id.text, role: _role.text, persona: _persona.text)) : null,
          child: Text(l10n.newAgentAdd),
        ),
      ],
    );
  }
}

class OrganizationTab extends StatefulWidget {
  const OrganizationTab({super.key, required this.backend, required this.onOpenTasks, this.onOpenChat});

  final AgentsBackend backend;
  final void Function(String agentId) onOpenTasks;
  final void Function(String agentId)? onOpenChat;

  @override
  State<OrganizationTab> createState() => _OrganizationTabState();
}

class _OrganizationTabState extends State<OrganizationTab> {
  HubAgents? _hub;
  String? _error;

  /// The hub's tasks, for the activity line of each node (an extra: without them the card just shows no line).
  List<AgentTask> _tasks = const [];
  AgentActivityExtras _extras = const AgentActivityExtras();

  @override
  void initState() {
    super.initState();
    _load();
  }

  Future<void> _load() async {
    try {
      final hub = await widget.backend.listHubAgents();
      if (mounted) {
        setState(() {
          _hub = hub;
          _error = null;
        });
      }
    } catch (e) {
      if (mounted) setState(() => _error = e.toString());
    }
    try {
      final tasks = await widget.backend.listAgentTasks();
      if (mounted) setState(() => _tasks = tasks);
    } catch (_) {
      // The activity is an extra.
    }
    try {
      final extras = await widget.backend.activityExtras();
      if (mounted) setState(() => _extras = extras);
    } catch (_) {
      // So are the spending and the last conversation.
    }
  }

  /// What [agent] has been up to, on one line; null when nothing involves it.
  String? _activity(String agent) {
    final activity = activityOf(
      _tasks,
      agent,
      spend: spendOf(_extras.spend, agent),
      lastConversationMs: lastConversationOf(_extras.conversations, agent),
    );
    return activity == null ? null : activityLine(activity, DateTime.now().millisecondsSinceEpoch, AppLocalizations.of(context));
  }

  /// Asks for the key and applies [edit]; the screen shows what the hub holds afterwards.
  Future<void> _apply(OrgEdit edit, String what) async {
    setState(() => _error = null);
    await askPairingKey(
      context,
      title: what,
      message: AppLocalizations.of(context).orgRestarts,
      submit: (key) async {
        final hub = await widget.backend.editAgentOrg(key, edit);
        if (mounted) setState(() => _hub = hub);
      },
      onError: (error) {
        if (mounted) setState(() => _error = error);
      },
    );
  }

  Future<void> _edit(AgentInfo agent, List<AgentInfo> all) async {
    final picked = await showDialog<({String role, String superior})>(
      context: context,
      builder: (_) => PositionDialog(agent: agent, choices: superiorChoices(all, agent.id)),
    );
    if (picked == null || !mounted) return;
    await _apply(SetPositionEdit(agent.id, role: picked.role, reportsTo: picked.superior), AppLocalizations.of(context).orgChange(agent.id));
  }

  Future<void> _add(String? under) async {
    final picked = await showDialog<({String id, String role, String persona})>(
      context: context,
      builder: (_) => NewAgentDialog(under: under),
    );
    if (picked == null || !mounted) return;
    await _apply(AddReportEdit(picked.id, picked.persona, role: picked.role, reportsTo: under), AppLocalizations.of(context).orgAdd(picked.id));
  }

  /// What [agent] may do on its own (P122): autonomy, permissions and the kinds of action that ask for the person's yes.
  Future<void> _powers(AgentInfo agent) async {
    final picked = await showDialog<SetPowersEdit>(context: context, builder: (_) => _PowersDialog(agent: agent));
    if (picked == null || !mounted) return;
    await _apply(picked, AppLocalizations.of(context).powersTitle(agent.id));
  }

  /// The models [agent] may pick when it delegates (P123).
  Future<void> _limit(AgentInfo agent) async {
    final hub = _hub;
    if (hub == null) return;
    final picked = await showDialog<List<String>>(
      context: context,
      builder: (_) => _LimitDialog(agent: agent, candidates: delegationCandidates(hub.modelIds, hub.modelPolicies), policyIds: {for (final p in hub.modelPolicies) p.id}),
    );
    if (picked == null || !mounted) return;
    await _apply(SetDelegationModelsEdit(agent.id, picked), AppLocalizations.of(context).limitTitle(agent.id));
  }

  /// A model policy (P123): [original] is the one being edited, null for a new one.
  Future<void> _policy(ModelPolicy? original) async {
    final hub = _hub;
    if (hub == null) return;
    final picked = await showDialog<ModelPolicy>(
      context: context,
      builder: (_) => _PolicyDialog(
        original: original,
        modelIds: hub.modelIds,
        initialId: original?.id ?? nextPolicyId(delegationCandidates(hub.modelIds, hub.modelPolicies)),
      ),
    );
    if (picked == null || !mounted) return;
    await _apply(SetModelPoliciesEdit(policiesWith(hub.modelPolicies, picked, original?.id)), AppLocalizations.of(context).orgSavePolicy(picked.id.trim()));
  }

  Future<void> _removePolicy(ModelPolicy policy) async {
    final hub = _hub;
    if (hub == null) return;
    await _apply(SetModelPoliciesEdit(policiesWithout(hub.modelPolicies, policy.id)), AppLocalizations.of(context).orgRemovePolicy(policy.id));
  }

  Future<void> _remove(OrgNode node) async {
    final agent = node.agent;
    final count = node.children.length;
    final l10n = AppLocalizations.of(context);
    final goesTo = agent.reportsTo ?? l10n.orgNobodyTop;
    final ok = await showDialog<bool>(
      context: context,
      builder: (dialogContext) => AlertDialog(
        title: Text(l10n.orgRemoveTitle(agent.id)),
        content: Text(l10n.orgRemoveBody(count, goesTo)),
        actions: [
          TextButton(onPressed: () => Navigator.of(dialogContext).pop(false), child: Text(l10n.commonKeep)),
          FilledButton(onPressed: () => Navigator.of(dialogContext).pop(true), child: Text(l10n.commonRemove)),
        ],
      ),
    );
    if (ok != true || !mounted) return;
    await _apply(RemoveAgentEdit(agent.id), l10n.orgRemove(agent.id));
  }

  List<String> _badges(AgentInfo a, AppLocalizations l10n) => [
        if (a.canDelegateToAgents) l10n.badgeDelegates,
        if (a.canManageAgents) l10n.badgeManages,
        if (a.canMessageAgents) l10n.badgeNotes,
        if (a.canManageTasks) l10n.badgeTasks,
        // On by default: only what was taken away shows.
        if (!a.canStartTasks) l10n.badgeNoBackground,
        if (!a.canCreateWorkers) l10n.badgeNoTemp,
        if (!a.canMessageUser) l10n.badgeNoChat,
        if (!a.canChooseModels) l10n.badgeNoModels,
        if (a.autonomy != 4) l10n.badgeAutonomy(a.autonomy, autonomyShortLabel(a.autonomy, l10n)).trim(),
        if (a.approvalRequired.isNotEmpty) l10n.badgeAsksFirst(a.approvalRequired.length),
      ];

  List<Widget> _rows(List<OrgNode> nodes, List<AgentInfo> all, int depth) {
    final l10n = AppLocalizations.of(context);
    final rows = <Widget>[];
    for (final node in nodes) {
      final agent = node.agent;
      rows.add(
        Padding(
          padding: EdgeInsets.only(left: depth * 16.0),
          child: Card(
            key: Key('agent-${agent.id}'),
            child: ListTile(
              title: Text(agent.id),
              subtitle: Column(
                crossAxisAlignment: CrossAxisAlignment.start,
                children: [
                  if (agent.role != null && agent.role!.isNotEmpty) Text(agent.role!),
                  if (_badges(agent, l10n).isNotEmpty) Wrap(spacing: 4, children: [for (final b in _badges(agent, l10n)) Chip(label: Text(b), visualDensity: VisualDensity.compact)]),
                  if (agent.canDelegateToAgents) Text(delegationSummary(agent.delegationModels, l10n), style: Theme.of(context).textTheme.bodySmall),
                  if (node.children.isNotEmpty) Text(l10n.orgReports(node.children.length), style: Theme.of(context).textTheme.bodySmall),
                  if (_activity(agent.id) != null) Text(_activity(agent.id)!, key: Key('activity-${agent.id}'), style: Theme.of(context).textTheme.bodySmall),
                ],
              ),
              isThreeLine: true,
              trailing: PopupMenuButton<String>(
                key: Key('menu-${agent.id}'),
                onSelected: (action) {
                  switch (action) {
                    case 'chat':
                      widget.onOpenChat?.call(agent.id);
                    case 'tasks':
                      widget.onOpenTasks(agent.id);
                    case 'edit':
                      _edit(agent, all);
                    case 'powers':
                      _powers(agent);
                    case 'models':
                      _limit(agent);
                    case 'add':
                      _add(agent.id);
                    case 'remove':
                      _remove(node);
                  }
                },
                itemBuilder: (_) => [
                  if (widget.onOpenChat != null) PopupMenuItem(value: 'chat', child: Text(l10n.menuChat)),
                  PopupMenuItem(value: 'tasks', child: Text(l10n.menuTasks)),
                  PopupMenuItem(value: 'edit', child: Text(l10n.menuEditPosition)),
                  PopupMenuItem(value: 'powers', child: Text(l10n.menuPowers)),
                  if (agent.canDelegateToAgents) PopupMenuItem(value: 'models', child: Text(l10n.menuModels)),
                  PopupMenuItem(value: 'add', child: Text(l10n.menuAddReport)),
                  PopupMenuItem(value: 'remove', child: Text(l10n.commonRemove)),
                ],
              ),
            ),
          ),
        ),
      );
      rows.addAll(_rows(node.children, all, depth + 1));
    }
    return rows;
  }

  @override
  Widget build(BuildContext context) {
    final hub = _hub;
    if (hub == null) {
      return Center(
        child: _error != null
            ? Padding(padding: const EdgeInsets.all(16), child: Text(_error!, style: TextStyle(color: Theme.of(context).colorScheme.error)))
            : const CircularProgressIndicator(),
      );
    }
    final tree = buildOrg(hub.agents);
    final l10n = AppLocalizations.of(context);
    return RefreshIndicator(
      onRefresh: _load,
      child: ListView(
        padding: const EdgeInsets.all(12),
        children: [
          Text(l10n.orgHint, style: Theme.of(context).textTheme.bodySmall),
          if (_error != null) Padding(padding: const EdgeInsets.only(top: 8), child: Text(_error!, style: TextStyle(color: Theme.of(context).colorScheme.error))),
          const SizedBox(height: 8),
          if (tree.isEmpty) Text(l10n.orgNone) else ..._rows(tree, hub.agents, 0),
          if (tree.isNotEmpty && tree.every((n) => n.children.isEmpty))
            Padding(padding: const EdgeInsets.symmetric(vertical: 8), child: Text(l10n.orgNobodyReports)),
          TextButton.icon(key: const Key('add-top'), onPressed: () => _add(null), icon: const Icon(Icons.add), label: Text(l10n.orgAddTop)),
          const Divider(),
          Text(l10n.policiesTitle, style: Theme.of(context).textTheme.titleSmall),
          if (hub.modelPolicies.isEmpty) Text(l10n.policiesNone),
          for (final p in hub.modelPolicies)
            ListTile(
              key: Key('policy-${p.id}'),
              dense: true,
              contentPadding: EdgeInsets.zero,
              title: Text('${p.id} → ${p.model}'),
              subtitle: p.description == null || p.description!.isEmpty ? null : Text(p.description!),
              trailing: PopupMenuButton<String>(
                key: Key('policy-menu-${p.id}'),
                onSelected: (action) => action == 'edit' ? _policy(p) : _removePolicy(p),
                itemBuilder: (_) => [
                  PopupMenuItem(value: 'edit', child: Text(l10n.commonEdit)),
                  PopupMenuItem(value: 'remove', child: Text(l10n.commonRemove)),
                ],
              ),
            ),
          TextButton.icon(
            key: const Key('new-policy'),
            onPressed: hub.modelIds.isEmpty ? null : () => _policy(null),
            icon: const Icon(Icons.add),
            label: Text(l10n.policyNew),
          ),
        ],
      ),
    );
  }
}

// ---- Tasks ----

String _clock(int ms) {
  final t = DateTime.fromMillisecondsSinceEpoch(ms);
  String two(int n) => n.toString().padLeft(2, '0');
  return '${two(t.day)}/${two(t.month)} ${two(t.hour)}:${two(t.minute)}';
}

class AgentTasksTab extends StatefulWidget {
  const AgentTasksTab({super.key, required this.backend, required this.agent, required this.onClearAgent, this.refreshEvery});

  final AgentsBackend backend;
  final String? agent;
  final VoidCallback onClearAgent;
  final Duration? refreshEvery;

  @override
  State<AgentTasksTab> createState() => _AgentTasksTabState();
}

class _AgentTasksTabState extends State<AgentTasksTab> {
  List<AgentTask>? _tasks;
  GrantsView? _grants;
  String? _error;
  Timer? _timer;

  @override
  void initState() {
    super.initState();
    _load();
    final every = widget.refreshEvery;
    if (every != null) _timer = Timer.periodic(every, (_) => _load());
  }

  @override
  void dispose() {
    _timer?.cancel();
    super.dispose();
  }

  Future<void> _load() async {
    try {
      final tasks = await widget.backend.listAgentTasks();
      if (mounted) {
        setState(() {
          _tasks = tasks;
          _error = null;
        });
      }
    } catch (e) {
      if (mounted) setState(() => _error = e.toString());
    }
    try {
      final grants = await widget.backend.listGrants();
      if (mounted) setState(() => _grants = grants);
    } catch (_) {
      // The permissions are an extra: a hub that predates them just shows none.
    }
  }

  /// P122 — takes one "always" back so the agent asks again. It only makes the agent more careful, so no pairing key.
  Future<void> _revoke(Grant grant) async {
    try {
      final grants = await widget.backend.revokeGrant(grant);
      if (mounted) setState(() => _grants = grants);
    } catch (e) {
      if (mounted) setState(() => _error = e.toString());
    }
  }

  Widget _grantsSection() {
    final view = _grants;
    if (view == null) return const SizedBox.shrink();
    final grants = view.grants;
    final ttlDays = view.ttlDays;
    final l10n = AppLocalizations.of(context);
    return Column(
      key: const Key('grants-section'),
      crossAxisAlignment: CrossAxisAlignment.start,
      children: [
        const SizedBox(height: 16),
        Text(l10n.grantsTitle, style: Theme.of(context).textTheme.titleSmall),
        const SizedBox(height: 4),
        Text(ttlDays != null ? l10n.grantsHintKept(ttlDays) : l10n.grantsHintSession, style: Theme.of(context).textTheme.bodySmall),
        if (grants.isEmpty) Padding(padding: const EdgeInsets.symmetric(vertical: 8), child: Text(l10n.grantsNone)),
        for (final g in grants)
          ListTile(
            key: Key('grant-${g.conversationId}-${g.agentId ?? ''}-${g.covers}'),
            contentPadding: EdgeInsets.zero,
            title: Text(grantCategoryLabel(g.covers, l10n)),
            subtitle: Text([
              g.agentId ?? l10n.grantDefaultAgent,
              l10n.grantConversation(g.conversationId.length > 8 ? g.conversationId.substring(0, 8) : g.conversationId),
              if (g.lastUsedMs != null) l10n.grantUsed(agoLabel(g.lastUsedMs!, DateTime.now().millisecondsSinceEpoch, l10n)),
            ].join(' · ')),
            trailing: TextButton(key: Key('revoke-${g.conversationId}-${g.agentId ?? ''}-${g.covers}'), onPressed: () => _revoke(g), child: Text(l10n.grantTakeBack)),
          ),
      ],
    );
  }

  Future<void> _control(AgentTask task, String action) async {
    setState(() => _error = null);
    final l10n = AppLocalizations.of(context);
    await askPairingKey(
      context,
      title: l10n.taskControlTitle(taskActionLabel(action, l10n), task.assignee),
      message: action == 'cancel' ? l10n.taskControlStops : l10n.taskControlOnly,
      submit: (key) async {
        final tasks = await widget.backend.controlAgentTask(key, task.id, action);
        if (mounted) setState(() => _tasks = tasks);
      },
      onError: (error) {
        if (mounted) setState(() => _error = error);
      },
    );
  }

  Widget _header(TaskGroup g) {
    final c = g.counts;
    final l10n = AppLocalizations.of(context);
    final parts = [
      if (c['done']! > 0) l10n.tallyDone(c['done']!),
      if (c['running']! > 0) l10n.tallyRunning(c['running']!),
      if (c['waiting']! > 0) l10n.tallyWaiting(c['waiting']!),
      if (c['paused']! > 0) l10n.tallyPaused(c['paused']!),
      if (c['pending']! > 0) l10n.tallyPending(c['pending']!),
      if (c['failed']! > 0) l10n.tallyFailed(c['failed']!),
      if (c['cancelled']! > 0) l10n.tallyStopped(c['cancelled']!),
    ];
    return Padding(
      padding: const EdgeInsets.fromLTRB(4, 12, 4, 4),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          Row(children: [
            Text(g.owner ?? l10n.taskAnAgent, style: Theme.of(context).textTheme.titleSmall),
            const SizedBox(width: 8),
            Text(_clock(g.createdAtMs), style: Theme.of(context).textTheme.bodySmall),
            if (g.active) ...[const SizedBox(width: 8), Text(l10n.taskWorking)],
          ]),
          const SizedBox(height: 4),
          LinearProgressIndicator(value: g.percent / 100),
          const SizedBox(height: 4),
          Text(
            '${l10n.taskProgress(g.finished, g.total, g.percent, parts.join(', '))}${g.totalTokens > 0 ? ' · ${l10n.tallyTokens(formatTokens(g.totalTokens))}' : ''}',
            style: Theme.of(context).textTheme.bodySmall,
          ),
        ],
      ),
    );
  }

  Widget _row(TaskRow row) {
    final task = row.task;
    final l10n = AppLocalizations.of(context);
    final duration = durationLabel(task, DateTime.now().millisecondsSinceEpoch);
    final detail = task.state == 'done' ? task.result : task.error;
    final meta = [
      if (task.model != null) task.model!,
      ?duration,
      if (task.totalTokens != null) l10n.taskTok(formatTokens(task.totalTokens!)),
    ].join(' · ');
    return Padding(
      padding: EdgeInsets.only(left: row.depth * 16.0),
      child: Card(
        key: Key('task-${task.id}'),
        child: ExpansionTile(
          leading: Text(taskStateMark[task.state] ?? taskStateMark['pending']!, semanticsLabel: taskStateLabel(task.state, l10n)),
          title: Text(task.assignee),
          subtitle: Text(
            [task.objective, if (meta.isNotEmpty) meta].join('\n'),
            maxLines: 3,
            overflow: TextOverflow.ellipsis,
          ),
          childrenPadding: const EdgeInsets.fromLTRB(16, 0, 16, 12),
          expandedCrossAxisAlignment: CrossAxisAlignment.start,
          children: [
            Text(l10n.taskDetailTask, style: Theme.of(context).textTheme.labelSmall),
            SelectableText(task.objective),
            const SizedBox(height: 8),
            Text(task.state == 'done' ? l10n.taskDetailResult : l10n.taskDetailWhy, style: Theme.of(context).textTheme.labelSmall),
            detail != null && detail.isNotEmpty ? SelectableText(detail) : Text(l10n.taskNoResult(taskStateLabel(task.state, l10n))),
            Wrap(
              spacing: 8,
              children: [
                for (final action in actionsFor(task))
                  TextButton(key: Key('task-${task.id}-$action'), onPressed: () => _control(task, action), child: Text(taskActionLabel(action, l10n))),
              ],
            ),
          ],
        ),
      ),
    );
  }

  @override
  Widget build(BuildContext context) {
    final tasks = _tasks;
    final agent = widget.agent;
    if (tasks == null) {
      return Center(
        child: _error != null
            ? Padding(padding: const EdgeInsets.all(16), child: Text(_error!, style: TextStyle(color: Theme.of(context).colorScheme.error)))
            : const CircularProgressIndicator(),
      );
    }
    final groups = groupTasks(agent == null ? tasks : involvingAgent(tasks, agent));
    final l10n = AppLocalizations.of(context);
    return RefreshIndicator(
      onRefresh: _load,
      child: ListView(
        padding: const EdgeInsets.all(12),
        children: [
          Text(l10n.tasksHint, style: Theme.of(context).textTheme.bodySmall),
          if (agent != null)
            Row(children: [
              Expanded(child: Text(l10n.tasksOnly(agent), style: Theme.of(context).textTheme.bodySmall)),
              TextButton(key: const Key('show-all-tasks'), onPressed: widget.onClearAgent, child: Text(l10n.tasksShowAll)),
            ]),
          if (_error != null) Padding(padding: const EdgeInsets.only(top: 8), child: Text(_error!, style: TextStyle(color: Theme.of(context).colorScheme.error))),
          if (groups.isEmpty)
            Padding(
              padding: const EdgeInsets.symmetric(vertical: 16),
              child: Text(agent != null ? l10n.tasksNoneFor(agent) : l10n.tasksNone),
            ),
          for (final g in groups) ...[_header(g), for (final row in g.rows) _row(row)],
          _grantsSection(),
        ],
      ),
    );
  }
}

/// P121 — the feed of activity: who did what among the agents, newest first, without opening each conversation. A tap goes to what
/// the event touches ([onOpen]): the agent's tasks, the conversation between two agents, or the agent's channel.
class ActivityTab extends StatefulWidget {
  const ActivityTab({super.key, required this.backend, required this.onOpen, this.refreshEvery});

  final AgentsBackend backend;
  final void Function(ActivityDestination to) onOpen;
  final Duration? refreshEvery;

  @override
  State<ActivityTab> createState() => _ActivityTabState();
}

class _ActivityTabState extends State<ActivityTab> {
  List<ActivityEvent>? _events;
  String? _error;
  String _agent = '';
  Timer? _timer;

  @override
  void initState() {
    super.initState();
    _load();
    final every = widget.refreshEvery;
    if (every != null) _timer = Timer.periodic(every, (_) => _load());
  }

  @override
  void dispose() {
    _timer?.cancel();
    super.dispose();
  }

  Future<void> _load() async {
    try {
      final events = await widget.backend.listActivity();
      if (mounted) {
        setState(() {
          _events = events;
          _error = null;
        });
      }
    } catch (e) {
      if (mounted) setState(() => _error = e.toString());
    }
  }

  String _time(int ms) {
    final t = DateTime.fromMillisecondsSinceEpoch(ms);
    String two(int n) => n.toString().padLeft(2, '0');
    return '${two(t.hour)}:${two(t.minute)}';
  }

  Widget _row(ActivityEvent event) {
    final to = activityDestination(event);
    return ListTile(
      key: Key('activity-${event.id}'),
      dense: true,
      leading: Text(activityMark(event.kind), style: Theme.of(context).textTheme.titleMedium),
      title: Text(activityHeadline(event, AppLocalizations.of(context))),
      subtitle: event.text.isEmpty ? Text(_time(event.atMs)) : Text('${_time(event.atMs)} · ${event.text}', maxLines: 3, overflow: TextOverflow.ellipsis),
      onTap: to == null ? null : () => widget.onOpen(to),
    );
  }

  @override
  Widget build(BuildContext context) {
    final events = _events;
    if (events == null) {
      return Center(
        child: _error != null
            ? Padding(padding: const EdgeInsets.all(16), child: Text(_error!, style: TextStyle(color: Theme.of(context).colorScheme.error)))
            : const CircularProgressIndicator(),
      );
    }
    final agents = activityAgents(events);
    // The filter names an agent that may leave the feed (its events aged out): then it shows everyone again.
    final chosen = agents.contains(_agent) ? _agent : '';
    final l10n = AppLocalizations.of(context);
    final days = activityByDay(chosen.isEmpty ? events : activityInvolving(events, chosen), DateTime.now(), l10n);
    return RefreshIndicator(
      onRefresh: _load,
      child: ListView(
        padding: const EdgeInsets.all(12),
        children: [
          Text(l10n.activityHint, style: Theme.of(context).textTheme.bodySmall),
          const SizedBox(height: 8),
          DropdownButton<String>(
            key: const Key('activity-filter'),
            value: chosen,
            isExpanded: true,
            items: [
              DropdownMenuItem(value: '', child: Text(l10n.activityAll)),
              for (final name in agents) DropdownMenuItem(value: name, child: Text(name)),
            ],
            onChanged: (value) => setState(() => _agent = value ?? ''),
          ),
          if (_error != null) Padding(padding: const EdgeInsets.only(top: 8), child: Text(_error!, style: TextStyle(color: Theme.of(context).colorScheme.error))),
          if (days.isEmpty)
            Padding(
              padding: const EdgeInsets.symmetric(vertical: 16),
              child: Text(chosen.isNotEmpty ? l10n.activityNothingFor(chosen) : l10n.activityNothing),
            ),
          for (final day in days) ...[
            Padding(
              padding: const EdgeInsets.fromLTRB(4, 12, 4, 4),
              child: Text(day.label, style: Theme.of(context).textTheme.titleSmall),
            ),
            for (final event in day.events) _row(event),
          ],
        ],
      ),
    );
  }
}
