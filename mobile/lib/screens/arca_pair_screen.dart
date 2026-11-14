import 'package:flutter/material.dart';

import '../l10n/generated/app_localizations.dart';
import '../services/arca_pair_flow.dart';
import '../services/connection_settings.dart';
import '../src/rust/api/arca_pair.dart' as bridge;

/// Pairs this phone, as the NEW device, with another that already has the vault key (arca pairing through the hub).
///
/// The phone shows a code to type on the other device, and later asks for the fingerprint the other device shows. That field
/// takes only typed characters: copying the fingerprint across would defeat the check against a hub that swaps the key.
class ArcaPairScreen extends StatefulWidget {
  const ArcaPairScreen({super.key, this.backend});

  /// Tests pass a fake; the app uses the Rust bridge.
  final ArcaPairBackend? backend;

  @override
  State<ArcaPairScreen> createState() => _ArcaPairScreenState();
}

enum _Phase { loading, noHub, ready, waiting, asking, done, cancelled, failed }

class _ArcaPairScreenState extends State<ArcaPairScreen> {
  late final ArcaPairBackend _backend = widget.backend ?? BridgeArcaPairBackend();
  final _fingerprint = TextEditingController();

  _Phase _phase = _Phase.loading;
  String? _hubUrl;
  String? _token;
  String? _deviceId;
  String? _code;
  bridge.ArcaPairErrorKind? _failure;
  bool _running = false;

  @override
  void initState() {
    super.initState();
    _load();
  }

  @override
  void dispose() {
    // Leaving the screen is giving up: the bridge frees its one slot.
    if (_running) _backend.cancel();
    _fingerprint.dispose();
    super.dispose();
  }

  Future<void> _load() async {
    final store = ConnectionSettingsStore();
    final settings = await store.load();
    final token = settings == null ? null : await store.deviceTokenFor(settings.host, settings.port);
    final deviceId = await store.getOrCreateDeviceId();
    if (!mounted) return;
    setState(() {
      if (settings == null || token == null) {
        _phase = _Phase.noHub;
        return;
      }
      _hubUrl = '${settings.useTls ? 'https' : 'http'}://${settings.host}:${settings.port}';
      _token = token;
      _deviceId = deviceId;
      _phase = _Phase.ready;
    });
  }

  Future<void> _start() async {
    setState(() {
      _phase = _Phase.waiting;
      _code = null;
      _failure = null;
      _fingerprint.clear();
    });
    try {
      await _backend.start(hubUrl: _hubUrl!, deviceToken: _token!, deviceId: _deviceId!);
    } on bridge.ArcaPairFailure catch (failure) {
      if (mounted) {
        setState(() {
          _failure = failure.kind;
          _phase = _Phase.failed;
        });
      }
      return;
    }
    _running = true;
    while (mounted && _running) {
      final bridge.ArcaPairStep step;
      try {
        step = await _backend.next();
      } on bridge.ArcaPairFailure catch (failure) {
        _finish(_Phase.failed, failure.kind);
        return;
      }
      if (!mounted) return;
      switch (step.kind) {
        case bridge.ArcaPairStepKind.offerCode:
          setState(() => _code = step.code);
        case bridge.ArcaPairStepKind.askFingerprint:
          setState(() => _phase = _Phase.asking);
        case bridge.ArcaPairStepKind.done:
          _finish(_Phase.done, null);
        case bridge.ArcaPairStepKind.cancelled:
          _finish(_Phase.cancelled, null);
        case bridge.ArcaPairStepKind.failed:
          _finish(_Phase.failed, step.failure?.kind ?? bridge.ArcaPairErrorKind.hub);
      }
    }
  }

  void _finish(_Phase phase, bridge.ArcaPairErrorKind? failure) {
    _running = false;
    if (!mounted) return;
    setState(() {
      _phase = phase;
      _failure = failure;
      _code = null;
    });
  }

  Future<void> _confirm() async {
    final typed = _fingerprint.text;
    setState(() => _phase = _Phase.waiting);
    try {
      await _backend.answer(typed);
    } on bridge.ArcaPairFailure catch (failure) {
      _finish(_Phase.failed, failure.kind);
    }
  }

  @override
  Widget build(BuildContext context) {
    final l10n = AppLocalizations.of(context);
    return Scaffold(
      appBar: AppBar(title: Text(l10n.arcaPairTitle)),
      body: ListView(
        padding: const EdgeInsets.all(16),
        children: [
          Text(l10n.arcaPairIntro),
          const SizedBox(height: 16),
          ..._body(l10n),
        ],
      ),
    );
  }

  List<Widget> _body(AppLocalizations l10n) {
    final startButton = FilledButton(key: const Key('arca-pair-start'), onPressed: _start, child: Text(l10n.arcaPairStart));
    final cancelButton = OutlinedButton(key: const Key('arca-pair-cancel'), onPressed: () => _backend.cancel(), child: Text(l10n.arcaPairCancel));
    switch (_phase) {
      case _Phase.loading:
        return const [Center(child: CircularProgressIndicator())];
      case _Phase.noHub:
        return [Text(l10n.arcaPairNoHub)];
      case _Phase.ready:
        return [startButton];
      case _Phase.waiting:
        return [
          if (_code != null) ...[
            Text(l10n.arcaPairShowCode),
            const SizedBox(height: 8),
            SelectableText(_code!, style: Theme.of(context).textTheme.headlineSmall),
            const SizedBox(height: 16),
          ],
          Text(l10n.arcaPairWaiting),
          const SizedBox(height: 8),
          const LinearProgressIndicator(),
          const SizedBox(height: 16),
          cancelButton,
        ];
      case _Phase.asking:
        return [
          Text(l10n.arcaPairAskFingerprint),
          const SizedBox(height: 8),
          TextField(
            key: const Key('arca-pair-fingerprint'),
            controller: _fingerprint,
            decoration: InputDecoration(labelText: l10n.arcaPairFingerprintLabel, border: const OutlineInputBorder()),
            inputFormatters: [TypedOnlyFormatter()],
            // Nothing that could bring the fingerprint in from outside: no paste menu, no suggestions, no autofill.
            enableSuggestions: false,
            autocorrect: false,
            enableInteractiveSelection: false,
            contextMenuBuilder: (context, editableTextState) => const SizedBox.shrink(),
          ),
          const SizedBox(height: 16),
          FilledButton(key: const Key('arca-pair-confirm'), onPressed: _confirm, child: Text(l10n.arcaPairConfirm)),
          const SizedBox(height: 8),
          cancelButton,
        ];
      case _Phase.done:
        return [Text(l10n.arcaPairDone)];
      case _Phase.cancelled:
        return [Text(l10n.arcaPairCancelled), const SizedBox(height: 16), startButton];
      case _Phase.failed:
        return [
          Text(arcaPairErrorText(l10n, _failure ?? bridge.ArcaPairErrorKind.hub), style: TextStyle(color: Theme.of(context).colorScheme.error)),
          const SizedBox(height: 16),
          startButton,
        ];
    }
  }
}
