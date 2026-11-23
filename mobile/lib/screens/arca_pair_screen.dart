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
  const ArcaPairScreen({super.key, this.backend, this.swap});

  /// Tests pass a fake; the app uses the Rust bridge.
  final ArcaPairBackend? backend;

  /// Set when this is the key swap (P-49) and not the first pairing: which half this device plays.
  final ArcaSwapSide? swap;

  @override
  State<ArcaPairScreen> createState() => _ArcaPairScreenState();
}

enum _Phase { loading, noHub, ready, waiting, asking, choosing, askingCode, done, cancelled, failed }

class _ArcaPairScreenState extends State<ArcaPairScreen> {
  late final ArcaPairBackend _backend = widget.backend ?? BridgeArcaPairBackend();
  /// What the person types in the current step: the fingerprint (new device) or the offer code (the device that grants).
  final _typed = TextEditingController();

  _Phase _phase = _Phase.loading;
  bridge.ArcaPairRole _role = bridge.ArcaPairRole.offer;
  List<String> _offers = const [];
  String? _grantedFingerprint;
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
    _typed.dispose();
    super.dispose();
  }

  Future<void> _load() async {
    final store = ConnectionSettingsStore();
    final settings = await store.load();
    final token = settings == null ? null : await store.deviceTokenFor(settings.host, settings.port);
    final deviceId = await store.getOrCreateDeviceId();
    // In a swap both ends already have a key, so the key cannot say which half this is: the person said it.
    final role = widget.swap == null ? await _backend.role() : _role;
    if (!mounted) return;
    setState(() {
      _role = role;
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
      _offers = const [];
      _grantedFingerprint = null;
      _typed.clear();
    });
    try {
      if (widget.swap == ArcaSwapSide.grant) {
        await _backend.startSwapGrant(hubUrl: _hubUrl!, deviceToken: _token!);
      } else if (widget.swap == ArcaSwapSide.replace) {
        await _backend.startSwapReplace(hubUrl: _hubUrl!, deviceToken: _token!, deviceId: _deviceId!);
      } else if (_role == bridge.ArcaPairRole.grant) {
        await _backend.startGrant(hubUrl: _hubUrl!, deviceToken: _token!);
      } else {
        await _backend.start(hubUrl: _hubUrl!, deviceToken: _token!, deviceId: _deviceId!);
      }
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
        case bridge.ArcaPairStepKind.chooseOffer:
          setState(() {
            _offers = step.offers;
            _phase = _Phase.choosing;
          });
        case bridge.ArcaPairStepKind.askOfferCode:
          setState(() => _phase = _Phase.askingCode);
        case bridge.ArcaPairStepKind.showFingerprint:
          // The flow ends right after this (`done`), so the screen keeps the fingerprint for the person to type on the new device.
          setState(() => _grantedFingerprint = step.fingerprint);
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

  Future<void> _pick(int index) async {
    setState(() => _phase = _Phase.waiting);
    try {
      await _backend.pick(index);
    } on bridge.ArcaPairFailure catch (failure) {
      _finish(_Phase.failed, failure.kind);
    }
  }

  Future<void> _confirm() async {
    final typed = _typed.text;
    _typed.clear();
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
    final swap = widget.swap;
    final intro = switch (swap) {
      ArcaSwapSide.grant => l10n.arcaSwapGrantIntro,
      ArcaSwapSide.replace => l10n.arcaSwapReplaceIntro,
      null => _role == bridge.ArcaPairRole.grant ? l10n.arcaPairGrantIntro : l10n.arcaPairIntro,
    };
    return Scaffold(
      appBar: AppBar(title: Text(swap == null ? l10n.arcaPairTitle : l10n.arcaSwapTitle)),
      body: ListView(
        padding: const EdgeInsets.all(16),
        children: [
          Text(intro),
          const SizedBox(height: 16),
          ..._body(l10n),
        ],
      ),
    );
  }

  /// A step where the person types something the OTHER device shows (the fingerprint on the new device, the offer code on the one
  /// that grants). Nothing that could bring it in from outside: no paste menu, no suggestions, no autofill.
  List<Widget> _typedStep(AppLocalizations l10n, Widget cancelButton, {required String prompt, required String fieldKey, required String label}) => [
        Text(prompt),
        const SizedBox(height: 8),
        TextField(
          key: Key(fieldKey),
          controller: _typed,
          decoration: InputDecoration(labelText: label, border: const OutlineInputBorder()),
          inputFormatters: widget.swap != null ? const <TextInputFormatter>[] : [TypedOnlyFormatter()],
          enableSuggestions: false,
          autocorrect: false,
          enableInteractiveSelection: false,
          // The keyboard must not learn a fingerprint (or a head code) either.
          enableIMEPersonalizedLearning: false,
          contextMenuBuilder: (context, editableTextState) => const SizedBox.shrink(),
        ),
        const SizedBox(height: 16),
        FilledButton(key: const Key('arca-pair-confirm'), onPressed: _confirm, child: Text(l10n.arcaPairConfirm)),
        const SizedBox(height: 8),
        cancelButton,
      ];

  List<Widget> _body(AppLocalizations l10n) {
    final startButton = FilledButton(key: const Key('arca-pair-start'), onPressed: _start, child: Text(l10n.arcaPairStart));
    final swap = widget.swap != null;
    // The replacing side wipes the local sync copy, and its confirmation (the epochs read at the tap) lives on the Arca mode screen:
    // after a failure or a cancel the way out is back to that screen, not a "start again" that would skip the confirmation.
    final replacing = widget.swap == ArcaSwapSide.replace;
    final restartButton = replacing
        ? OutlinedButton(key: const Key('arca-pair-back'), onPressed: () => Navigator.of(context).maybePop(), child: Text(l10n.arcaSwapBack))
        : startButton;
    final cancelButton = OutlinedButton(key: const Key('arca-pair-cancel'), onPressed: () => _backend.cancel(), child: Text(swap ? l10n.arcaSwapCancel : l10n.arcaPairCancel));
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
        return _typedStep(
          l10n,
          cancelButton,
          prompt: swap ? l10n.arcaSwapAskFingerprint : l10n.arcaPairAskFingerprint,
          fieldKey: 'arca-pair-fingerprint',
          label: swap ? l10n.arcaSwapFingerprintLabel : l10n.arcaPairFingerprintLabel,
        );
      case _Phase.choosing:
        return [
          Text(l10n.arcaPairChooseOffer),
          const SizedBox(height: 8),
          for (var i = 0; i < _offers.length; i++) ListTile(key: Key('arca-pair-offer-$i'), title: Text(_offers[i]), onTap: () => _pick(i)),
          const SizedBox(height: 16),
          cancelButton,
        ];
      case _Phase.askingCode:
        return _typedStep(l10n, cancelButton, prompt: l10n.arcaPairAskOfferCode, fieldKey: 'arca-pair-code', label: l10n.arcaPairOfferCodeLabel);
      case _Phase.done:
        final granted = _grantedFingerprint;
        return [
          if (granted == null) Text(swap ? l10n.arcaSwapDone : l10n.arcaPairDone) else ...[
            Text(swap ? l10n.arcaSwapGranted : l10n.arcaPairGranted),
            const SizedBox(height: 8),
            SelectableText(granted, style: Theme.of(context).textTheme.headlineSmall),
          ],
        ];
      case _Phase.cancelled:
        return [Text(l10n.arcaPairCancelled), const SizedBox(height: 16), restartButton];
      case _Phase.failed:
        final kind = _failure ?? bridge.ArcaPairErrorKind.hub;
        // "No device is waiting" means something else in a swap: the new device starts it on the other device (the swap's own words).
        final text = swap && kind == bridge.ArcaPairErrorKind.noOffers ? l10n.arcaSwapErrNoOffers : arcaPairErrorText(l10n, kind);
        return [
          Text(text, style: TextStyle(color: Theme.of(context).colorScheme.error)),
          const SizedBox(height: 16),
          restartButton,
        ];
    }
  }
}
