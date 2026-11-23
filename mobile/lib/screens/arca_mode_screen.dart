import 'package:flutter/material.dart';

import '../l10n/generated/app_localizations.dart';
import '../services/arca_mode_flow.dart';
import '../services/arca_pair_flow.dart';
import '../services/connection_settings.dart';
import '../src/rust/api/arca_mode.dart' as bridge;
import 'arca_pair_screen.dart';

/// The arca mode of this phone: turn it on, sync by hand, and see (and acknowledge) the newer key epochs that stop the vault.
///
/// The hub address and token are the ones the Connection screen already saved, read again at every sync. When that hub is plain
/// `http`, the sync warns the person that the device token will travel in the clear. The agreement is kept only in this screen's
/// memory and only for that exact address: it is never written to disk, so leaving the screen (or changing the hub) asks again.
class ArcaModeScreen extends StatefulWidget {
  const ArcaModeScreen({super.key, this.backend, this.pairBackend});

  /// Tests pass a fake; the app uses the Rust bridge.
  final ArcaModeBackend? backend;

  /// The backend of the key swap screens this one opens (tests pass a fake).
  final ArcaPairBackend? pairBackend;

  @override
  State<ArcaModeScreen> createState() => _ArcaModeScreenState();
}

/// The saved hub as the sync would use it: where, the token, and whether the address is TLS.
typedef _SavedHub = ({String origin, String token, bool tls});

class _ArcaModeScreenState extends State<ArcaModeScreen> {
  late final ArcaModeBackend _backend = widget.backend ?? BridgeArcaModeBackend();

  bool _loading = true;
  bool _busy = false;

  /// A dialog is open or about to open: a second tap meanwhile must not stack another one (`_busy` only starts after the answer).
  bool _asking = false;
  bool _on = false;

  /// Whether the mode could be read. When it could not, the screen does not guess "off": it would offer an irreversible "turn on".
  bool _modeKnown = true;

  /// Whether the newer key epochs could be read. When they could not, an empty list must not be taken for "nothing pending".
  bool _epochsKnown = true;
  String? _hubOrigin;
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

  Future<_SavedHub?> _savedHub() async {
    final store = ConnectionSettingsStore();
    final settings = await store.load();
    final token = settings == null ? null : await store.deviceTokenFor(settings.host, settings.port);
    if (settings == null || token == null) return null;
    // An IPv6 literal (`::1`) needs its brackets in an address, or the port is read as part of it.
    final host = settings.host.contains(':') ? '[${settings.host}]' : settings.host;
    return (origin: '${settings.useTls ? 'https' : 'http'}://$host:${settings.port}', token: token, tls: settings.useTls);
  }

  Future<void> _load() async {
    try {
      final hub = await _savedHub();
      if (!mounted) return;
      _hubOrigin = hub?.origin;
      await _refresh();
    } catch (_) {
      if (mounted) setState(() => _modeKnown = false);
    }
    if (mounted) setState(() => _loading = false);
  }

  /// Reads the mode and, when it is on, the newer key epochs again. What could not be read is flagged (`_modeKnown`, `_epochsKnown`)
  /// and the screen keeps what it knew: it never turns a failed read into "off" or "nothing pending". Returns whether both were read.
  Future<bool> _refresh() async {
    bridge.ArcaModeStatusDto status;
    try {
      status = await _backend.status();
    } catch (_) {
      if (mounted) setState(() => _modeKnown = false);
      return false;
    }
    List<bridge.PendingEpochDto>? epochs = const [];
    if (status.on_) {
      try {
        epochs = await _backend.epochsPending();
      } catch (_) {
        epochs = null;
      }
    }
    if (!mounted) return false;
    setState(() {
      _modeKnown = true;
      _on = status.on_;
      _epochsKnown = epochs != null;
      if (epochs != null) _epochs = epochs;
    });
    return epochs != null;
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
    if (_busy || _asking) return;
    _asking = true;
    final l10n = AppLocalizations.of(context);
    final bool? yes;
    try {
      yes = await showDialog<bool>(
        context: context,
        barrierDismissible: false,
        builder: (context) => AlertDialog(
          title: Text(l10n.arcaModeEnableTitle),
          content: Text(l10n.arcaModeEnableWarning),
          actions: [
            TextButton(key: const Key('arca-mode-enable-no'), onPressed: () => Navigator.of(context).pop(false), child: Text(l10n.arcaModeEnableNo)),
            FilledButton(key: const Key('arca-mode-enable-yes'), onPressed: () => Navigator.of(context).pop(true), child: Text(l10n.arcaModeEnableYes)),
          ],
        ),
      );
    } finally {
      _asking = false;
    }
    // Only an explicit "yes" turns it on: closing the dialog any other way (back button) answers `null`.
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

  /// True when the sync may go ahead: the hub is `https`, or the person has agreed (now or before, in this screen) to this exact address.
  Future<bool> _mayUseThisAddress(AppLocalizations l10n, _SavedHub hub) async {
    if (hub.tls || _insecureAgreedFor == hub.origin) {
      _asking = false;
      return true;
    }
    final bool? agreed;
    try {
      agreed = await showDialog<bool>(
        context: context,
        barrierDismissible: false,
        builder: (context) => AlertDialog(
          title: Text(l10n.arcaModeInsecureTitle),
          content: Text(l10n.arcaModeInsecureWarning(hub.origin)),
          actions: [
            TextButton(key: const Key('arca-mode-insecure-no'), onPressed: () => Navigator.of(context).pop(false), child: Text(l10n.arcaModeInsecureNo)),
            FilledButton(key: const Key('arca-mode-insecure-yes'), onPressed: () => Navigator.of(context).pop(true), child: Text(l10n.arcaModeInsecureYes)),
          ],
        ),
      );
    } finally {
      _asking = false;
    }
    // Only an explicit "yes" lets the token go in the clear: any other way out of the dialog answers `null`.
    if (agreed != true) return false;
    _insecureAgreedFor = hub.origin;
    return true;
  }

  Future<void> _syncNow() async {
    if (_busy || _asking) return;
    _asking = true;
    final l10n = AppLocalizations.of(context);
    // Read again now, not at open: the hub or its token may have changed since, and the warning must name the address that is used.
    final _SavedHub? hub;
    try {
      hub = await _savedHub();
    } catch (_) {
      _asking = false;
      _say(arcaModeErrorText(l10n, bridge.ArcaModeErrorKind.other), error: true);
      return;
    }
    if (hub == null) {
      _asking = false;
      _say(l10n.arcaModeNoHub, error: true);
      return;
    }
    if (!mounted) {
      _asking = false;
      return;
    }
    // `_mayUseThisAddress` ends the "asking" state itself, with or without a dialog.
    if (!await _mayUseThisAddress(l10n, hub) || !mounted) return;
    setState(() {
      _busy = true;
      _message = null;
    });
    try {
      final result = await _backend.syncRun(hubUrl: hub.origin, deviceToken: hub.token, allowInsecureHttp: !hub.tls);
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

  /// Opens the key swap screen for one half. Whatever it did (a key replaced, a key handed over) changes what this screen shows, so
  /// the mode and the newer epochs are read again when it comes back.
  Future<void> _openSwap(ArcaSwapSide side) async {
    await Navigator.of(context).push(MaterialPageRoute<void>(builder: (_) => ArcaPairScreen(backend: widget.pairBackend, swap: side)));
    if (mounted) await _refresh();
  }

  /// Replacing this device's key drops its local sync copy, so it asks first, and about the epochs read NOW (not the ones the screen
  /// drew a while ago): the person confirms what is pending at the moment of the tap. Only an explicit "yes" opens the swap screen.
  Future<void> _replaceKey() async {
    if (_busy || _asking) return;
    _asking = true;
    final l10n = AppLocalizations.of(context);
    final List<bridge.PendingEpochDto> fresh;
    try {
      fresh = await _backend.epochsPending();
    } catch (error) {
      _asking = false;
      _say(arcaModeErrorText(l10n, _kindOf(error)), error: true);
      return;
    }
    if (!mounted) {
      _asking = false;
      return;
    }
    if (fresh.isEmpty && _busy) {
      _asking = false;
      await _refresh();
      _say(l10n.arcaSwapNothingPending, error: true);
      return;
    }
    final bool? yes;
    try {
      yes = await showDialog<bool>(
        context: context,
        barrierDismissible: false,
        builder: (context) => AlertDialog(
          title: Text(l10n.arcaSwapReplaceConfirmTitle),
          content: SingleChildScrollView(
            child: Column(
              mainAxisSize: MainAxisSize.min,
              crossAxisAlignment: CrossAxisAlignment.start,
              children: [
                Text(l10n.arcaSwapReplaceConfirmBody),
                const SizedBox(height: 12),
                for (final item in fresh) Text(l10n.arcaModeEpochItem(item.epoch, epochShortId(item.id))),
              ],
            ),
          ),
          actions: [
            TextButton(key: const Key('arca-mode-swap-no'), onPressed: () => Navigator.of(context).pop(false), child: Text(l10n.arcaSwapReplaceConfirmNo)),
            FilledButton(key: const Key('arca-mode-swap-yes'), onPressed: () => Navigator.of(context).pop(true), child: Text(l10n.arcaSwapReplaceConfirmYes)),
          ],
        ),
      );
    } finally {
      _asking = false;
    }
    // Only an explicit "yes": closing the dialog any other way (back button) answers `null`.
    if (yes != true || !mounted) return;
    await _openSwap(ArcaSwapSide.replace);
  }

  Future<void> _acknowledge() async {
    if (_busy) return;
    final l10n = AppLocalizations.of(context);
    // What the person is looking at, not a list read now: only those pairs can be acknowledged.
    final seen = List.of(_epochs);
    setState(() {
      _busy = true;
      _message = null;
    });
    int? count;
    try {
      count = await _backend.epochsAcknowledge(seen);
    } catch (error) {
      _say(arcaModeErrorText(l10n, _kindOf(error)), error: true);
    }
    await _refresh();
    if (count != null) {
      // Nothing acknowledged means the list changed under the person: the vault is still stopped, so "acknowledged" would be a lie.
      if (count == 0) {
        _say(l10n.arcaModeEpochsChanged, error: true);
      } else {
        _say(l10n.arcaModeEpochsAcknowledged(count));
      }
    }
    if (mounted) setState(() => _busy = false);
  }

  @override
  Widget build(BuildContext context) {
    final l10n = AppLocalizations.of(context);
    return Scaffold(
      appBar: AppBar(title: Text(l10n.arcaModeTitle)),
      // Pulling down reads the mode again: the way out of "could not read it" without leaving the screen.
      body: RefreshIndicator(
        onRefresh: () async {
          await _refresh();
        },
        child: ListView(
          physics: const AlwaysScrollableScrollPhysics(),
          padding: const EdgeInsets.all(16),
          children: _loading ? const [Center(child: CircularProgressIndicator())] : _body(l10n),
        ),
      ),
    );
  }

  List<Widget> _body(AppLocalizations l10n) {
    final error = Theme.of(context).colorScheme.error;
    final errorStyle = TextStyle(color: error);
    return [
      Text(l10n.arcaModeIntro),
      const SizedBox(height: 16),
      if (!_modeKnown)
        // Not "off": the mode could not be read, and offering to turn it on could be offering something already done.
        Text(arcaModeErrorText(l10n, bridge.ArcaModeErrorKind.other), style: errorStyle)
      else ...[
        Text(_on ? l10n.arcaModeStatusOn : l10n.arcaModeStatusOff, style: Theme.of(context).textTheme.titleMedium),
        const SizedBox(height: 16),
        if (!_on)
          FilledButton(key: const Key('arca-mode-enable'), onPressed: _busy ? null : _enable, child: Text(l10n.arcaModeEnable))
        else if (_hubOrigin == null)
          Text(l10n.arcaModeNoHub)
        else
          FilledButton(key: const Key('arca-mode-sync'), onPressed: _busy ? null : _syncNow, child: Text(l10n.arcaModeSyncNow)),
        if (!_on && _hubOrigin == null) ...[const SizedBox(height: 8), Text(l10n.arcaModeNoHub)],
        // The key swap (P-49): hand the new key over any time the mode is on; replace this device's key only while a newer one is pending.
        if (_on && _hubOrigin != null) ...[
          const SizedBox(height: 8),
          // With a newer key pending this device is the stranded one: its own key is the old one, so it must not hand it over.
          if (_epochs.isEmpty)
            OutlinedButton(key: const Key('arca-mode-swap-grant'), onPressed: _busy ? null : () => _openSwap(ArcaSwapSide.grant), child: Text(l10n.arcaSwapGrantOpen))
          else ...[
            const SizedBox(height: 8),
            OutlinedButton(key: const Key('arca-mode-swap-replace'), onPressed: _busy ? null : _replaceKey, child: Text(l10n.arcaSwapReplaceOpen)),
          ],
        ],
      ],
      if (_busy) ...[
        const SizedBox(height: 16),
        Text(l10n.arcaModeSyncing),
        const SizedBox(height: 8),
        const LinearProgressIndicator(),
      ],
      if (_message != null) ...[
        const SizedBox(height: 16),
        Text(_message!, style: _messageIsError ? errorStyle : null),
      ],
      // An unreadable list is not an empty one: with the vault stopped by a rotation, hiding it would hide the reason it does not sync.
      if (_modeKnown && _on && !_epochsKnown) ...[
        const SizedBox(height: 16),
        Text(arcaModeErrorText(l10n, bridge.ArcaModeErrorKind.other), key: const Key('arca-mode-epochs-unreadable'), style: errorStyle),
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
