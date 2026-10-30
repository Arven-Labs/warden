import 'dart:async';

import 'package:flutter/foundation.dart';
import 'package:flutter/material.dart';

import '../l10n/generated/app_localizations.dart';
import '../services/channel_unread.dart';
import '../services/chat_notifications.dart';
import '../services/chat_transcript.dart';
import '../services/connection_settings.dart';
import '../services/hub_pairing_qr.dart';
import '../services/mobile_file_tool.dart';
import '../services/server_connection.dart';
import '../src/rust/api/discovery.dart';
import 'chat_screen.dart';
import 'language_menu.dart';
import 'theme_button.dart';
import '../theme/warden_logo.dart';
import 'member_account.dart';
import 'qr_scan_screen.dart';
import 'skills_screen.dart';
import 'sync_screen.dart';

/// Phase 7.2 scope: prove connectivity to a warden-server over WebSocket.
/// A successful connect now pushes straight into the real chat UI (Phase 7.3, `ChatScreen`) —
/// this screen stays underneath so the user can navigate back to it without hanging up.
class ConnectionScreen extends StatefulWidget {
  const ConnectionScreen({super.key, this.connector = ServerConnection.connect});

  /// How a connection gets opened — `ServerConnection.connect` (a real WebSocket) in the app, a
  /// fake channel in tests, so the connect → chat → back → resume flow (P41) can be exercised
  /// without a network.
  final ServerConnector connector;

  @override
  State<ConnectionScreen> createState() => _ConnectionScreenState();
}

class _ConnectionScreenState extends State<ConnectionScreen> {
  final _hostController = TextEditingController();
  final _portController = TextEditingController(text: '${ConnectionSettingsStore.defaultPort}');
  final _authKeyController = TextEditingController();
  final _deviceNameController = TextEditingController();
  // P84 — a member signs in with the username and password the owner created for them.
  final _usernameController = TextEditingController();
  final _passwordController = TextEditingController();
  bool _useAccount = false;
  bool _useTls = false;

  final _settingsStore = ConnectionSettingsStore();
  final _fileTool = MobileFileTool();

  ServerConnection? _connection;
  ChatTranscript? _transcript;
  ConnectionStatus _status = const Disconnected();
  StreamSubscription<ConnectionStatus>? _statusSubscription;

  @override
  void initState() {
    super.initState();
    _deviceNameController.text = ConnectionSettingsStore.defaultDeviceName();
    _loadSavedSettings();
  }

  Future<void> _loadSavedSettings() async {
    final saved = await _settingsStore.load();
    if (saved != null) {
      setState(() {
        _hostController.text = saved.host;
        _portController.text = '${saved.port}';
        _authKeyController.text = saved.authKey;
        _deviceNameController.text = saved.deviceName;
        _useTls = saved.useTls;
      });
      return;
    }
    // No persisted settings yet: prefill a dev-friendly default so the app
    // is usable out of the box against a locally-run warden-server, without
    // ever silently overriding a value the user already saved. 10.0.2.2 is
    // the Android emulator's special alias for the host machine's loopback.
    final hint = _debugAndroidHostHint();
    if (hint != null) {
      setState(() => _hostController.text = hint);
    }
  }

  String? _debugAndroidHostHint() {
    if (kDebugMode && defaultTargetPlatform == TargetPlatform.android) {
      return '10.0.2.2';
    }
    return null;
  }

  Future<void> _connect() async {
    // Read before the first await: the failures below are shown in the language the person has now.
    final l10n = AppLocalizations.of(context);
    final host = _hostController.text.trim();
    final port = int.tryParse(_portController.text.trim());
    final useAccount = _useAccount;
    final authKey = useAccount ? '' : _authKeyController.text;
    final username = _usernameController.text.trim();
    final password = _passwordController.text;
    final deviceName = _deviceNameController.text.trim();
    final useTls = _useTls;
    final missing = useAccount ? l10n.connFillAccount : l10n.connFillKey;

    if (host.isEmpty || port == null || deviceName.isEmpty) {
      setState(() => _status = ConnectionFailure(missing));
      return;
    }
    // P36 — once paired, the device token is enough; the auth key (or a member's password, P84)
    // only matters for pairing.
    final deviceToken = await _settingsStore.deviceTokenFor(host, port);
    final hasCredentials = useAccount ? username.isNotEmpty && password.isNotEmpty : authKey.isNotEmpty;
    if (!hasCredentials && deviceToken == null) {
      setState(() => _status = ConnectionFailure(missing));
      return;
    }

    setState(() => _status = const Connecting());

    try {
      final deviceId = await _settingsStore.getOrCreateDeviceId();
      final hasRootFolder = await _fileTool.rootFolderUri() != null;
      final connection = await widget.connector(
        host: host,
        port: port,
        secure: useTls,
        deviceId: deviceId,
        deviceName: deviceName,
        authKey: authKey,
        deviceToken: deviceToken,
        username: useAccount && hasCredentials ? username : null,
        password: useAccount && hasCredentials ? password : null,
        // Opt-in, same spirit as the desktop's `enable_shell`: only advertise (and answer) the
        // file tools once the user has picked a root folder for them to operate in.
        toolSpecs: hasRootFolder ? const [MobileFileTool.listFilesSpec, MobileFileTool.readFileSpec] : const [],
        toolHandlers: hasRootFolder ? {'list_phone_files': _fileTool.listFiles, 'read_phone_file': _fileTool.readFile} : const {},
      );
      final issuedToken = connection.issuedDeviceToken;
      if (issuedToken != null) {
        await _settingsStore.saveDeviceToken(host, port, issuedToken);
      }
      await _settingsStore.save(ConnectionSettings(
        host: host,
        port: port,
        authKey: authKey,
        deviceName: deviceName,
        useTls: useTls,
      ));

      _passwordController.clear();

      // P84 — what a member has to get through before the chat: their own password, the recovery
      // code of their data, the workspace's recovery policy and any recovery the owner made.
      if (connection.user != null) {
        final proceed = await _memberGate(connection, l10n);
        if (!proceed) return;
      }

      await _statusSubscription?.cancel();
      _statusSubscription = connection.statusStream.listen((s) {
        if (mounted) setState(() => _status = s);
      });

      final lastConversation = await _settingsStore.lastConversationFor(host, port);
      _transcript?.dispose();
      setState(() {
        _connection = connection;
        _transcript = ChatTranscript(
          chatStream: connection.chatStream,
          backend: connection,
          // P78 — reopens the conversation that was open on this hub last time.
          lastConversationId: lastConversation,
          onConversationOpened: (id) => unawaited(_settingsStore.saveLastConversation(host, port, id)),
          // P121 — what was shown of each agent's channel is kept per hub, and an agent that writes there gets a notification.
          seenStore: PrefsChannelSeenStore(host, port),
          onAgentMessage: (agent, text) => unawaited(showAgentMessageNotification(agent, text)),
          // P40 — the last 100 messages are plenty to pick a phone conversation back up, and keep
          // the reply small even when older turns carry base64 image attachments.
          historyLimit: 100,
        );
        _status = connection.status;
      });

      await _openChat();
    } on HandshakeException catch (e) {
      setState(() => _status = ConnectionFailure(e.message));
    } catch (e) {
      setState(() => _status = ConnectionFailure(e.toString()));
    }
  }

  /// P84 — the screens a member passes before the chat. False when they stopped and the connection was closed.
  Future<bool> _memberGate(ServerConnection connection, AppLocalizations l10n) async {
    final user = connection.user!;
    // The hub restarted and doesn't hold their key: a token can't open it, only the password can.
    if (user.locked && !user.mustChangePassword) {
      await connection.goodbye('data locked');
      if (mounted) {
        setState(() {
          _useAccount = true;
          _status = ConnectionFailure(l10n.connLocked);
        });
      }
      return false;
    }
    if (user.mustChangePassword) {
      final change = mounted
          ? await showDialog<PasswordChange>(
              context: context,
              barrierDismissible: false,
              builder: (_) => ChangePasswordDialog(connection: connection, name: user.name, needsRecovery: user.needsRecovery),
            )
          : null;
      if (change == null) {
        await connection.goodbye('password not changed');
        if (mounted) setState(() => _status = ConnectionFailure(l10n.connChoosePassword));
        return false;
      }
      final code = change.recoveryCode;
      if (code != null && mounted) await showRecoveryCode(context, code);
    }
    // Encryption that signing in turned on for a member from before (the hub pushes the code on its own).
    final pushed = connection.takeUnclaimedRecoveryCode();
    if (pushed != null && mounted) await showRecoveryCode(context, pushed);
    // A recovery policy weaker than the one their data follows waits for their yes.
    if (user.policyPending && mounted) {
      final code = await showDialog<String>(context: context, builder: (_) => AcceptPolicyDialog(connection: connection, policy: user.recoveryPolicy));
      if (code != null && code.isNotEmpty && mounted) await showRecoveryCode(context, code, replacing: true);
    }
    // The owner recovered their data: they're told, once.
    final unseen = user.unseenRecoveries;
    if (unseen.isNotEmpty && mounted) {
      final seen = await showDialog<bool>(context: context, builder: (_) => RecoveryNoticeDialog(events: unseen));
      if (seen ?? false) {
        try {
          await connection.ackRecoveryNotices();
        } on PasswordException {
          // They'll see it again next time — not worth stopping the sign-in for.
        }
      }
    }
    return true;
  }

  // Phase 9.1 (redefined) — sweeps the LAN instead of asking the user to already know the IP.
  // Reuses the same `discover_hubs` Rust already exposes to the desktop's WorkspaceView, over the
  // mobile bridge. Only ever fills host/port — the auth key is never part of a hub's reply, so it
  // stays manual on purpose, same security boundary as the desktop and the QR pairing flow. Sweeps
  // whatever port is already typed in the form (same parsing `_connect()` uses) so a hub started
  // with `--listen` on a non-default port is still discoverable — falls back to the default only
  // when the field is empty/invalid.
  Future<void> _discoverHubs() async {
    if (!mounted) return;
    final port = int.tryParse(_portController.text.trim()) ?? ConnectionSettingsStore.defaultPort;
    final result = await showModalBottomSheet<DiscoveredHubDto>(
      context: context,
      isScrollControlled: true,
      builder: (context) => _DiscoveredHubsSheet(
        future: bridgeDiscoverHubs(port: port),
      ),
    );
    if (result == null || !mounted) return;
    // P36 — a TLS-only hub is reached by the name its certificate covers, not the LAN IP the sweep
    // found it at, so its advertised wss:// URL wins.
    final secureUri = result.secureUrl == null ? null : Uri.tryParse(result.secureUrl!);
    setState(() {
      _hostController.text = secureUri?.host ?? result.host;
      _portController.text = '${secureUri?.port ?? result.port}';
      _useTls = secureUri != null;
    });
  }

  Future<void> _scanQr() async {
    final payload = await Navigator.of(context).push<HubPairingPayload>(
      MaterialPageRoute(builder: (_) => const QrScanScreen()),
    );
    if (payload == null || !mounted) return;
    setState(() {
      _hostController.text = payload.host;
      _portController.text = '${payload.port}';
      _authKeyController.text = payload.authKey;
      _useTls = payload.useTls;
    });
  }

  /// P41 — (re)opens the chat for the live connection. The transcript belongs to this screen, not to
  /// `ChatScreen`, so backing out of the chat and coming back here keeps the conversation.
  Future<void> _openChat() async {
    final connection = _connection;
    final transcript = _transcript;
    if (connection == null || transcript == null || !mounted) return;
    await Navigator.of(context).push(
      MaterialPageRoute(builder: (_) => ChatScreen(connection: connection, transcript: transcript)),
    );
  }

  Future<void> _disconnect() async {
    await _connection?.goodbye('user disconnected');
  }

  @override
  void dispose() {
    _statusSubscription?.cancel();
    _transcript?.dispose();
    _connection?.goodbye('screen closed');
    _hostController.dispose();
    _portController.dispose();
    _authKeyController.dispose();
    _deviceNameController.dispose();
    _usernameController.dispose();
    _passwordController.dispose();
    super.dispose();
  }

  bool get _isConnected => _status is Connected;
  bool get _isBusy => _status is Connecting;

  @override
  Widget build(BuildContext context) {
    final l10n = AppLocalizations.of(context);
    return Scaffold(
      appBar: AppBar(
        // P137 — the mark beside the title (the form below has no room to spare on a small phone).
        title: Row(children: [const WardenLogo(size: 26), const SizedBox(width: 8), Flexible(child: Text(l10n.connTitle, overflow: TextOverflow.ellipsis))]),
        actions: [
          // P136 — the language of the app, reachable before anybody is connected.
          const LanguageMenu(),
          // P137 — dark or light.
          const ThemeButton(),
          // Phase 9.1 (redefined) — sweeps the LAN for a warden-server hub and fills in host/port
          // from the pick, instead of typing an IP by hand. Only useful before connecting.
          if (!_isConnected && !_isBusy)
            IconButton(
              icon: const Icon(Icons.wifi_find),
              tooltip: l10n.connDiscoverTooltip,
              onPressed: _discoverHubs,
            ),
          // Phase 9.7 — scans the desktop Workspace screen's pairing QR to fill in host/port/auth
          // key below, instead of typing them by hand. Only useful before connecting.
          if (!_isConnected && !_isBusy)
            IconButton(
              icon: const Icon(Icons.qr_code_scanner),
              tooltip: l10n.connScanTooltip,
              onPressed: _scanQr,
            ),
          // P84 — a member's own password, recovery code and TruthID, once connected as one.
          if (_isConnected && _connection?.user != null)
            IconButton(
              key: const Key('account-button'),
              icon: const Icon(Icons.account_circle),
              tooltip: l10n.connAccountTooltip,
              onPressed: () => Navigator.of(context).push(MaterialPageRoute(builder: (_) => AccountScreen(connection: _connection!))),
            ),
          // Phase 4.4 — sync doesn't depend on being connected to warden-server (it only talks to
          // a paired device over LAN and to Arweave/TruthID), so it's reachable independent of
          // this screen's connection state.
          IconButton(
            icon: const Icon(Icons.sync),
            tooltip: l10n.connSyncTooltip,
            onPressed: () => Navigator.of(context).push(MaterialPageRoute(builder: (_) => const SyncScreen())),
          ),
          // P72 (b) — skills live in this device's local vault, independent of the server connection; the connected hub
          // (if any) only writes drafts.
          IconButton(
            icon: const Icon(Icons.auto_awesome),
            tooltip: l10n.connSkillsTooltip,
            onPressed: () => Navigator.of(context).push(MaterialPageRoute(builder: (_) => SkillsScreen(draftBackend: _connection))),
          ),
        ],
      ),
      body: Padding(
        padding: const EdgeInsets.all(16),
        child: ListView(
          children: [
            TextField(
              controller: _hostController,
              enabled: !_isConnected && !_isBusy,
              decoration: InputDecoration(labelText: l10n.connHost),
            ),
            const SizedBox(height: 12),
            TextField(
              controller: _portController,
              enabled: !_isConnected && !_isBusy,
              keyboardType: TextInputType.number,
              decoration: InputDecoration(labelText: l10n.connPort),
            ),
            SwitchListTile(
              contentPadding: EdgeInsets.zero,
              title: Text(l10n.connUseTls),
              subtitle: Text(l10n.connUseTlsHint),
              value: _useTls,
              onChanged: _isConnected || _isBusy ? null : (value) => setState(() => _useTls = value),
            ),
            const SizedBox(height: 12),
            // P84 — the owner pairs with the hub's key; everyone else with their own account.
            SegmentedButton<bool>(
              segments: [
                ButtonSegment(value: false, label: Text(l10n.connPairingKey)),
                ButtonSegment(value: true, label: Text(l10n.connUsername)),
              ],
              selected: {_useAccount},
              onSelectionChanged: _isConnected || _isBusy ? null : (selected) => setState(() => _useAccount = selected.first),
            ),
            const SizedBox(height: 12),
            if (_useAccount) ...[
              TextField(
                controller: _usernameController,
                enabled: !_isConnected && !_isBusy,
                autocorrect: false,
                decoration: InputDecoration(labelText: l10n.connUsername),
              ),
              const SizedBox(height: 12),
              TextField(
                controller: _passwordController,
                enabled: !_isConnected && !_isBusy,
                obscureText: true,
                decoration: InputDecoration(labelText: l10n.connPassword, helperText: l10n.connPasswordHint),
              ),
            ] else
              TextField(
                controller: _authKeyController,
                enabled: !_isConnected && !_isBusy,
                obscureText: true,
                // A hint, not helper text: it takes no room, and the screen has none to spare on a small phone. The key pairs only
                // the hub's first device (P36); every other one pastes an invite's code here.
                decoration: InputDecoration(labelText: l10n.connAuthKey, hintText: l10n.connAuthKeyHint),
              ),
            const SizedBox(height: 12),
            TextField(
              controller: _deviceNameController,
              enabled: !_isConnected && !_isBusy,
              decoration: InputDecoration(labelText: l10n.connDeviceName),
            ),
            const SizedBox(height: 24),
            _StatusIndicator(status: _status),
            const SizedBox(height: 24),
            // P41 — a live connection can be resumed after backing out of the chat, instead of the
            // only way back being disconnect + reconnect.
            if (_isConnected) ...[
              FilledButton(onPressed: _openChat, child: Text(l10n.connResume)),
              const SizedBox(height: 12),
              OutlinedButton(onPressed: _disconnect, child: Text(l10n.connDisconnect)),
            ] else
              FilledButton(onPressed: _isBusy ? null : _connect, child: Text(l10n.connConnect)),
          ],
        ),
      ),
    );
  }
}

class _DiscoveredHubsSheet extends StatelessWidget {
  const _DiscoveredHubsSheet({required this.future});

  final Future<List<DiscoveredHubDto>> future;

  @override
  Widget build(BuildContext context) {
    final l10n = AppLocalizations.of(context);
    return SafeArea(
      child: Padding(
        padding: const EdgeInsets.all(16),
        child: FutureBuilder<List<DiscoveredHubDto>>(
          future: future,
          builder: (context, snapshot) {
            if (snapshot.connectionState != ConnectionState.done) {
              return const Padding(
                padding: EdgeInsets.symmetric(vertical: 32),
                child: Center(child: CircularProgressIndicator()),
              );
            }
            if (snapshot.hasError) {
              return Padding(
                padding: const EdgeInsets.symmetric(vertical: 16),
                child: Text(l10n.connDiscoveryFailed('${snapshot.error}'), style: TextStyle(color: Theme.of(context).colorScheme.error)),
              );
            }
            final hubs = snapshot.data!;
            if (hubs.isEmpty) {
              return Padding(
                padding: const EdgeInsets.symmetric(vertical: 16),
                child: Text(l10n.connNoHub),
              );
            }
            return Column(
              mainAxisSize: MainAxisSize.min,
              crossAxisAlignment: CrossAxisAlignment.stretch,
              children: [
                Padding(
                  padding: const EdgeInsets.only(bottom: 8),
                  child: Text(l10n.connHubsFound, style: const TextStyle(fontWeight: FontWeight.bold)),
                ),
                for (final hub in hubs)
                  ListTile(
                    title: Text(hub.serverName),
                    subtitle: Text(hub.secureUrl ?? '${hub.host}:${hub.port}'),
                    onTap: () => Navigator.of(context).pop(hub),
                  ),
              ],
            );
          },
        ),
      ),
    );
  }
}

class _StatusIndicator extends StatelessWidget {
  const _StatusIndicator({required this.status});

  final ConnectionStatus status;

  @override
  Widget build(BuildContext context) {
    final l10n = AppLocalizations.of(context);
    final (label, color) = switch (status) {
      Disconnected(:final reason) => (reason == null ? l10n.connStatusDisconnected : l10n.connStatusDisconnectedWhy(reason), Colors.grey),
      Connecting() => (l10n.connStatusConnecting, Colors.orange),
      Connected(:final serverName) => (l10n.connStatusConnected(serverName), Colors.green),
      ConnectionFailure(:final message) => (l10n.connStatusError(message), Colors.red),
    };
    return Row(
      children: [
        Icon(Icons.circle, size: 12, color: color),
        const SizedBox(width: 8),
        Expanded(child: Text(label, style: Theme.of(context).textTheme.bodyLarge)),
      ],
    );
  }
}
