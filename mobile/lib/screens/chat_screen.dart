import 'dart:async';
import 'dart:convert';
import 'dart:io';

import 'package:audioplayers/audioplayers.dart';
import 'package:flutter/material.dart';
import 'package:path_provider/path_provider.dart';
import 'package:video_player/video_player.dart';

import '../l10n/generated/app_localizations.dart';
import '../protocol/messages.dart';
import '../services/agent_work.dart' show orgAccessOf;
import '../services/chat_notifications.dart';
import '../services/chat_transcript.dart';
import '../services/conversation_groups.dart';
import '../services/live_turn.dart';
import '../services/mobile_file_tool.dart';
import '../services/server_connection.dart';
import '../services/side_work.dart';
import '../services/workdir.dart';
import '../services/sync_auto_pull.dart';
import '../services/vault_paths.dart';
import '../src/rust/api/sync.dart' as sync_bridge;
import 'agents_screen.dart';
import 'device_invites_screen.dart';
import 'usage_screen.dart';
import 'attachment_kind.dart';
import 'folder_picker.dart';
import 'language_menu.dart';
import 'theme_button.dart';

/// Phase 7.3: the real chat UI, built on top of the connection 7.2 proved works. The transcript is
/// kept in memory by the [ChatTranscript] the caller passes in (P41: it outlives this screen, so
/// leaving with the back button and resuming keeps the conversation), which since P40 also starts
/// with the conversation `warden-server` persisted for this device, so reopening the app (or
/// reconnecting) picks up where it left off.
class ChatScreen extends StatefulWidget {
  const ChatScreen({super.key, required this.connection, required this.transcript});

  final ServerConnection connection;
  final ChatTranscript transcript;

  @override
  State<ChatScreen> createState() => _ChatScreenState();
}

class _ChatScreenState extends State<ChatScreen> with WidgetsBindingObserver {
  final _inputController = TextEditingController();
  final _scrollController = ScrollController();

  /// P121 — the note or run of an agent that was opened from its list: the way back to the list while it is the open conversation.
  ({String agent, String id})? _workOpen;

  StreamSubscription<ServerMessage>? _chatSubscription;
  StreamSubscription<ConnectionStatus>? _statusSubscription;
  StreamSubscription<ServerMessage>? _approvalSubscription;

  /// P87 — approvals the hub is waiting on, oldest first, and the one whose dialog is open.
  final _approvals = <ApprovalRequestMessage>[];
  int? _approvalShown;

  ConnectionStatus _status = const Disconnected();
  AppLifecycleState _lifecycleState = AppLifecycleState.resumed;
  bool _autoPulling = false;

  /// P168 — the hosted credit under the title; null on a hub that has none and for a member, whose report carries no credit.
  HostedCredit? _credit;
  bool _wasWaiting = false;

  @override
  void initState() {
    super.initState();
    WidgetsBinding.instance.addObserver(this);
    widget.transcript.addListener(_refreshCreditWhenTurnEnds);
    unawaited(_refreshCredit());
    unawaited(requestNotificationPermission());
    _status = widget.connection.status;
    _chatSubscription = widget.connection.chatStream.listen(_onChatMessage);
    _statusSubscription = widget.connection.statusStream.listen((s) {
      if (mounted) setState(() => _status = s);
    });
    _approvalSubscription = widget.connection.approvalStream.listen(_onApproval);
  }

  /// P168 — read on opening and each time the turn being waited on ends, since a turn is what spends the credit. Never fails the chat.
  Future<void> _refreshCredit() async {
    // The owner's only: the credit belongs to the instance, and the hub is not asked for a member's usage here for it.
    if (widget.connection.user != null) return;
    try {
      final summary = await widget.connection.usageSummary();
      if (mounted) setState(() => _credit = summary.credit);
    } catch (_) {}
  }

  void _refreshCreditWhenTurnEnds() {
    final waiting = widget.transcript.waitingForReply;
    if (_wasWaiting && !waiting) unawaited(_refreshCredit());
    _wasWaiting = waiting;
  }

  /// P87 — a tool in this device's turn needs the person's yes (an agent creating another, an SSH
  /// host with approval, a spending-limit pause). One dialog at a time; the hub gives up (and says
  /// so) after 120 s, which closes it.
  void _onApproval(ServerMessage msg) {
    switch (msg) {
      case ApprovalRequestMessage():
        _approvals.add(msg);
        if (shouldNotifyFor(_lifecycleState)) {
          unawaited(showChatNotification(msg, serverName: widget.connection.serverName));
        }
        _showNextApproval();
      case ApprovalCancelledMessage(:final approvalId):
        _approvals.removeWhere((a) => a.approvalId == approvalId);
        if (_approvalShown == approvalId && mounted) Navigator.of(context).pop();
      default:
        break;
    }
  }

  Future<void> _showNextApproval() async {
    if (_approvalShown != null || _approvals.isEmpty || !mounted) return;
    final request = _approvals.first;
    _approvalShown = request.approvalId;
    final l10n = AppLocalizations.of(context);
    final approved = await showDialog<bool>(
      context: context,
      barrierDismissible: false,
      builder: (dialogContext) => AlertDialog(
        title: Text(l10n.chatApprovalTitle),
        content: Column(
          mainAxisSize: MainAxisSize.min,
          crossAxisAlignment: CrossAxisAlignment.start,
          children: [
            Text('${request.action}: ${request.target}', style: const TextStyle(fontWeight: FontWeight.bold)),
            if (request.category != null) ...[
              const SizedBox(height: 4),
              Text(l10n.chatApprovalCategory(request.category!)),
            ],
            if (request.detail.isNotEmpty) ...[const SizedBox(height: 8), Text(request.detail)],
          ],
        ),
        actions: [
          TextButton(onPressed: () => Navigator.of(dialogContext).pop(false), child: Text(l10n.chatDeny)),
          FilledButton(onPressed: () => Navigator.of(dialogContext).pop(true), child: Text(l10n.chatApprove)),
        ],
      ),
    );
    // Null: the hub gave up on it first (ApprovalCancelled closed the dialog).
    if (approved != null) widget.connection.resolveApproval(request.approvalId, approved);
    _approvals.removeWhere((a) => a.approvalId == request.approvalId);
    _approvalShown = null;
    _showNextApproval();
  }

  @override
  void dispose() {
    WidgetsBinding.instance.removeObserver(this);
    widget.transcript.removeListener(_refreshCreditWhenTurnEnds);
    _chatSubscription?.cancel();
    _statusSubscription?.cancel();
    _approvalSubscription?.cancel();
    _inputController.dispose();
    _scrollController.dispose();
    super.dispose();
  }

  @override
  void didChangeAppLifecycleState(AppLifecycleState state) {
    if (shouldAutoPullOnResume(_lifecycleState, state)) {
      unawaited(_autoPullOnResume());
    }
    _lifecycleState = state;
    // P121 — an agent's channel is read only while the app is on screen; with it away, a message that comes is unread and gets a notification.
    widget.transcript.setForeground(state == AppLifecycleState.resumed);
  }

  /// P71 slice 2 — pulls sync changes silently when the app comes back to the foreground,
  /// mirroring the desktop's periodic `spawn_auto_pull` (Phase 4.7 slice 1) as closely as a
  /// process without a long-lived background daemon allows. Skips silently when sync was never
  /// paired on this device (`bridgeStatus` is safe to call even then — see
  /// `warden-sync::SyncEngine::status`); any other failure (e.g. no network) is only logged, never
  /// surfaced, same posture as the desktop's `eprintln!`.
  Future<void> _autoPullOnResume() async {
    if (_autoPulling) return;
    _autoPulling = true;
    try {
      final paths = await VaultPaths.resolve();
      final status = sync_bridge.bridgeStatus(
        vaultRoot: paths.vaultRoot,
        configPath: paths.configPath,
        secretsPath: paths.secretsPath,
        manifestPath: paths.manifestPath,
      );
      if (!status.paired) return;
      final result = await sync_bridge.bridgePull(
        vaultRoot: paths.vaultRoot,
        configPath: paths.configPath,
        secretsPath: paths.secretsPath,
        manifestPath: paths.manifestPath,
      );
      final message = autoPullMessageFor(
        filesWritten: result.filesWritten,
        filesDeleted: result.filesDeleted,
        configUpdated: result.configUpdated,
      );
      if (message != null && mounted) {
        ScaffoldMessenger.of(context).showSnackBar(SnackBar(content: Text(AppLocalizations.of(context).chatSyncSnack(message))));
      }
    } catch (e) {
      debugPrint('mobile: auto-pull on resume failed: $e');
    } finally {
      _autoPulling = false;
    }
  }

  /// The transcript itself is updated by `ChatTranscript`; this screen only reacts to a new reply
  /// while it's visible (scroll, local notification when the app is in the background).
  void _onChatMessage(ServerMessage msg) {
    if (!mounted) return;
    _scrollToBottom();
    if (shouldNotifyFor(_lifecycleState)) {
      unawaited(showChatNotification(msg, serverName: widget.connection.serverName));
    }
  }

  void _send() {
    if (_status is! Connected) return;
    if (widget.transcript.send(_inputController.text)) {
      _inputController.clear();
      _scrollToBottom();
    }
  }

  /// P157 — turns the person's message at [index] into a box to change it. Sending it replaces that message and all that followed, so
  /// the box says so when something follows.
  Future<void> _edit(int index) async {
    final transcript = widget.transcript;
    final after = transcript.messagesAfter(index);
    final controller = TextEditingController(text: transcript.entries[index].text);
    final l10n = AppLocalizations.of(context);
    final text = await showDialog<String>(
      context: context,
      builder: (dialogContext) => AlertDialog(
        title: Text(l10n.chatEditTitle),
        content: Column(
          mainAxisSize: MainAxisSize.min,
          crossAxisAlignment: CrossAxisAlignment.start,
          children: [
            TextField(controller: controller, autofocus: true, minLines: 1, maxLines: 8),
            if (after > 0) ...[
              const SizedBox(height: 12),
              Text(l10n.chatEditWarning(after), style: Theme.of(dialogContext).textTheme.bodySmall),
            ],
          ],
        ),
        actions: [
          TextButton(onPressed: () => Navigator.of(dialogContext).pop(), child: Text(l10n.commonCancel)),
          TextButton(onPressed: () => Navigator.of(dialogContext).pop(controller.text), child: Text(l10n.chatSend)),
        ],
      ),
    );
    controller.dispose();
    if (text == null || !mounted) return;
    if (transcript.resend(index, text)) _scrollToBottom();
  }

  void _scrollToBottom() {
    WidgetsBinding.instance.addPostFrameCallback((_) {
      if (!_scrollController.hasClients) return;
      _scrollController.animateTo(
        _scrollController.position.maxScrollExtent,
        duration: const Duration(milliseconds: 200),
        curve: Curves.easeOut,
      );
    });
  }

  /// P121 — the agents as contacts: one conversation each, its channel. Picking one opens it in the chat.
  void _openChannels() {
    unawaited(widget.transcript.refreshAgents());
    final l10n = AppLocalizations.of(context);
    showModalBottomSheet<void>(
      context: context,
      showDragHandle: true,
      builder: (sheet) => _ChannelsSheet(
        transcript: widget.transcript,
        onPick: (agent) async {
          final messenger = ScaffoldMessenger.of(context);
          Navigator.of(sheet).pop();
          try {
            await widget.transcript.openAgentChannel(agent);
          } catch (e) {
            messenger.showSnackBar(SnackBar(content: Text(l10n.chatChannelFailed('$e'))));
          }
        },
      ),
    );
  }

  /// P121 — what [agent] did outside its channel: the notes it traded with other agents and the runs nobody watched. Picking one opens
  /// it as the conversation it is, with a way back to this list.
  void _openWork(String agent) {
    showModalBottomSheet<void>(
      context: context,
      showDragHandle: true,
      isScrollControlled: true,
      builder: (sheet) => _WorkSheet(
        agent: agent,
        transcript: widget.transcript,
        onPick: (id) {
          Navigator.of(sheet).pop();
          setState(() => _workOpen = (agent: agent, id: id));
          widget.transcript.open(id);
        },
      ),
    );
  }

  /// P120, P123 — the agents screen. "Chat" on a node leaves it and starts an empty conversation as that agent.
  void _openAgents() {
    final l10n = AppLocalizations.of(context);
    Navigator.of(context).push(
      MaterialPageRoute(
        builder: (_) => AgentsScreen(
          backend: widget.connection,
          // A member sees the owner's tree as the access they were given allows (P120); the owner's tab is the owner's.
          memberOrg: widget.connection.user == null ? null : widget.connection,
          memberOrgAccess: orgAccessOf(widget.connection.user?.orgAccess),
          onOpenChat: (agentId) {
            widget.transcript.startNew();
            widget.transcript.selectAgent(agentId);
          },
          onOpenConversation: widget.transcript.open,
          onOpenChannel: (agent) async {
            final messenger = ScaffoldMessenger.of(context);
            try {
              await widget.transcript.openAgentChannel(agent);
            } catch (e) {
              messenger.showSnackBar(SnackBar(content: Text(l10n.chatChannelFailed('$e'))));
            }
          },
        ),
      ),
    );
  }

  /// P99 — the usage screen: the owner's report is the whole hub's, a member's holds only theirs.
  void _openUsage() {
    Navigator.of(context).push(MaterialPageRoute(builder: (_) => UsageScreen(load: widget.connection.usageSummary)));
  }

  /// P36 — the devices screen: invites for new devices, and changing the pairing key.
  void _openDevices() {
    Navigator.of(context).push(MaterialPageRoute(builder: (_) => DeviceInvitesScreen(backend: widget.connection)));
  }

  Future<void> _disconnect() async {
    await widget.connection.goodbye('user disconnected');
    if (mounted) Navigator.of(context).pop();
  }

  Future<void> _openFilesDialog() async {
    final fileTool = MobileFileTool();
    if (!mounted) return;
    final l10n = AppLocalizations.of(context);
    await showDialog<void>(
      context: context,
      builder: (dialogContext) => StatefulBuilder(
        builder: (dialogContext, setDialogState) {
          return AlertDialog(
            title: Text(l10n.chatFilesTitle),
            content: FutureBuilder<String?>(
              future: fileTool.rootFolderUri(),
              builder: (context, snapshot) {
                if (!snapshot.hasData) return const SizedBox(height: 24, child: Center(child: CircularProgressIndicator(strokeWidth: 2)));
                final uri = snapshot.data;
                return Column(
                  mainAxisSize: MainAxisSize.min,
                  crossAxisAlignment: CrossAxisAlignment.start,
                  children: [
                    Text(uri == null ? l10n.chatFilesNone : l10n.chatFilesConfigured(uri)),
                    const SizedBox(height: 8),
                    Text(l10n.chatFilesHint, style: const TextStyle(fontSize: 12)),
                  ],
                );
              },
            ),
            actions: [
              TextButton(
                onPressed: () async {
                  await fileTool.clearRootFolder();
                  setDialogState(() {});
                },
                child: Text(l10n.chatFilesClear),
              ),
              TextButton(
                onPressed: () async {
                  await fileTool.pickRootFolder();
                  setDialogState(() {});
                },
                child: Text(l10n.chatFilesChoose),
              ),
              TextButton(
                onPressed: () => Navigator.of(dialogContext).pop(),
                child: Text(l10n.commonDone),
              ),
            ],
          );
        },
      ),
    );
  }

  @override
  Widget build(BuildContext context) {
    final l10n = AppLocalizations.of(context);
    final serverName = switch (_status) {
      Connected(:final serverName) => serverName,
      _ => widget.connection.serverName,
    };

    return Scaffold(
      appBar: AppBar(
        // P78 — the open conversation's title, with the hub's name under it.
        title: ListenableBuilder(
          listenable: widget.transcript,
          builder: (context, _) => Column(
            crossAxisAlignment: CrossAxisAlignment.start,
            children: [
              Text(widget.transcript.channelAgent ?? widget.transcript.activeTitle ?? l10n.chatNewConversation, overflow: TextOverflow.ellipsis),
              Row(
                children: [
                  Flexible(
                    child: Text(
                      [serverName, if (widget.transcript.selectedAgentId != null) l10n.chatAs(widget.transcript.selectedAgentId!)].join(' · '),
                      style: Theme.of(context).textTheme.bodySmall,
                      overflow: TextOverflow.ellipsis,
                    ),
                  ),
                  if (_credit case final credit?)
                    InkWell(
                      key: const Key('chat-credit'),
                      onTap: _openUsage,
                      child: Text(
                        ' · ${credit.isOut ? l10n.chatCreditEmpty : l10n.chatCredit(formatUsd(credit.availableUsd))}',
                        style: Theme.of(context).textTheme.bodySmall?.copyWith(color: credit.isOut ? Theme.of(context).colorScheme.error : null),
                      ),
                    ),
                ],
              ),
            ],
          ),
        ),
        actions: [
          // P136 — the language of the app, also reachable once connected.
          const LanguageMenu(),
          // P137 — dark or light.
          const ThemeButton(),
          // P87 — which configured agent this conversation speaks as.
          ListenableBuilder(
            listenable: widget.transcript,
            // In an agent's channel (P121) the agent is the channel's: nothing to pick.
            builder: (context, _) => widget.transcript.channelAgent != null ? const SizedBox.shrink() : _AgentMenu(transcript: widget.transcript),
          ),
          // P121 — the agents as contacts: one conversation each.
          ListenableBuilder(
            listenable: widget.transcript,
            builder: (context, _) {
              final unread = widget.transcript.unreadChannelIds.length;
              return IconButton(
                key: const Key('channels-button'),
                onPressed: _openChannels,
                icon: Badge(isLabelVisible: unread > 0, label: Text('$unread'), child: const Icon(Icons.chat_outlined)),
                tooltip: unread > 0 ? l10n.chatChannelsTooltipNew(unread) : l10n.chatChannelsTooltip,
              );
            },
          ),
          // P120, P123 — the organization of the agents and the work they hand each other.
          IconButton(
            key: const Key('agents-button'),
            onPressed: _openAgents,
            icon: const Icon(Icons.account_tree_outlined),
            tooltip: l10n.chatAgentsTooltip,
          ),
          // P99 — what the person spent, and the limits that apply to them.
          IconButton(
            key: const Key('usage-button'),
            onPressed: _openUsage,
            icon: const Icon(Icons.bar_chart_outlined),
            tooltip: l10n.chatUsageTooltip,
          ),
          // P36 — invites for new devices and the pairing key: the owner's, since a member has no key to ask for.
          if (widget.connection.user == null)
            IconButton(
              key: const Key('devices-button'),
              onPressed: _openDevices,
              icon: const Icon(Icons.devices_outlined),
              tooltip: l10n.chatDevicesTooltip,
            ),
          Builder(
            builder: (context) => IconButton(
              onPressed: () => Scaffold.of(context).openEndDrawer(),
              icon: const Icon(Icons.forum_outlined),
              tooltip: l10n.chatConversationsTooltip,
            ),
          ),
          IconButton(
            onPressed: _openFilesDialog,
            icon: const Icon(Icons.folder_outlined),
            tooltip: l10n.chatFilesTooltip,
          ),
          TextButton(
            onPressed: _disconnect,
            child: Text(l10n.chatDisconnect),
          ),
        ],
      ),
      endDrawer: _ConversationDrawer(transcript: widget.transcript, connected: _status is Connected),
      body: Column(
        children: [
          if (_status is! Connected) _DisconnectedBanner(status: _status),
          // P125 — in a thread, the way back to the conversation it came from.
          ListenableBuilder(
            listenable: widget.transcript,
            builder: (context, _) => widget.transcript.threadParent == null
                ? const SizedBox.shrink()
                : Align(
                    alignment: Alignment.centerLeft,
                    child: TextButton.icon(
                      key: const Key('thread-back'),
                      onPressed: widget.transcript.closeThread,
                      icon: const Icon(Icons.arrow_back),
                      label: Text(l10n.chatThreadBack),
                    ),
                  ),
          ),
          // P121 — in an agent's channel, the way back to the loose conversations.
          ListenableBuilder(
            listenable: widget.transcript,
            builder: (context, _) {
              final agent = widget.transcript.channelAgent;
              if (agent == null) return const SizedBox.shrink();
              return Row(
                children: [
                  TextButton.icon(
                    key: const Key('channel-back'),
                    onPressed: widget.transcript.leaveChannel,
                    icon: const Icon(Icons.arrow_back),
                    label: Text(l10n.chatChannelBack),
                  ),
                  const Spacer(),
                  TextButton(
                    key: const Key('channel-work'),
                    onPressed: () => _openWork(agent),
                    child: Text(sideWorkButtonLabel(sideWork(widget.transcript.conversations, agent).length, l10n)),
                  ),
                ],
              );
            },
          ),
          // P121 — a note or run opened from an agent's list: the way back to the list.
          ListenableBuilder(
            listenable: widget.transcript,
            builder: (context, _) {
              final open = _workOpen;
              if (open == null || open.id != widget.transcript.activeConversationId) return const SizedBox.shrink();
              return Align(
                alignment: Alignment.centerLeft,
                child: TextButton.icon(
                  key: const Key('work-back'),
                  onPressed: () {
                    setState(() => _workOpen = null);
                    _openWork(open.agent);
                  },
                  icon: const Icon(Icons.arrow_back),
                  label: Text(l10n.chatWorkBack),
                ),
              );
            },
          ),
          // P102 — the folder this conversation works in, picked before its first message. A channel has none.
          ListenableBuilder(
            listenable: widget.transcript,
            builder: (context, _) =>
                widget.transcript.channelAgent != null ? const SizedBox.shrink() : _FolderBar(transcript: widget.transcript, connected: _status is Connected),
          ),
          Expanded(
            child: ListenableBuilder(
              listenable: widget.transcript,
              builder: (context, _) {
                final entries = widget.transcript.entries;
                final waiting = widget.transcript.waitingForReply;
                // P125 — no thread inside a thread; a message gets the button once the hub has given it an id.
                final inThread = widget.transcript.threadParent != null;
                final threads = widget.transcript.threads;
                return ListView.builder(
                  controller: _scrollController,
                  padding: const EdgeInsets.all(12),
                  itemCount: entries.length + (waiting ? 1 : 0),
                  itemBuilder: (context, index) {
                    if (index == entries.length) {
                      final live = widget.transcript.liveTurn;
                      return live == null
                          ? const _ThinkingIndicator()
                          : _LiveBubble(live: live, onStop: widget.transcript.canStop ? widget.transcript.stop : null);
                    }
                    final entry = entries[index];
                    final id = entry.id;
                    return _MessageBubble(
                      entry: entry,
                      thread: inThread || id == null || entry.role == EntryRole.error
                          ? null
                          : _ThreadLink(replies: threads[id]?.replies ?? 0, onTap: () => widget.transcript.openThread(id)),
                      // P157 — edit a message of the person's, or generate the last answer again.
                      actions: [
                        // P168 — the hosted credit ran out: the balance screen is one tap away.
                        if (entry.topUp) _BubbleAction(id: 'top-up', label: l10n.chatTopUp, onTap: _openUsage),
                        if (_status is Connected && widget.transcript.canEdit(index)) _BubbleAction(id: 'edit', label: l10n.commonEdit, onTap: () => _edit(index)),
                        if (_status is Connected && widget.transcript.canRegenerate(index))
                          _BubbleAction(id: 'regenerate', label: l10n.chatRegenerate, onTap: widget.transcript.regenerate),
                      ],
                    );
                  },
                );
              },
            ),
          ),
          ListenableBuilder(
            listenable: widget.transcript,
            builder: (context, _) => _InputBar(
              controller: _inputController,
              enabled: !widget.transcript.waitingForReply && _status is Connected,
              onSend: _send,
              onStop: widget.transcript.canStop ? widget.transcript.stop : null,
            ),
          ),
        ],
      ),
    );
  }
}

/// P102 — the working folder of the open conversation: a button to pick one while the conversation is new (and the
/// hub is reachable), a plain label once it has begun. Hidden for an old conversation that has no folder.
class _FolderBar extends StatelessWidget {
  const _FolderBar({required this.transcript, required this.connected});

  final ChatTranscript transcript;
  final bool connected;

  Future<void> _pick(BuildContext context) async {
    final nodes = await transcript.loadNodes();
    if (!context.mounted) return;
    final picked = await showFolderPicker(context, listDirs: transcript.listDirs, initialPath: transcript.workdir, nodes: nodes);
    if (picked != null) transcript.selectWorkdir(picked);
  }

  @override
  Widget build(BuildContext context) {
    final folder = transcript.workdir;
    final l10n = AppLocalizations.of(context);
    if (transcript.canPickFolder) {
      return Align(
        alignment: Alignment.centerLeft,
        child: Padding(
          padding: const EdgeInsets.symmetric(horizontal: 12),
          child: Row(
            mainAxisSize: MainAxisSize.min,
            children: [
              ActionChip(
                key: const Key('folder-button'),
                avatar: const Icon(Icons.folder_special_outlined, size: 18),
                label: Text(folder == null ? l10n.folderBarNone : folderLabel(folder, transcript.nodes)),
                tooltip: folder == null ? l10n.folderBarTooltip : folderPlace(folder, transcript.nodes),
                onPressed: connected ? () => _pick(context) : null,
              ),
              if (folder != null)
                IconButton(
                  key: const Key('folder-clear'),
                  onPressed: () => transcript.selectWorkdir(null),
                  icon: const Icon(Icons.close, size: 18),
                  tooltip: l10n.folderBarClear,
                ),
            ],
          ),
        ),
      );
    }
    if (folder == null) return const SizedBox.shrink();
    return Align(
      alignment: Alignment.centerLeft,
      child: Padding(
        padding: const EdgeInsets.symmetric(horizontal: 12),
        child: Tooltip(
          message: folderPlace(folder, transcript.nodes),
          child: Chip(
            key: const Key('folder-chip'),
            avatar: const Icon(Icons.folder_special_outlined, size: 18),
            label: Text(folderLabel(folder, transcript.nodes)),
          ),
        ),
      ),
    );
  }
}

/// P87 — picks the agent the next turns speak as, like the web's selector. Hidden while the hub has
/// no agents configured (and none is selected); locked while the conversation waits on a reply.
/// P121 — the agents as contacts: newest conversation first, the ones never spoken to at the end.
class _ChannelsSheet extends StatelessWidget {
  const _ChannelsSheet({required this.transcript, required this.onPick});

  final ChatTranscript transcript;
  final void Function(String agent) onPick;

  static String _when(int millis) {
    final date = DateTime.fromMillisecondsSinceEpoch(millis);
    final now = DateTime.now();
    final sameDay = date.year == now.year && date.month == now.month && date.day == now.day;
    String two(int n) => n.toString().padLeft(2, '0');
    return sameDay ? '${two(date.hour)}:${two(date.minute)}' : '${two(date.day)}/${two(date.month)}';
  }

  @override
  Widget build(BuildContext context) {
    final l10n = AppLocalizations.of(context);
    return SafeArea(
      child: ListenableBuilder(
        listenable: transcript,
        builder: (context, _) {
          final rows = [
            for (final agent in transcript.agentIds)
              (
                agent: agent,
                channel: transcript.conversations.where((c) => c.id == transcript.channels[agent]).firstOrNull,
                answering: transcript.channels[agent] != null && transcript.isAnswering(transcript.channels[agent]!),
              ),
          ]..sort((a, b) => (b.channel?.updatedAt ?? 0).compareTo(a.channel?.updatedAt ?? 0));
          if (rows.isEmpty) {
            return Padding(padding: const EdgeInsets.all(24), child: Text(l10n.channelsNone, textAlign: TextAlign.center));
          }
          return ListView(
            shrinkWrap: true,
            children: [
              for (final row in rows)
                ListTile(
                  key: Key('channel-${row.agent}'),
                  selected: transcript.channelAgent == row.agent,
                  leading: Badge(
                    key: Key('channel-unread-${row.agent}'),
                    isLabelVisible: transcript.unreadAgents.contains(row.agent),
                    smallSize: 9,
                    child: const Icon(Icons.person_outline),
                  ),
                  title: Text(row.agent),
                  trailing: Text(row.answering ? l10n.channelsAnswering : (row.channel == null ? l10n.channelsNew : _when(row.channel!.updatedAt))),
                  onTap: () => onPick(row.agent),
                ),
            ],
          );
        },
      ),
    );
  }
}

/// P121 — the notes and runs of an agent outside its channel, newest first.
class _WorkSheet extends StatelessWidget {
  const _WorkSheet({required this.agent, required this.transcript, required this.onPick});

  final String agent;
  final ChatTranscript transcript;
  final void Function(String conversationId) onPick;

  @override
  Widget build(BuildContext context) {
    final l10n = AppLocalizations.of(context);
    return SafeArea(
      child: ListenableBuilder(
        listenable: transcript,
        builder: (context, _) {
          final items = sideWork(transcript.conversations, agent);
          if (items.isEmpty) {
            return Padding(
              padding: const EdgeInsets.all(24),
              child: Text(l10n.workNone(agent), textAlign: TextAlign.center),
            );
          }
          return ListView(
            shrinkWrap: true,
            children: [
              for (final item in items)
                ListTile(
                  key: Key('work-${item.id}'),
                  title: Text(item.title, overflow: TextOverflow.ellipsis),
                  subtitle: Text(sideWorkLabel(item, l10n)),
                  trailing: Text(transcript.isAnswering(item.id) ? l10n.channelsAnswering : _ChannelsSheet._when(item.updatedAt)),
                  onTap: () => onPick(item.id),
                ),
            ],
          );
        },
      ),
    );
  }
}

class _AgentMenu extends StatelessWidget {
  const _AgentMenu({required this.transcript});

  final ChatTranscript transcript;

  /// `PopupMenuButton` can't return null as a choice, so "no agent" is this.
  static const _none = '';

  @override
  Widget build(BuildContext context) {
    final selected = transcript.selectedAgentId;
    final ids = transcript.agentIds;
    if (ids.isEmpty && selected == null) return const SizedBox.shrink();
    final l10n = AppLocalizations.of(context);
    return PopupMenuButton<String>(
      enabled: !transcript.waitingForReply,
      tooltip: l10n.agentMenuTooltip,
      icon: Icon(selected == null ? Icons.person_outline : Icons.person),
      initialValue: selected ?? _none,
      onOpened: () => unawaited(transcript.refreshAgents()),
      onSelected: (id) => transcript.selectAgent(id == _none ? null : id),
      itemBuilder: (context) => [
        PopupMenuItem(value: _none, child: Text(l10n.agentMenuNone)),
        for (final id in ids) PopupMenuItem(value: id, child: Text(id)),
        if (selected != null && !ids.contains(selected)) PopupMenuItem(value: selected, child: Text(l10n.agentMenuRemoved(selected))),
      ],
    );
  }
}

/// P78 — this device's conversations on the hub: open one, start a new one, rename or delete.
class _ConversationDrawer extends StatelessWidget {
  const _ConversationDrawer({required this.transcript, required this.connected});

  final ChatTranscript transcript;

  /// Rename/delete need the hub; browsing and starting a new (still local) one don't.
  final bool connected;

  Future<void> _rename(BuildContext context, ConversationSummary conversation) async {
    final controller = TextEditingController(text: conversation.title);
    final l10n = AppLocalizations.of(context);
    final title = await showDialog<String>(
      context: context,
      builder: (dialogContext) => AlertDialog(
        title: Text(l10n.drawerRenameTitle),
        content: TextField(
          controller: controller,
          autofocus: true,
          maxLength: 120,
          onSubmitted: (value) => Navigator.of(dialogContext).pop(value),
        ),
        actions: [
          TextButton(onPressed: () => Navigator.of(dialogContext).pop(), child: Text(l10n.commonCancel)),
          TextButton(onPressed: () => Navigator.of(dialogContext).pop(controller.text), child: Text(l10n.commonSave)),
        ],
      ),
    );
    controller.dispose();
    if (title == null || title.trim().isEmpty || title.trim() == conversation.title || !context.mounted) return;
    await _run(context, () => transcript.rename(conversation.id, title.trim()));
  }

  Future<void> _delete(BuildContext context, ConversationSummary conversation) async {
    final l10n = AppLocalizations.of(context);
    final confirmed = await showDialog<bool>(
      context: context,
      builder: (dialogContext) => AlertDialog(
        title: Text(l10n.drawerDeleteTitle),
        content: Text(l10n.drawerDeleteBody(conversation.title)),
        actions: [
          TextButton(onPressed: () => Navigator.of(dialogContext).pop(false), child: Text(l10n.commonCancel)),
          TextButton(onPressed: () => Navigator.of(dialogContext).pop(true), child: Text(l10n.commonDelete)),
        ],
      ),
    );
    if (confirmed != true || !context.mounted) return;
    await _run(context, () => transcript.delete(conversation.id));
  }

  Future<void> _run(BuildContext context, Future<void> Function() action) async {
    final messenger = ScaffoldMessenger.of(context);
    try {
      await action();
    } catch (e) {
      messenger.showSnackBar(SnackBar(content: Text(e.toString())));
    }
  }

  Widget _tile(BuildContext context, ConversationSummary conversation) {
    final answering = transcript.isAnswering(conversation.id);
    final l10n = AppLocalizations.of(context);
    return ListTile(
      selected: conversation.id == transcript.activeConversationId,
      title: Text(conversation.title.isEmpty ? l10n.drawerUntitled : conversation.title, maxLines: 1, overflow: TextOverflow.ellipsis),
      subtitle: answering ? Text(l10n.drawerAnswering) : null,
      onTap: () {
        transcript.open(conversation.id);
        Navigator.of(context).pop();
      },
      trailing: PopupMenuButton<String>(
        // Nothing on the hub to change yet, or its answer is still on the way.
        enabled: connected && !answering,
        tooltip: l10n.drawerActions,
        onSelected: (action) => switch (action) {
          'rename' => _rename(context, conversation),
          'pin' => _run(context, () => transcript.setFlags(conversation.id, pinned: !conversation.pinned)),
          'archive' => _run(context, () => transcript.setFlags(conversation.id, archived: !conversation.archived)),
          _ => _delete(context, conversation),
        },
        itemBuilder: (_) => [
          if (!conversation.archived) PopupMenuItem(value: 'pin', child: Text(conversation.pinned ? l10n.drawerUnpin : l10n.drawerPin)),
          PopupMenuItem(value: 'archive', child: Text(conversation.archived ? l10n.drawerUnarchive : l10n.drawerArchive)),
          PopupMenuItem(value: 'rename', child: Text(l10n.drawerRename)),
          PopupMenuItem(value: 'delete', child: Text(l10n.commonDelete)),
        ],
      ),
    );
  }

  @override
  Widget build(BuildContext context) {
    final l10n = AppLocalizations.of(context);
    return Drawer(
      child: SafeArea(
        child: ListenableBuilder(
          listenable: transcript,
          builder: (context, _) {
            final conversations = transcript.visibleConversations;
            // P157 — pinned first, then by how recently they were used, with the archived ones folded away at the end.
            final groups = groupConversations(conversations, DateTime.now());
            final error = transcript.conversationsError;
            return Column(
              crossAxisAlignment: CrossAxisAlignment.stretch,
              children: [
                Padding(
                  padding: const EdgeInsets.all(12),
                  child: FilledButton.icon(
                    onPressed: () {
                      transcript.startNew();
                      Navigator.of(context).pop();
                    },
                    icon: const Icon(Icons.add),
                    label: Text(l10n.drawerNew),
                  ),
                ),
                if (error != null)
                  Padding(
                    padding: const EdgeInsets.symmetric(horizontal: 16, vertical: 4),
                    child: Text(error, style: TextStyle(color: Theme.of(context).colorScheme.error)),
                  ),
                if (conversations.isEmpty && error == null)
                  Padding(
                    padding: const EdgeInsets.all(16),
                    child: Text(l10n.drawerNone, textAlign: TextAlign.center),
                  ),
                Expanded(
                  child: ListView(
                    children: [
                      if (groups.pinned.isNotEmpty) ...[
                        _GroupHeader(l10n.drawerPinned),
                        for (final c in groups.pinned) _tile(context, c),
                      ],
                      for (final bucket in groups.buckets) ...[
                        _GroupHeader(bucketTitle(bucket.label, l10n)),
                        for (final c in bucket.items) _tile(context, c),
                      ],
                      if (groups.archived.isNotEmpty)
                        ExpansionTile(
                          title: Text(l10n.drawerArchived(groups.archived.length)),
                          children: [for (final c in groups.archived) _tile(context, c)],
                        ),
                    ],
                  ),
                ),
              ],
            );
          },
        ),
      ),
    );
  }
}

/// P157 — the heading of a block of the drawer's list: "Pinned", "Today", "Yesterday", …
class _GroupHeader extends StatelessWidget {
  const _GroupHeader(this.label);

  final String label;

  @override
  Widget build(BuildContext context) {
    final theme = Theme.of(context);
    return Padding(
      padding: const EdgeInsets.fromLTRB(16, 12, 16, 4),
      child: Text(label, style: theme.textTheme.labelMedium?.copyWith(color: theme.colorScheme.onSurfaceVariant)),
    );
  }
}

class _DisconnectedBanner extends StatelessWidget {
  const _DisconnectedBanner({required this.status});

  final ConnectionStatus status;

  @override
  Widget build(BuildContext context) {
    final l10n = AppLocalizations.of(context);
    final label = switch (status) {
      Disconnected(:final reason) => reason == null ? l10n.bannerDisconnected : l10n.bannerDisconnectedWhy(reason),
      Connecting() => l10n.bannerConnecting,
      ConnectionFailure(:final message) => l10n.bannerError(message),
      Connected() => '',
    };
    return Container(
      width: double.infinity,
      color: Theme.of(context).colorScheme.errorContainer,
      padding: const EdgeInsets.symmetric(horizontal: 16, vertical: 8),
      child: Text(label, style: TextStyle(color: Theme.of(context).colorScheme.onErrorContainer)),
    );
  }
}

/// P124 — what the engine is doing in the turn that is going on: its words and tools in the order they happened.
class _LiveBubble extends StatelessWidget {
  const _LiveBubble({required this.live, this.onStop});

  final LiveTurn live;

  /// Stops the task; null for a conversation that has none to stop.
  final VoidCallback? onStop;

  static String _mark(ToolStatus status) => switch (status) {
        ToolStatus.running => '…',
        ToolStatus.completed => '✓',
        ToolStatus.failed => '✗',
      };

  @override
  Widget build(BuildContext context) {
    final scheme = Theme.of(context).colorScheme;
    final muted = TextStyle(fontSize: 13, color: scheme.onSurfaceVariant);
    return Align(
      alignment: Alignment.centerLeft,
      child: Container(
        margin: const EdgeInsets.symmetric(vertical: 4),
        padding: const EdgeInsets.symmetric(horizontal: 12, vertical: 8),
        constraints: BoxConstraints(maxWidth: MediaQuery.of(context).size.width * 0.8),
        decoration: BoxDecoration(color: scheme.surfaceContainerHighest, borderRadius: BorderRadius.circular(12)),
        child: Column(
          crossAxisAlignment: CrossAxisAlignment.start,
          mainAxisSize: MainAxisSize.min,
          children: [
            for (final item in live.items)
              switch (item) {
                // Words of a manager's task say which one, since several can be writing at once.
                LiveText(:final text, :final source) => Padding(
                    padding: const EdgeInsets.only(bottom: 4),
                    child: Column(
                      crossAxisAlignment: CrossAxisAlignment.start,
                      children: [
                        if (source != null) Text(source, style: muted.copyWith(fontSize: 12, fontWeight: FontWeight.w600)),
                        Text(text, style: TextStyle(color: scheme.onSurface)),
                      ],
                    ),
                  ),
                LiveTool(:final tool, :final title, :final status) => Padding(
                    padding: const EdgeInsets.only(bottom: 4),
                    child: Text.rich(
                      TextSpan(style: muted, children: [
                        TextSpan(text: '${_mark(status)} '),
                        TextSpan(text: tool, style: TextStyle(fontWeight: FontWeight.w600, color: scheme.onSurface)),
                        TextSpan(text: '  $title', style: const TextStyle(fontFamily: 'monospace')),
                      ]),
                    ),
                  ),
              },
            if (live.notice != null) Text(live.notice!, style: muted.copyWith(fontStyle: FontStyle.italic)),
            if (onStop != null) TextButton(onPressed: onStop, child: Text(AppLocalizations.of(context).liveStop)),
          ],
        ),
      ),
    );
  }
}

class _ThinkingIndicator extends StatelessWidget {
  const _ThinkingIndicator();

  @override
  Widget build(BuildContext context) {
    return Padding(
      padding: const EdgeInsets.symmetric(vertical: 8, horizontal: 4),
      child: Row(
        children: [
          const SizedBox(width: 16, height: 16, child: CircularProgressIndicator(strokeWidth: 2)),
          const SizedBox(width: 8),
          Text(AppLocalizations.of(context).thinking),
        ],
      ),
    );
  }
}

/// P125 — what a message offers under it: start a thread, or open the one it has (with its reply count).
class _ThreadLink extends StatelessWidget {
  const _ThreadLink({required this.replies, required this.onTap});

  final int replies;
  final VoidCallback onTap;

  @override
  Widget build(BuildContext context) {
    return TextButton(
      key: const Key('thread-link'),
      style: TextButton.styleFrom(visualDensity: VisualDensity.compact, padding: EdgeInsets.zero, minimumSize: const Size(0, 28)),
      onPressed: onTap,
      child: Text(replies == 0 ? AppLocalizations.of(context).threadReply : AppLocalizations.of(context).threadReplies(replies)),
    );
  }
}

/// P157 — a small button under a message: "Edit" on the person's, "Regenerate" on the last answer.
class _BubbleAction extends StatelessWidget {
  const _BubbleAction({required this.id, required this.label, required this.onTap});

  /// What the action is, whatever the language the label is in (it makes the key).
  final String id;
  final String label;
  final VoidCallback onTap;

  @override
  Widget build(BuildContext context) {
    return TextButton(
      key: Key('bubble-action-$id'),
      style: TextButton.styleFrom(visualDensity: VisualDensity.compact, padding: EdgeInsets.zero, minimumSize: const Size(0, 28)),
      onPressed: onTap,
      child: Text(label),
    );
  }
}

class _MessageBubble extends StatelessWidget {
  const _MessageBubble({required this.entry, this.thread, this.actions = const []});

  final ChatEntry entry;
  final _ThreadLink? thread;
  final List<_BubbleAction> actions;

  @override
  Widget build(BuildContext context) {
    final scheme = Theme.of(context).colorScheme;
    final (alignment, background, foreground) = switch (entry.role) {
      EntryRole.user => (Alignment.centerRight, scheme.primaryContainer, scheme.onPrimaryContainer),
      EntryRole.assistant => (Alignment.centerLeft, scheme.surfaceContainerHighest, scheme.onSurface),
      EntryRole.error => (Alignment.centerLeft, scheme.errorContainer, scheme.onErrorContainer),
    };

    return Align(
      alignment: alignment,
      child: Container(
        margin: const EdgeInsets.symmetric(vertical: 4),
        padding: const EdgeInsets.symmetric(horizontal: 12, vertical: 8),
        constraints: BoxConstraints(maxWidth: MediaQuery.of(context).size.width * 0.8),
        decoration: BoxDecoration(color: background, borderRadius: BorderRadius.circular(12)),
        child: Column(
          crossAxisAlignment: CrossAxisAlignment.start,
          mainAxisSize: MainAxisSize.min,
          children: [
            Text(entry.text, style: TextStyle(color: foreground)),
            for (final attachment in entry.attachments) _AttachmentPreview(attachment: attachment, foreground: foreground),
            if (thread != null || actions.isNotEmpty) Wrap(spacing: 12, children: [?thread, ...actions]),
          ],
        ),
      ),
    );
  }
}

/// Media extracted from an MCP tool result (P64 frente 2). `image/*` renders via `Image.memory`
/// (slice 3); `audio/*`/`video/*` play for real too now (P66) — audio straight from bytes
/// (`audioplayers`'s `BytesSource`), video via a temp file (`video_player` has no bytes source).
/// Anything else still falls back to a caption instead of vanishing silently, same
/// graceful-degradation spirit as `MEDIA_REPLY` on the WhatsApp side for *received* media.
class _AttachmentPreview extends StatelessWidget {
  const _AttachmentPreview({required this.attachment, required this.foreground});

  final Attachment attachment;
  final Color foreground;

  @override
  Widget build(BuildContext context) {
    return switch (attachmentKindFor(attachment.mimeType)) {
      AttachmentKind.image => _ImageAttachment(attachment: attachment, foreground: foreground),
      AttachmentKind.audio => _AudioAttachmentPlayer(attachment: attachment, foreground: foreground),
      AttachmentKind.video => _VideoAttachmentPlayer(attachment: attachment, foreground: foreground),
      AttachmentKind.unsupported => _UnsupportedAttachment(attachment: attachment, foreground: foreground),
    };
  }
}

class _UnsupportedAttachment extends StatelessWidget {
  const _UnsupportedAttachment({required this.attachment, required this.foreground});

  final Attachment attachment;
  final Color foreground;

  @override
  Widget build(BuildContext context) {
    return Padding(
      padding: const EdgeInsets.only(top: 6),
      child: Text(
        AppLocalizations.of(context).attachmentUnsupported(attachment.mimeType),
        style: TextStyle(color: foreground, fontSize: 12, fontStyle: FontStyle.italic),
      ),
    );
  }
}

class _AttachmentError extends StatelessWidget {
  const _AttachmentError({required this.attachment, required this.foreground, required this.verb});

  final Attachment attachment;
  final Color foreground;

  /// What failed: `decode` or `play`.
  final String verb;

  @override
  Widget build(BuildContext context) {
    final l10n = AppLocalizations.of(context);
    return Text(
      verb == 'decode' ? l10n.attachmentDecodeFailed(attachment.mimeType) : l10n.attachmentPlayFailed(attachment.mimeType),
      style: TextStyle(color: foreground, fontSize: 12, fontStyle: FontStyle.italic),
    );
  }
}

class _ImageAttachment extends StatelessWidget {
  const _ImageAttachment({required this.attachment, required this.foreground});

  final Attachment attachment;
  final Color foreground;

  @override
  Widget build(BuildContext context) {
    return Padding(
      padding: const EdgeInsets.only(top: 6),
      child: ClipRRect(
        borderRadius: BorderRadius.circular(8),
        child: Image.memory(
          base64Decode(attachment.data),
          errorBuilder: (context, error, stackTrace) =>
              _AttachmentError(attachment: attachment, foreground: foreground, verb: 'decode'),
        ),
      ),
    );
  }
}

/// Plays an `audio/*` attachment straight from the decoded bytes — `BytesSource` needs no temp
/// file, same zero-I/O spirit as `Image.memory` above. Manual play/pause, no autoplay (same
/// deliberate posture as the desktop's TTS `SpeakButton`, P28).
class _AudioAttachmentPlayer extends StatefulWidget {
  const _AudioAttachmentPlayer({required this.attachment, required this.foreground});

  final Attachment attachment;
  final Color foreground;

  @override
  State<_AudioAttachmentPlayer> createState() => _AudioAttachmentPlayerState();
}

class _AudioAttachmentPlayerState extends State<_AudioAttachmentPlayer> {
  final _player = AudioPlayer();
  PlayerState _state = PlayerState.stopped;
  String? _error;
  StreamSubscription<PlayerState>? _stateSubscription;

  @override
  void initState() {
    super.initState();
    _stateSubscription = _player.onPlayerStateChanged.listen((state) {
      if (mounted) setState(() => _state = state);
    });
  }

  @override
  void dispose() {
    _stateSubscription?.cancel();
    _player.dispose();
    super.dispose();
  }

  Future<void> _toggle() async {
    if (_state == PlayerState.playing) {
      await _player.pause();
      return;
    }
    try {
      final bytes = base64Decode(widget.attachment.data);
      await _player.play(BytesSource(bytes));
    } catch (_) {
      if (mounted) setState(() => _error = 'play');
    }
  }

  @override
  Widget build(BuildContext context) {
    if (_error != null) {
      return _AttachmentError(attachment: widget.attachment, foreground: widget.foreground, verb: _error!);
    }
    return Padding(
      padding: const EdgeInsets.only(top: 6),
      child: Row(
        mainAxisSize: MainAxisSize.min,
        children: [
          IconButton(
            icon: Icon(_state == PlayerState.playing ? Icons.pause_circle_filled : Icons.play_circle_filled,
                color: widget.foreground),
            onPressed: _toggle,
          ),
          Text(widget.attachment.mimeType, style: TextStyle(color: widget.foreground, fontSize: 12)),
        ],
      ),
    );
  }
}

/// Plays a `video/*` attachment — `video_player` has no bytes source, so the decoded bytes are
/// written to a temp file once and cleaned up on dispose.
class _VideoAttachmentPlayer extends StatefulWidget {
  const _VideoAttachmentPlayer({required this.attachment, required this.foreground});

  final Attachment attachment;
  final Color foreground;

  @override
  State<_VideoAttachmentPlayer> createState() => _VideoAttachmentPlayerState();
}

class _VideoAttachmentPlayerState extends State<_VideoAttachmentPlayer> {
  VideoPlayerController? _controller;
  File? _tempFile;
  late final Future<void> _initialization = _initialize();

  static const _extensionBySubtype = {'quicktime': 'mov', 'x-msvideo': 'avi'};

  Future<void> _initialize() async {
    final bytes = base64Decode(widget.attachment.data);
    final subtype = widget.attachment.mimeType.split('/').last;
    final extension = _extensionBySubtype[subtype] ?? subtype;
    final dir = await getTemporaryDirectory();
    final file = File('${dir.path}/warden_attachment_${DateTime.now().microsecondsSinceEpoch}.$extension');
    await file.writeAsBytes(bytes);
    _tempFile = file;

    final controller = VideoPlayerController.file(file);
    await controller.initialize();
    _controller = controller;
  }

  @override
  void dispose() {
    _controller?.dispose();
    _tempFile?.delete().catchError((_) => _tempFile!);
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    return Padding(
      padding: const EdgeInsets.only(top: 6),
      child: FutureBuilder<void>(
        future: _initialization,
        builder: (context, snapshot) {
          if (snapshot.hasError || (snapshot.connectionState == ConnectionState.done && _controller == null)) {
            return _AttachmentError(attachment: widget.attachment, foreground: widget.foreground, verb: 'play');
          }
          if (snapshot.connectionState != ConnectionState.done) {
            return SizedBox(
              height: 32,
              width: 32,
              child: CircularProgressIndicator(strokeWidth: 2, color: widget.foreground),
            );
          }
          final controller = _controller!;
          return ClipRRect(
            borderRadius: BorderRadius.circular(8),
            child: AspectRatio(
              aspectRatio: controller.value.aspectRatio,
              child: Stack(
                alignment: Alignment.center,
                children: [
                  VideoPlayer(controller),
                  GestureDetector(
                    onTap: () => setState(() {
                      controller.value.isPlaying ? controller.pause() : controller.play();
                    }),
                    child: AnimatedOpacity(
                      opacity: controller.value.isPlaying ? 0 : 1,
                      duration: const Duration(milliseconds: 200),
                      child: Icon(Icons.play_circle_filled, size: 48, color: widget.foreground),
                    ),
                  ),
                ],
              ),
            ),
          );
        },
      ),
    );
  }
}

class _InputBar extends StatelessWidget {
  const _InputBar({required this.controller, required this.enabled, required this.onSend, this.onStop});

  final TextEditingController controller;
  final bool enabled;
  final VoidCallback onSend;

  /// Given while a turn is being answered (P157): the send button becomes a Stop button that ends it where it is.
  final VoidCallback? onStop;

  @override
  Widget build(BuildContext context) {
    final l10n = AppLocalizations.of(context);
    return SafeArea(
      child: Padding(
        padding: const EdgeInsets.all(8),
        child: Row(
          children: [
            Expanded(
              child: TextField(
                controller: controller,
                enabled: enabled,
                minLines: 1,
                maxLines: 5,
                decoration: InputDecoration(
                  hintText: l10n.inputHint,
                  border: const OutlineInputBorder(),
                  contentPadding: const EdgeInsets.symmetric(horizontal: 12, vertical: 8),
                ),
                onSubmitted: (_) => enabled ? onSend() : null,
              ),
            ),
            const SizedBox(width: 8),
            if (onStop != null)
              IconButton.filled(
                onPressed: onStop,
                tooltip: l10n.liveStop,
                icon: const Icon(Icons.stop),
              )
            else
              IconButton.filled(
                onPressed: enabled ? onSend : null,
                icon: const Icon(Icons.send),
              ),
          ],
        ),
      ),
    );
  }
}
