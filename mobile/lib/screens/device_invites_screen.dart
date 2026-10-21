import 'dart:async';

import 'package:flutter/material.dart';
import 'package:flutter/services.dart';

import '../l10n/generated/app_localizations.dart';
import '../protocol/messages.dart' show PendingDeviceInvite;
import '../services/device_invites.dart';
import 'agents_screen.dart' show askPairingKey;

/// P36 — the devices screen: makes a one-use invite for a new device and changes the hub's pairing key. The pairing key
/// pairs only the hub's first device, so every other one comes in with an invite's code, pasted where the key would go. The
/// pairing key is asked on every action and never kept; the code (and the new key) show once, because the hub keeps only a
/// hash of the code.
class DeviceInvitesScreen extends StatefulWidget {
  const DeviceInvitesScreen({super.key, required this.backend});

  final DeviceInvitesBackend backend;

  @override
  State<DeviceInvitesScreen> createState() => _DeviceInvitesScreenState();
}

class _DeviceInvitesScreenState extends State<DeviceInvitesScreen> {
  final _name = TextEditingController();
  final _minutes = TextEditingController(text: '15');
  DeviceInvite? _invite;
  String? _newKey;
  String? _error;
  String? _cancelled;
  // The invites that wait, which any paired device may see (by name, never the code).
  List<PendingDeviceInvite> _waiting = const [];

  @override
  void initState() {
    super.initState();
    _loadWaiting();
  }

  Future<void> _loadWaiting() async {
    try {
      final waiting = await widget.backend.listDeviceInvites();
      if (mounted) setState(() => _waiting = waiting);
    } catch (e) {
      if (mounted) setState(() => _error = e.toString());
    }
  }

  Future<void> _cancel(PendingDeviceInvite invite) async {
    setState(() {
      _error = null;
      _invite = null;
      _newKey = null;
      _cancelled = null;
    });
    List<PendingDeviceInvite>? left;
    final l10n = AppLocalizations.of(context);
    final ok = await askPairingKey(
      context,
      title: l10n.invCancelTitle,
      message: l10n.invCancelMessage(invite.name),
      submit: (key) async => left = await widget.backend.cancelDeviceInvite(key, invite.name),
      onError: (error) {
        if (mounted) setState(() => _error = error);
      },
    );
    if (ok && mounted) {
      setState(() {
        _waiting = left ?? _waiting;
        _cancelled = invite.name;
      });
    }
  }

  @override
  void dispose() {
    _name.dispose();
    _minutes.dispose();
    super.dispose();
  }

  int? get _ttl {
    final minutes = int.tryParse(_minutes.text.trim());
    return minutes != null && minutes >= 1 ? minutes : null;
  }

  Future<void> _createInvite() async {
    final name = _name.text.trim();
    final ttl = _ttl;
    final l10n = AppLocalizations.of(context);
    if (name.isEmpty || ttl == null) {
      setState(() => _error = l10n.invFill);
      return;
    }
    setState(() {
      _error = null;
      _invite = null;
      _newKey = null;
      _cancelled = null;
    });
    DeviceInvite? made;
    final ok = await askPairingKey(
      context,
      title: l10n.invCreateTitle,
      message: l10n.invCreateMessage(ttl),
      submit: (key) async => made = await widget.backend.createDeviceInvite(key, name, ttl),
      onError: (error) {
        if (mounted) setState(() => _error = error);
      },
    );
    if (ok && mounted) {
      setState(() {
        _invite = made;
        _cancelled = null;
      });
      unawaited(_loadWaiting());
    }
  }

  Future<void> _rotateKey() async {
    setState(() {
      _error = null;
      _invite = null;
      _newKey = null;
      _cancelled = null;
    });
    String? made;
    final l10n = AppLocalizations.of(context);
    final ok = await askPairingKey(
      context,
      title: l10n.invRotateTitle,
      message: l10n.invRotateMessage,
      submit: (key) async => made = await widget.backend.rotatePairingKey(key),
      onError: (error) {
        if (mounted) setState(() => _error = error);
      },
    );
    if (ok && mounted) setState(() => _newKey = made);
  }

  @override
  Widget build(BuildContext context) {
    final invite = _invite;
    final newKey = _newKey;
    final l10n = AppLocalizations.of(context);
    return Scaffold(
      appBar: AppBar(title: Text(l10n.invTitle)),
      body: ListView(
        padding: const EdgeInsets.all(16),
        children: [
          Text(l10n.invHint),
          const SizedBox(height: 16),
          TextField(
            key: const Key('invite-name'),
            controller: _name,
            decoration: InputDecoration(labelText: l10n.invWho, hintText: l10n.invWhoHint),
          ),
          const SizedBox(height: 12),
          TextField(
            key: const Key('invite-minutes'),
            controller: _minutes,
            keyboardType: TextInputType.number,
            decoration: InputDecoration(labelText: l10n.invMinutes),
          ),
          const SizedBox(height: 16),
          Row(
            children: [
              FilledButton(key: const Key('invite-create'), onPressed: _createInvite, child: Text(l10n.invCreate)),
              const SizedBox(width: 12),
              TextButton(key: const Key('rotate-key'), onPressed: _rotateKey, child: Text(l10n.invRotateTitle)),
            ],
          ),
          if (_error != null) ...[
            const SizedBox(height: 12),
            Text(_error!, key: const Key('devices-error'), style: TextStyle(color: Theme.of(context).colorScheme.error)),
          ],
          if (_cancelled != null) ...[
            const SizedBox(height: 12),
            Text(l10n.invCancelled(_cancelled!), key: const Key('invite-cancelled')),
          ],
          if (_waiting.isNotEmpty) ...[
            const SizedBox(height: 16),
            Text(l10n.invWaiting, style: const TextStyle(fontWeight: FontWeight.bold)),
            for (final waiting in _waiting)
              ListTile(
                key: Key('waiting-${waiting.name}'),
                contentPadding: EdgeInsets.zero,
                title: Text(waiting.name),
                subtitle: Text(l10n.invValidUntil(_when(waiting.expiresMs))),
                trailing: TextButton(
                  key: Key('cancel-${waiting.name}'),
                  onPressed: () => _cancel(waiting),
                  child: Text(l10n.commonCancel),
                ),
              ),
          ],
          if (invite != null) ...[
            const SizedBox(height: 16),
            Text(l10n.invMade(invite.name, _when(invite.expiresMs))),
            const SizedBox(height: 8),
            _Secret(secret: invite.code, valueKey: const Key('invite-code')),
          ],
          if (newKey != null) ...[
            const SizedBox(height: 16),
            Text(l10n.invNewKey),
            const SizedBox(height: 8),
            _Secret(secret: newKey, valueKey: const Key('new-key')),
          ],
        ],
      ),
    );
  }

  String _when(int ms) {
    final t = DateTime.fromMillisecondsSinceEpoch(ms);
    String two(int n) => n.toString().padLeft(2, '0');
    return '${two(t.day)}/${two(t.month)} ${two(t.hour)}:${two(t.minute)}';
  }
}

/// A code or key to copy, selectable and with a copy button.
class _Secret extends StatelessWidget {
  const _Secret({required this.secret, required this.valueKey});

  final String secret;
  final Key valueKey;

  @override
  Widget build(BuildContext context) {
    return Row(
      children: [
        Expanded(child: SelectableText(secret, key: valueKey, style: const TextStyle(fontFamily: 'monospace'))),
        IconButton(
          tooltip: AppLocalizations.of(context).commonCopy,
          icon: const Icon(Icons.copy),
          onPressed: () => Clipboard.setData(ClipboardData(text: secret)),
        ),
      ],
    );
  }
}
