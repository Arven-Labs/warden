import 'dart:async';
import 'dart:typed_data';

import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:mobile/l10n/generated/app_localizations_en.dart';
import 'package:mobile/l10n/generated/app_localizations_pt.dart';
import 'package:mobile/screens/arca_rotation_screen.dart';
import 'package:mobile/services/arca_mode_flow.dart';
import 'package:mobile/services/secure_window.dart';
import 'package:mobile/src/rust/api/arca_mode.dart' as bridge;
import 'package:mobile/src/rust/api/arca_rotation.dart' as bridge_rot;

import '../helpers/test_app.dart';

/// A real-looking phrase (not `word01` .. `word24`, the shape the neighbouring tests use): a label that only matched that shape would show here.
const _phrase = [
  'abandon', 'ability', 'able', 'about', 'above', 'absent', 'absorb', 'abstract', 'absurd', 'abuse', 'access', 'accident', //
  'account', 'accuse', 'achieve', 'acid', 'acoustic', 'acquire', 'across', 'act', 'action', 'actor', 'actress', 'actual',
];

/// The positions (from 1) the person is asked to type back: `ability`, `absurd`, `acoustic`.
const _positions = [2, 9, 17];

/// The bridge, as far as this screen goes: answers the phrase above and records what was asked of it.
class _Backend implements ArcaModeBackend {
  bridge_rot.ArcaRotationFailure? beginFailure;
  bridge_rot.ArcaRotationFailure? confirmFailure;
  int cancelCalls = 0;

  @override
  Future<bridge_rot.RotationStartDto> rotationBegin() async {
    final failure = beginFailure;
    if (failure != null) throw failure;
    return bridge_rot.RotationStartDto(phrase: _phrase, positions: Uint32List.fromList(_positions));
  }

  @override
  Future<void> rotationConfirm({required List<String> typedWords, required bool revokedOnHub}) async {
    final failure = confirmFailure;
    if (failure != null) throw failure;
  }

  @override
  Future<void> rotationResume(String phrase) async {}

  @override
  Future<void> rotationCancel() async => cancelCalls++;

  @override
  Future<bridge.ArcaModeStatusDto> status() => throw UnimplementedError();

  @override
  Future<void> enable() => throw UnimplementedError();

  @override
  Future<bridge.ArcaSyncDto> syncRun({required String hubUrl, required String deviceToken, required bool allowInsecureHttp}) => throw UnimplementedError();

  @override
  Future<List<bridge.PendingEpochDto>> epochsPending() => throw UnimplementedError();

  @override
  Future<int> epochsAcknowledge(List<bridge.PendingEpochDto> seen) => throw UnimplementedError();
}

/// Records what the screen asks of the window. [enableGate] holds `enable` until the test lets it go.
class _FakeSecureWindow implements SecureWindow {
  final calls = <String>[];
  Completer<void>? enableGate;

  /// The window is protected when the last thing asked was `enable`.
  bool get protected => calls.isNotEmpty && calls.last == 'enable';

  @override
  Future<void> enable() async {
    calls.add('enable');
    final gate = enableGate;
    if (gate != null) await gate.future;
  }

  @override
  Future<void> disable() async => calls.add('disable');
}

bridge_rot.ArcaRotationFailure _failure(bridge_rot.ArcaRotationErrorKind kind) => bridge_rot.ArcaRotationFailure(kind: kind, detail: 'english detail SECRET');

// Not `pumpAndSettle`: a spinner never settles. Longer than a single pump: a route has to finish its transition.
Future<void> _settle(WidgetTester tester) async {
  for (var i = 0; i < 10; i++) {
    await tester.pump(const Duration(milliseconds: 100));
  }
}

Future<void> _tap(WidgetTester tester, String key) async {
  await tester.tap(find.byKey(Key(key)));
  await _settle(tester);
}

// The person types: a character at a time (the fields refuse a block that arrives at once).
Future<void> _typeInto(WidgetTester tester, String key, String text) async {
  for (var i = 1; i <= text.length; i++) {
    await tester.enterText(find.byKey(Key(key)), text.substring(0, i));
  }
  await tester.pump();
}

// The framework only accepts the transitions Android makes: resumed > inactive > hidden > paused, and the way back through the same steps.
void _goToBackground(WidgetTester tester) {
  for (final state in [AppLifecycleState.inactive, AppLifecycleState.hidden, AppLifecycleState.paused]) {
    tester.binding.handleAppLifecycleStateChanged(state);
  }
}

void _backToForeground(WidgetTester tester) {
  for (final state in [AppLifecycleState.hidden, AppLifecycleState.inactive, AppLifecycleState.resumed]) {
    tester.binding.handleAppLifecycleStateChanged(state);
  }
}

/// Opens the screen the way the app does, on top of another route, so that leaving it is a real pop.
Future<void> _open(WidgetTester tester, _Backend backend, SecureWindow? window, {Locale locale = const Locale('en')}) async {
  // An app left over from an earlier call in the same test would keep its pushed route: start from nothing.
  await tester.pumpWidget(const SizedBox());
  // The phrase and the three fields are taller than the default test window: a tall one keeps everything built and tappable.
  tester.view.physicalSize = const Size(800, 3200);
  tester.view.devicePixelRatio = 1;
  addTearDown(tester.view.reset);
  await tester.pumpWidget(testApp(
    locale: locale,
    home: Builder(
      builder: (context) => TextButton(
        key: const Key('open-rotation'),
        onPressed: () => Navigator.of(context).push(MaterialPageRoute<bool>(
          builder: (_) => window == null ? ArcaRotationScreen(backend: backend) : ArcaRotationScreen(backend: backend, secureWindow: window),
        )),
        child: const Text('open'),
      ),
    ),
  ));
  await _tap(tester, 'open-rotation');
}

Future<void> _fillAndDeclare(WidgetTester tester) async {
  for (var i = 0; i < _positions.length; i++) {
    await _typeInto(tester, 'arca-rotation-word-$i', _phrase[_positions[i] - 1]);
  }
  await _tap(tester, 'arca-rotation-revoked');
}

void main() {
  final en = AppLocalizationsEn();
  final pt = AppLocalizationsPt();

  group('the window is protected while the 24 words are showing (P-49)', () {
    testWidgets('opening the screen asks for nothing: there is no phrase yet', (tester) async {
      final window = _FakeSecureWindow();

      await _open(tester, _Backend(), window);

      expect(find.byKey(const Key('arca-rotation-begin')), findsOneWidget, reason: 'the idle step is what is on screen');
      expect(window.calls, isEmpty);
    });

    testWidgets('begin protects the window, and only then shows the words', (tester) async {
      final window = _FakeSecureWindow()..enableGate = Completer<void>();
      await _open(tester, _Backend(), window);

      await tester.tap(find.byKey(const Key('arca-rotation-begin')));
      await _settle(tester);

      expect(window.calls, ['enable'], reason: 'asked as soon as the bridge answered');
      expect(find.text('1. abandon'), findsNothing, reason: 'not one word on screen while the window can still be captured');

      window.enableGate!.complete();
      await _settle(tester);

      expect(find.text('1. abandon'), findsOneWidget);
      expect(window.calls, ['enable'], reason: 'asked once');
      expect(window.protected, isTrue);
    });

    testWidgets('a refusal to begin shows no phrase and does not touch the window', (tester) async {
      final window = _FakeSecureWindow();
      final backend = _Backend()..beginFailure = _failure(bridge_rot.ArcaRotationErrorKind.notInArcaMode);
      await _open(tester, backend, window);

      await _tap(tester, 'arca-rotation-begin');

      expect(find.text('1. abandon'), findsNothing);
      expect(window.calls, isEmpty);
    });

    testWidgets('cancel lets the window be captured again, and the words are gone', (tester) async {
      final window = _FakeSecureWindow();
      await _open(tester, _Backend(), window);
      await _tap(tester, 'arca-rotation-begin');
      expect(window.protected, isTrue, reason: 'premise: the words are showing and the window is protected');
      expect(find.text('1. abandon'), findsOneWidget);

      await _tap(tester, 'arca-rotation-cancel');

      expect(window.calls, ['enable', 'disable']);
      expect(window.protected, isFalse);
      expect(find.text('1. abandon'), findsNothing);
      expect(find.byKey(const Key('arca-rotation-begin')), findsOneWidget, reason: 'back to the idle step');
    });

    testWidgets('going back with the words showing lets the window be captured again', (tester) async {
      final window = _FakeSecureWindow();
      final backend = _Backend();
      await _open(tester, backend, window);
      await _tap(tester, 'arca-rotation-begin');
      expect(window.protected, isTrue);

      await tester.state<NavigatorState>(find.byType(Navigator).first).maybePop();
      await _settle(tester);

      expect(find.byType(ArcaRotationScreen), findsNothing, reason: 'premise: the screen was left');
      expect(window.calls, ['enable', 'disable']);
      expect(window.protected, isFalse);
      expect(backend.cancelCalls, 1, reason: 'the key waiting in the bridge is dropped too');
    });

    testWidgets('the screen disappearing for any other reason (dispose) lets the window be captured again', (tester) async {
      final window = _FakeSecureWindow();
      await tester.pumpWidget(testApp(home: ArcaRotationScreen(backend: _Backend(), secureWindow: window)));
      await _tap(tester, 'arca-rotation-begin');
      expect(window.protected, isTrue);

      await tester.pumpWidget(testApp(home: const SizedBox()));

      expect(window.calls, ['enable', 'disable']);
    });

    testWidgets('a confirmed rotation lets the window be captured again', (tester) async {
      final window = _FakeSecureWindow();
      await _open(tester, _Backend(), window);
      await _tap(tester, 'arca-rotation-begin');
      await _fillAndDeclare(tester);

      await _tap(tester, 'arca-rotation-confirm');
      expect(window.protected, isTrue, reason: 'still asking "are you sure?": the words are still behind the dialog');
      await _tap(tester, 'arca-rotation-confirm-yes');

      expect(find.byType(ArcaRotationScreen), findsNothing, reason: 'premise: the confirm went through and the screen was left');
      expect(window.calls, ['enable', 'disable']);
      expect(window.protected, isFalse);
    });

    testWidgets('a confirm that fails drops the phrase, and the window is released with it, wherever the screen goes next', (tester) async {
      for (final (kind, nextKey) in [
        (bridge_rot.ArcaRotationErrorKind.wordMismatch, 'arca-rotation-begin'),
        (bridge_rot.ArcaRotationErrorKind.failed, 'arca-rotation-resume-field'),
      ]) {
        final window = _FakeSecureWindow();
        final backend = _Backend()..confirmFailure = _failure(kind);
        await _open(tester, backend, window);
        await _tap(tester, 'arca-rotation-begin');
        await _fillAndDeclare(tester);

        await _tap(tester, 'arca-rotation-confirm');
        await _tap(tester, 'arca-rotation-confirm-yes');

        expect(find.byKey(Key(nextKey)), findsOneWidget, reason: '$kind: premise, the screen went to the step without the phrase');
        expect(find.text('1. abandon'), findsNothing, reason: '$kind');
        expect(window.calls, ['enable', 'disable'], reason: '$kind');
        expect(window.protected, isFalse, reason: '$kind');
      }
    });

    testWidgets('the steps without the phrase never ask for it', (tester) async {
      final window = _FakeSecureWindow();
      await _open(tester, _Backend(), window);

      await _tap(tester, 'arca-rotation-resume-open');

      expect(find.byKey(const Key('arca-rotation-resume-field')), findsOneWidget, reason: 'premise: the resume step is on screen');
      expect(window.calls, isEmpty);
    });

    testWidgets('going to the background keeps the window protected (the recents thumbnail), and coming back asks again', (tester) async {
      final window = _FakeSecureWindow();
      await _open(tester, _Backend(), window);
      await _tap(tester, 'arca-rotation-begin');
      expect(window.calls, ['enable']);

      _goToBackground(tester);
      await tester.pump();

      expect(window.calls, ['enable'], reason: 'released here, the recents thumbnail would show the 24 words');
      expect(window.protected, isTrue);

      _backToForeground(tester);
      await tester.pump();

      expect(window.calls, ['enable', 'enable'], reason: 'the window may have been rebuilt meanwhile: the flag is asked again');
      expect(window.protected, isTrue);
    });

    testWidgets('coming back to the front without the phrase asks for nothing', (tester) async {
      final window = _FakeSecureWindow();
      await _open(tester, _Backend(), window);

      _goToBackground(tester);
      _backToForeground(tester);
      await tester.pump();

      expect(window.calls, isEmpty);
    });

    testWidgets('where nothing answers the channel (iOS, desktop) the words still show and nothing breaks', (tester) async {
      // No window handed over: the screen uses the real service. A handler that throws MissingPluginException is answered with null,
      // which is what a platform with no plugin on the channel does.
      final messenger = TestDefaultBinaryMessengerBinding.instance.defaultBinaryMessenger;
      const channel = MethodChannel(secureWindowChannelName);
      final asked = <String>[];
      messenger.setMockMethodCallHandler(channel, (call) async {
        asked.add(call.method);
        throw MissingPluginException();
      });
      addTearDown(() => messenger.setMockMethodCallHandler(channel, null));
      await _open(tester, _Backend(), null);

      await _tap(tester, 'arca-rotation-begin');

      expect(asked, ['enable'], reason: 'premise: the request did reach the channel, and was refused there');
      expect(find.text('1. abandon'), findsOneWidget);
      expect(find.byKey(const Key('arca-rotation-confirm')), findsOneWidget);
      expect(tester.takeException(), isNull);

      await _tap(tester, 'arca-rotation-cancel');

      expect(asked, ['enable', 'disable']);
      expect(find.byKey(const Key('arca-rotation-begin')), findsOneWidget);
      expect(tester.takeException(), isNull);
    });
  });

  group('TalkBack on the screen with the 24 words (P-49)', () {
    String labelOf(WidgetTester tester, Finder finder) => tester.getSemantics(finder).label;

    testWidgets('each word is read with its position and nothing else, in English and in Portuguese', (tester) async {
      final handle = tester.ensureSemantics();
      for (final (locale, words) in [(const Locale('en'), en), (const Locale('pt'), pt)]) {
        await _open(tester, _Backend(), _FakeSecureWindow(), locale: locale);
        await _tap(tester, 'arca-rotation-begin');

        for (var i = 1; i <= _phrase.length; i++) {
          expect(labelOf(tester, find.text('$i. ${_phrase[i - 1]}')), '${words.arcaRotationWordLabel(i)}: ${_phrase[i - 1]}', reason: '$locale, word $i');
        }
        // The one the neighbouring tests would call `word03`: here it is a real word, and the label carries the word and the position.
        expect(labelOf(tester, find.text('3. able')), '${words.arcaRotationWordLabel(3)}: able', reason: '$locale');
        expect(find.bySemanticsLabel('${words.arcaRotationWordLabel(3)}: able'), findsOneWidget, reason: '$locale');
      }
      expect(en.arcaRotationWordLabel(3), 'Word 3', reason: 'the text the label is built from (app_en.arb)');
      // Disposed here, not in a tear-down: the framework checks for open handles before the tear-downs run.
      handle.dispose();
    });

    testWidgets('the warning comes before the instructions and the words, and is announced when it appears', (tester) async {
      final handle = tester.ensureSemantics();
      await _open(tester, _Backend(), _FakeSecureWindow());
      await _tap(tester, 'arca-rotation-begin');
      final warning = find.byKey(const Key('arca-rotation-capture-warning'));

      expect(labelOf(tester, warning), en.arcaRotationCaptureWarning);
      // `containsSemantics` is deprecated in favour of `isSemantics`, whose parameters could not be checked here (no Flutter on this machine).
      // ignore: deprecated_member_use
      expect(tester.getSemantics(warning), containsSemantics(isLiveRegion: true), reason: 'TalkBack reads it when it shows up, before anything else');
      final warningTop = tester.getTopLeft(warning).dy;
      expect(warningTop, lessThan(tester.getTopLeft(find.text(en.arcaRotationPhraseIntro)).dy), reason: 'read top to bottom: the warning is the first thing');
      expect(warningTop, lessThan(tester.getTopLeft(find.text('1. abandon')).dy));
      handle.dispose();
    });

    testWidgets('the words are read in the order of the phrase', (tester) async {
      await _open(tester, _Backend(), _FakeSecureWindow());
      await _tap(tester, 'arca-rotation-begin');

      Offset at(int position) => tester.getTopLeft(find.text('$position. ${_phrase[position - 1]}'));
      for (var position = 1; position < _phrase.length; position++) {
        final (a, b) = (at(position), at(position + 1));
        expect(a.dy < b.dy || (a.dy == b.dy && a.dx < b.dx), isTrue, reason: 'word $position is read before word ${position + 1}');
      }
    });

    testWidgets('the fields and the buttons are named by what they do', (tester) async {
      final handle = tester.ensureSemantics();
      await _open(tester, _Backend(), _FakeSecureWindow());
      await _tap(tester, 'arca-rotation-begin');

      for (var i = 0; i < _positions.length; i++) {
        expect(labelOf(tester, find.byKey(Key('arca-rotation-word-$i'))), contains(en.arcaRotationWordLabel(_positions[i])), reason: 'field $i asks for word ${_positions[i]}');
      }
      expect(labelOf(tester, find.byKey(const Key('arca-rotation-revoked'))), contains(en.arcaRotationRevoked));
      expect(labelOf(tester, find.byKey(const Key('arca-rotation-confirm'))), en.arcaRotationConfirm);
      expect(labelOf(tester, find.byKey(const Key('arca-rotation-cancel'))), en.arcaRotationCancel);
      handle.dispose();
    });
  });
}
