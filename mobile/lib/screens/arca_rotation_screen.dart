import 'dart:async';

import 'package:flutter/material.dart';

import '../l10n/generated/app_localizations.dart';
import '../services/arca_mode_flow.dart';
import '../services/arca_pair_flow.dart' show TypedOnlyFormatter;
import '../services/secure_window.dart';
import '../src/rust/api/arca_rotation.dart' as bridge_rotation;

/// The key rotation for "I lost a device" (P-49): see the plan in `project/PENDING.md`. Pops `true` once the rotation was confirmed.
///
/// The new key lives in the bridge, never here: this screen only holds the 24 words it was shown (in memory, never written) until the
/// person confirms, cancels or leaves. Every way out drops the key in the bridge. Nothing here can paste: the words are typed.
class ArcaRotationScreen extends StatefulWidget {
  const ArcaRotationScreen({super.key, required this.backend, this.secureWindow = const PlatformSecureWindow()});

  final ArcaModeBackend backend;

  /// Blocks screenshots and the recent-apps thumbnail while the 24 words are showing.
  final SecureWindow secureWindow;

  @override
  State<ArcaRotationScreen> createState() => _ArcaRotationScreenState();
}

enum _Step { idle, showing, resuming }

class _ArcaRotationScreenState extends State<ArcaRotationScreen> with WidgetsBindingObserver {
  _Step _step = _Step.idle;

  /// Set synchronously before the first await of an action: a second tap meanwhile finds it and does nothing.
  bool _busy = false;
  bool _asking = false;

  /// The 24 words and which of them (1-based) are typed back. Only while [_step] is `showing`.
  List<String> _phrase = const [];
  List<int> _positions = const [];

  final _wordControllers = List.generate(3, (_) => TextEditingController());
  final _resumeController = TextEditingController();
  bool _revoked = false;
  String? _error;

  @override
  void initState() {
    super.initState();
    WidgetsBinding.instance.addObserver(this);
  }

  @override
  void dispose() {
    WidgetsBinding.instance.removeObserver(this);
    for (final controller in _wordControllers) {
      controller.dispose();
    }
    _resumeController.dispose();
    // Whatever way out (confirm, back, the screen replaced): the window may be captured again. Not before: in the background it stays protected.
    if (_step == _Step.showing) unawaited(widget.secureWindow.disable());
    // Leaving with the phrase showing (or half way) must not leave a key waiting in the bridge. After a confirm there is nothing left to drop.
    unawaited(widget.backend.rotationCancel().catchError((_) {}));
    super.dispose();
  }

  /// In the background the window stays protected: that is what keeps the words out of the recent-apps thumbnail. Coming back, the flag is
  /// asked again, in case the window was rebuilt meanwhile (asking is idempotent).
  @override
  void didChangeAppLifecycleState(AppLifecycleState state) {
    if (state == AppLifecycleState.resumed && _step == _Step.showing) unawaited(widget.secureWindow.enable());
  }

  bridge_rotation.ArcaRotationErrorKind _kindOf(Object error) => error is bridge_rotation.ArcaRotationFailure ? error.kind : bridge_rotation.ArcaRotationErrorKind.other;

  /// Forgets the phrase and everything typed. The state of the screen after a dropped key.
  void _wipe() {
    // The words are gone from the screen, so the window may be captured again.
    if (_step == _Step.showing) unawaited(widget.secureWindow.disable());
    _phrase = const [];
    _positions = const [];
    for (final controller in _wordControllers) {
      controller.clear();
    }
    _resumeController.clear();
    _revoked = false;
  }

  bool get _canConfirm => _revoked && _wordControllers.every((c) => c.text.trim().isNotEmpty);

  bool get _canResume => _revoked && _resumeController.text.trim().isNotEmpty;

  Future<void> _begin() async {
    if (_busy) return;
    _busy = true;
    final l10n = AppLocalizations.of(context);
    setState(() => _error = null);
    try {
      final start = await widget.backend.rotationBegin();
      // The window is protected before the first word can be drawn. `enable` never throws: a window that cannot be protected is no reason
      // to stop, and the screen carries its own warning.
      await widget.secureWindow.enable();
      if (!mounted) {
        // Left while asking: `dispose` saw no phrase showing and released nothing, so the flag taken just now is released here.
        unawaited(widget.secureWindow.disable());
        return;
      }
      setState(() {
        _phrase = start.phrase;
        _positions = start.positions.toList();
        _step = _Step.showing;
      });
    } catch (error) {
      if (mounted) setState(() => _error = arcaRotationErrorText(l10n, _kindOf(error)));
    } finally {
      _busy = false;
      if (mounted) setState(() {});
    }
  }

  void _openResume() {
    if (_busy) return;
    setState(() {
      _error = null;
      _wipe();
      _step = _Step.resuming;
    });
  }

  Future<void> _cancel() async {
    if (_busy) return;
    setState(() {
      _error = null;
      _wipe();
      _step = _Step.idle;
    });
    try {
      await widget.backend.rotationCancel();
    } catch (_) {
      // Nothing was written, and the bridge drops it again when this screen closes.
    }
  }

  /// Only an explicit "yes" goes on: the dialog cannot be dismissed by tapping outside, and the back button answers `null`.
  Future<bool> _askToConfirm(AppLocalizations l10n) async {
    if (_busy || _asking) return false;
    _asking = true;
    try {
      final yes = await showDialog<bool>(
        context: context,
        barrierDismissible: false,
        builder: (context) => AlertDialog(
          title: Text(l10n.arcaRotationConfirmTitle),
          content: Text(l10n.arcaRotationConfirmBody),
          actions: [
            TextButton(key: const Key('arca-rotation-confirm-no'), onPressed: () => Navigator.of(context).pop(false), child: Text(l10n.arcaRotationConfirmNo)),
            FilledButton(key: const Key('arca-rotation-confirm-yes'), onPressed: () => Navigator.of(context).pop(true), child: Text(l10n.arcaRotationConfirmYes)),
          ],
        ),
      );
      return yes == true && mounted;
    } finally {
      _asking = false;
    }
  }

  Future<void> _confirm() async {
    final l10n = AppLocalizations.of(context);
    if (!_canConfirm || !await _askToConfirm(l10n)) return;
    _busy = true;
    setState(() => _error = null);
    try {
      await widget.backend.rotationConfirm(typedWords: [for (final c in _wordControllers) c.text.trim()], revokedOnHub: _revoked);
    } catch (error) {
      // The bridge took the key out when it started: whatever the answer, that phrase is spent. `failed` may have written something,
      // and so may an error that is not the bridge's own (a panic, a broken channel: the bridge never answers `other` to a confirm).
      // Then the way on is the same 24 words typed again, never a new key.
      final kind = error is bridge_rotation.ArcaRotationFailure && error.kind != bridge_rotation.ArcaRotationErrorKind.other ? error.kind : bridge_rotation.ArcaRotationErrorKind.failed;
      if (!mounted) return;
      setState(() {
        _wipe();
        _error = arcaRotationErrorText(l10n, kind);
        // A stop (`failed`, or an emptied class that the same 24 words finish once the files are back) goes on in "resume".
        _step = kind == bridge_rotation.ArcaRotationErrorKind.failed || kind == bridge_rotation.ArcaRotationErrorKind.classEmptied ? _Step.resuming : _Step.idle;
        _busy = false;
      });
      return;
    }
    _busy = false;
    if (mounted) Navigator.of(context).pop(true);
  }

  Future<void> _resume() async {
    final l10n = AppLocalizations.of(context);
    if (!_canResume || !await _askToConfirm(l10n)) return;
    _busy = true;
    setState(() => _error = null);
    final phrase = _resumeController.text.trim().split(RegExp(r'\s+')).join(' ');
    try {
      await widget.backend.rotationResume(phrase);
    } catch (error) {
      // `resume` writes nothing: here `other` really is "nothing was changed".
      if (mounted) {
        setState(() {
          _error = arcaRotationErrorText(l10n, _kindOf(error), resuming: true);
          _busy = false;
        });
      }
      return;
    }
    try {
      await widget.backend.rotationConfirm(typedWords: const [], revokedOnHub: _revoked);
    } catch (error) {
      // As in `_confirm`: the bridge never answers `other` to a confirm, so `other` or an error that is not the bridge's own (a panic,
      // a broken channel) is the unknown, and the unknown may have written something. The 24 words stay to be tried again.
      final kind = error is bridge_rotation.ArcaRotationFailure && error.kind != bridge_rotation.ArcaRotationErrorKind.other ? error.kind : bridge_rotation.ArcaRotationErrorKind.failed;
      if (mounted) {
        setState(() {
          // `resuming`: an earlier attempt may already have published this key, so the text says nothing was written by THIS one.
          _error = arcaRotationErrorText(l10n, kind, resuming: true);
          _busy = false;
        });
      }
      return;
    }
    _busy = false;
    if (mounted) Navigator.of(context).pop(true);
  }

  Widget _revokedTile(AppLocalizations l10n) => CheckboxListTile(
        key: const Key('arca-rotation-revoked'),
        contentPadding: EdgeInsets.zero,
        controlAffinity: ListTileControlAffinity.leading,
        value: _revoked,
        onChanged: _busy ? null : (value) => setState(() => _revoked = value ?? false),
        title: Text(l10n.arcaRotationRevoked),
      );

  /// A field where the person types what a screen or a paper shows: nothing that could bring text in from outside.
  Widget _typedField({required Key key, required TextEditingController controller, required String label, int? maxLines = 1}) => TextField(
        key: key,
        controller: controller,
        decoration: InputDecoration(labelText: label, border: const OutlineInputBorder()),
        inputFormatters: [TypedOnlyFormatter()],
        enableSuggestions: false,
        autocorrect: false,
        enableInteractiveSelection: false,
        enableIMEPersonalizedLearning: false,
        maxLines: maxLines,
        contextMenuBuilder: (context, editableTextState) => const SizedBox.shrink(),
        onChanged: (_) => setState(() {}),
      );

  List<Widget> _idle(AppLocalizations l10n) => [
        Text(l10n.arcaRotationIntro),
        const SizedBox(height: 16),
        FilledButton(key: const Key('arca-rotation-begin'), onPressed: _busy ? null : _begin, child: Text(l10n.arcaRotationBegin)),
        const SizedBox(height: 8),
        OutlinedButton(key: const Key('arca-rotation-resume-open'), onPressed: _busy ? null : _openResume, child: Text(l10n.arcaRotationResumeOpen)),
      ];

  List<Widget> _showing(AppLocalizations l10n) => [
        // First, and a live region: TalkBack announces it the moment the words appear, before it reads anything else.
        Semantics(
          container: true,
          liveRegion: true,
          label: l10n.arcaRotationCaptureWarning,
          excludeSemantics: true,
          child: Text(l10n.arcaRotationCaptureWarning, key: const Key('arca-rotation-capture-warning'), style: TextStyle(color: Theme.of(context).colorScheme.error)),
        ),
        const SizedBox(height: 8),
        Text(l10n.arcaRotationPhraseIntro),
        const SizedBox(height: 16),
        // One stop per word, read as "Word 3: able": the position belongs to the word, and the order is the phrase's (top to bottom, left to right).
        Wrap(spacing: 16, runSpacing: 4, children: [
          for (var i = 0; i < _phrase.length; i++)
            Semantics(
              key: Key('arca-rotation-phrase-${i + 1}'),
              container: true,
              label: '${l10n.arcaRotationWordLabel(i + 1)}: ${_phrase[i]}',
              excludeSemantics: true,
              child: Text('${i + 1}. ${_phrase[i]}'),
            ),
        ]),
        const SizedBox(height: 16),
        Text(l10n.arcaRotationTypeBack),
        const SizedBox(height: 8),
        for (var i = 0; i < _positions.length && i < _wordControllers.length; i++) ...[
          _typedField(key: Key('arca-rotation-word-$i'), controller: _wordControllers[i], label: l10n.arcaRotationWordLabel(_positions[i])),
          const SizedBox(height: 8),
        ],
        _revokedTile(l10n),
        const SizedBox(height: 8),
        FilledButton(key: const Key('arca-rotation-confirm'), onPressed: _busy || !_canConfirm ? null : _confirm, child: Text(l10n.arcaRotationConfirm)),
        const SizedBox(height: 8),
        OutlinedButton(key: const Key('arca-rotation-cancel'), onPressed: _busy ? null : _cancel, child: Text(l10n.arcaRotationCancel)),
      ];

  List<Widget> _resuming(AppLocalizations l10n) => [
        Text(l10n.arcaRotationResumeIntro),
        const SizedBox(height: 8),
        _typedField(key: const Key('arca-rotation-resume-field'), controller: _resumeController, label: l10n.arcaRotationResumeLabel, maxLines: null),
        const SizedBox(height: 8),
        _revokedTile(l10n),
        const SizedBox(height: 8),
        FilledButton(key: const Key('arca-rotation-resume-confirm'), onPressed: _busy || !_canResume ? null : _resume, child: Text(l10n.arcaRotationResumeConfirm)),
        const SizedBox(height: 8),
        OutlinedButton(key: const Key('arca-rotation-cancel'), onPressed: _busy ? null : _cancel, child: Text(l10n.arcaRotationCancel)),
      ];

  @override
  Widget build(BuildContext context) {
    final l10n = AppLocalizations.of(context);
    // Not while a confirm or a resume is in flight: leaving would drop the screen that must hand `true` back so the new epoch is sent.
    return PopScope(
      canPop: !_busy,
      child: Scaffold(
      appBar: AppBar(title: Text(l10n.arcaRotationTitle)),
      body: ListView(
        padding: const EdgeInsets.all(16),
        children: [
          if (_error != null) ...[
            Text(_error!, style: TextStyle(color: Theme.of(context).colorScheme.error)),
            const SizedBox(height: 16),
          ],
          ...switch (_step) {
            _Step.idle => _idle(l10n),
            _Step.showing => _showing(l10n),
            _Step.resuming => _resuming(l10n),
          },
          if (_busy) ...[const SizedBox(height: 16), const LinearProgressIndicator()],
        ],
      ),
      ),
    );
  }
}
