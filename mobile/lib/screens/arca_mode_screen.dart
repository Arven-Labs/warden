import 'package:flutter/material.dart';

import '../l10n/generated/app_localizations.dart';
import '../services/arca_mode_flow.dart';
import '../services/connection_settings.dart';
import '../src/rust/api/arca_mode.dart' as bridge;

/// The arca mode of this phone: turn it on, sync by hand, and see (and acknowledge) the newer key epochs that stop the vault.
///
/// The hub address and token are the ones the Connection screen already saved. When that hub is plain `http`, the first sync of
/// this screen warns the person that the device token will travel in the clear. The agreement is kept only in this screen's memory
/// and only for that exact address: it is never written to disk, so leaving the screen (or changing the hub) asks again.
class ArcaModeScreen extends StatefulWidget {
  const ArcaModeScreen({super.key, this.backend});

  /// Tests pass a fake; the app uses the Rust bridge.
  final ArcaModeBackend? backend;

  @override
  State<ArcaModeScreen> createState() => _ArcaModeScreenState();
}

class _ArcaModeScreenState extends State<ArcaModeScreen> {
  late final ArcaModeBackend _backend = widget.backend ?? BridgeArcaModeBackend();

  bool _loading = true;
  bool _busy = false;
  bool _on = false;
  bool _useTls = true;
  String? _hubOrigin;
  String? _token;
  List<bridge.PendingEpochDto> _epochs = const [];

  /// What the last action said, and whether it was a problem.
  String? _message;
  bool _messageIsError = false;

  /// The address the person agreed to send the token to in the clear, in this screen only (see the class comment).
  String? _insecureAgreedFor;

  @override
  void initState() {
    super.initState();
    _load();
  }

  Future<void> _load() async {
    final store = ConnectionSettingsStore();
    final settings = await store.load();
    final token = settings == null ? null : await store.deviceTokenFor(settings.host, settings.port);
    if (!mounted) return;
    if (settings != null && token != null) {
      _useTls = settings.useTls;
      _hubOrigin = '${settings.useTls ? 'https' : 'http'}://${settings.host}:${settings.port}';
      _token = token;
    }
    await _refresh();
    if (mounted) setState(() => _loading = false);
  }

  /// Reads the mode and, when it is on, the newer key epochs again. A failure here is not fatal: the screen keeps what it knew.
  Future<void> _refresh() async {
    try {
      final status = await _backend.status();
      final epochs = status.on_ ? await _backend.epochsPending() : const <bridge.PendingEpochDto>[];
      if (!mounted) return;
      setState(() {
        _on = status.on_;
        _epochs = epochs;
      });
    } catch (_) {
      // Keep what was on the screen: the next action reads it again.
    }
  }

  bridge.ArcaModeErrorKind _kindOf(Object error) => error is bridge.ArcaModeFailure ? error.kind : bridge.ArcaModeErrorKind.other;

  void _say(String text, {bool error = false}) {
    if (!mounted) return;
    setState(() {
      _message = text;
      _messageIsError = error;
    });
  }

  Future<void> _enable() async {
    final l10n = AppLocalizations.of(context);
    final yes = await showDialog<bool>(
      context: context,
      builder: (context) => AlertDialog(
        title: Text(l10n.arcaModeEnableTitle),
        content: Text(l10n.arcaModeEnableWarning),
        actions: [
          TextButton(key: const Key('arca-mode-enable-no'), onPressed: () => Navigator.of(context).pop(false), child: Text(l10n.arcaModeEnableNo)),
          FilledButton(key: const Key('arca-mode-enable-yes'), onPressed: () => Navigator.of(context).pop(true), child: Text(l10n.arcaModeEnableYes)),
        ],
      ),
    );
    if (yes != true || !mounted) return;
    setState(() {
      _busy = true;
      _message = null;
    });
    try {
      await _backend.enable();
    } catch (error) {
      // "Already on" is not a failure. Any other one may still have saved the mode before it failed (the process can die after
      // writing it), so the status below is the truth, not the error.
      final kind = _kindOf(error);
      if (kind != bridge.ArcaModeErrorKind.alreadyOn && mounted) _say(arcaModeErrorText(l10n, kind), error: true);
    }
    await _refresh();
    if (mounted) setState(() => _busy = false);
  }

  /// True when the sync may go ahead: the hub is `https`, or the person has agreed (now or before, in this screen) to this address.
  Future<bool> _mayUseThisAddress(AppLocalizations l10n, String origin) async {
    if (_useTls || _insecureAgreedFor == origin) return true;
    final agreed = await showDialog<bool>(
      context: context,
      builder: (context) => AlertDialog(
        title: Text(l10n.arcaModeInsecureTitle),
        content: Text(l10n.arcaModeInsecureWarning(origin)),
        actions: [
          TextButton(key: const Key('arca-mode-insecure-no'), onPressed: () => Navigator.of(context).pop(false), child: Text(l10n.arcaModeInsecureNo)),
          FilledButton(key: const Key('arca-mode-insecure-yes'), onPressed: () => Navigator.of(context).pop(true), child: Text(l10n.arcaModeInsecureYes)),
        ],
      ),
    );
    if (agreed != true) return false;
    _insecureAgreedFor = origin;
    return true;
  }

  Future<void> _syncNow() async {
    final l10n = AppLocalizations.of(context);
    final origin = _hubOrigin;
    final token = _token;
    if (origin == null || token == null) return;
    if (!await _mayUseThisAddress(l10n, origin) || !mounted) return;
    setState(() {
      _busy = true;
      _message = null;
    });
    try {
      final result = await _backend.syncRun(hubUrl: origin, deviceToken: token, allowInsecureHttp: !_useTls);
      if (result.vaultEmptied) {
        _say(l10n.arcaModeVaultEmptied, error: true);
      } else {
        _say(l10n.arcaModeSynced(result.pulled));
      }
    } catch (error) {
      _say(arcaModeErrorText(l10n, _kindOf(error)), error: true);
    }
    await _refresh();
    if (mounted) setState(() => _busy = false);
  }

  Future<void> _acknowledge() async {
    final l10n = AppLocalizations.of(context);
    final seen = List.of(_epochs);
    setState(() {
      _busy = true;
      _message = null;
    });
    try {
      final count = await _backend.epochsAcknowledge(seen);
      await _refresh();
      // Nothing acknowledged means the list changed under the person: the vault is still stopped, so "acknowledged" would be a lie.
      if (count == 0) {
        _say(l10n.arcaModeEpochsChanged, error: true);
      } else {
        _say(l10n.arcaModeEpochsAcknowledged(count));
      }
    } catch (error) {
      _say(arcaModeErrorText(l10n, _kindOf(error)), error: true);
    }
    if (mounted) setState(() => _busy = false);
  }

  @override
  Widget build(BuildContext context) {
    final l10n = AppLocalizations.of(context);
    return Scaffold(
      appBar: AppBar(title: Text(l10n.arcaModeTitle)),
      body: ListView(
        padding: const EdgeInsets.all(16),
        children: _loading ? const [Center(child: CircularProgressIndicator())] : _body(l10n),
      ),
    );
  }

  List<Widget> _body(AppLocalizations l10n) {
    final error = Theme.of(context).colorScheme.error;
    return [
      Text(l10n.arcaModeIntro),
      const SizedBox(height: 16),
      Text(_on ? l10n.arcaModeStatusOn : l10n.arcaModeStatusOff, style: Theme.of(context).textTheme.titleMedium),
      const SizedBox(height: 16),
      if (!_on)
        FilledButton(key: const Key('arca-mode-enable'), onPressed: _busy ? null : _enable, child: Text(l10n.arcaModeEnable))
      else if (_hubOrigin == null)
        Text(l10n.arcaModeNoHub)
      else
        FilledButton(key: const Key('arca-mode-sync'), onPressed: _busy ? null : _syncNow, child: Text(l10n.arcaModeSyncNow)),
      if (!_on && _hubOrigin == null) ...[const SizedBox(height: 8), Text(l10n.arcaModeNoHub)],
      if (_busy) ...[
        const SizedBox(height: 16),
        Text(l10n.arcaModeSyncing),
        const SizedBox(height: 8),
        const LinearProgressIndicator(),
      ],
      if (_message != null) ...[
        const SizedBox(height: 16),
        Text(_message!, style: _messageIsError ? TextStyle(color: error) : null),
      ],
      if (_epochs.isNotEmpty) ...[
        const SizedBox(height: 24),
        Text(l10n.arcaModeEpochsTitle, style: Theme.of(context).textTheme.titleMedium),
        const SizedBox(height: 8),
        Text(l10n.arcaModeEpochsIntro),
        const SizedBox(height: 8),
        for (final item in _epochs) Text(l10n.arcaModeEpochItem(item.epoch, epochShortId(item.id))),
        const SizedBox(height: 16),
        OutlinedButton(key: const Key('arca-mode-ack'), onPressed: _busy ? null : _acknowledge, child: Text(l10n.arcaModeEpochsAcknowledge)),
      ],
    ];
  }
}
