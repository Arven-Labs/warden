import 'package:flutter/material.dart';
import 'package:flutter/services.dart';

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
    if (name.isEmpty || ttl == null) {
      setState(() => _error = 'Fill in who it is for and a validity of at least 1 minute.');
      return;
    }
    setState(() {
      _error = null;
      _invite = null;
      _newKey = null;
    });
    DeviceInvite? made;
    final ok = await askPairingKey(
      context,
      title: 'Create invite',
      message: 'The code works once, for $ttl minutes.',
      submit: (key) async => made = await widget.backend.createDeviceInvite(key, name, ttl),
      onError: (error) {
        if (mounted) setState(() => _error = error);
      },
    );
    if (ok && mounted) setState(() => _invite = made);
  }

  Future<void> _rotateKey() async {
    setState(() {
      _error = null;
      _invite = null;
      _newKey = null;
    });
    String? made;
    final ok = await askPairingKey(
      context,
      title: 'Change the pairing key',
      message: 'The old key stops opening the hub\'s administration. Devices already paired keep working.',
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
    return Scaffold(
      appBar: AppBar(title: const Text('Devices')),
      body: ListView(
        padding: const EdgeInsets.all(16),
        children: [
          const Text(
            "The pairing key pairs only the hub's first device. For each other one, make an invite and give the person the code: "
            'they paste it where the key would go. It works once.',
          ),
          const SizedBox(height: 16),
          TextField(
            key: const Key('invite-name'),
            controller: _name,
            decoration: const InputDecoration(labelText: 'Who is it for', hintText: "Ana's phone"),
          ),
          const SizedBox(height: 12),
          TextField(
            key: const Key('invite-minutes'),
            controller: _minutes,
            keyboardType: TextInputType.number,
            decoration: const InputDecoration(labelText: 'Valid for (minutes)'),
          ),
          const SizedBox(height: 16),
          Row(
            children: [
              FilledButton(key: const Key('invite-create'), onPressed: _createInvite, child: const Text('Create invite')),
              const SizedBox(width: 12),
              TextButton(key: const Key('rotate-key'), onPressed: _rotateKey, child: const Text('Change the pairing key')),
            ],
          ),
          if (_error != null) ...[
            const SizedBox(height: 12),
            Text(_error!, key: const Key('devices-error'), style: TextStyle(color: Theme.of(context).colorScheme.error)),
          ],
          if (invite != null) ...[
            const SizedBox(height: 16),
            Text('Invite for ${invite.name}, valid until ${_when(invite.expiresMs)}. The code shows only now:'),
            const SizedBox(height: 8),
            _Secret(secret: invite.code, valueKey: const Key('invite-code')),
          ],
          if (newKey != null) ...[
            const SizedBox(height: 16),
            const Text('New pairing key, shown only now:'),
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
          tooltip: 'Copy',
          icon: const Icon(Icons.copy),
          onPressed: () => Clipboard.setData(ClipboardData(text: secret)),
        ),
      ],
    );
  }
}
