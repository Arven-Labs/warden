import 'dart:async';

import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:mobile/l10n/generated/app_localizations_en.dart';
import 'package:mobile/l10n/generated/app_localizations_pt.dart';
import 'package:mobile/screens/arca_pair_screen.dart';
import 'package:mobile/services/arca_pair_flow.dart';
import 'package:mobile/src/rust/api/arca_pair.dart' as bridge;
import 'package:shared_preferences/shared_preferences.dart';

import '../helpers/test_app.dart';

/// A stand-in for the Rust bridge: records what the screen asks and hands out the steps the test pushes.
class FakeBackend implements ArcaPairBackend {
  final started = <Map<String, String>>[];
  final answers = <String>[];
  int cancels = 0;
  final _pending = <bridge.ArcaPairStep>[];
  Completer<bridge.ArcaPairStep>? _waiting;

  void push(bridge.ArcaPairStep step) {
    final waiting = _waiting;
    if (waiting != null) {
      _waiting = null;
      waiting.complete(step);
    } else {
      _pending.add(step);
    }
  }

  @override
  Future<void> start({required String hubUrl, required String deviceToken, required String deviceId}) async {
    started.add({'hubUrl': hubUrl, 'deviceToken': deviceToken, 'deviceId': deviceId});
  }

  @override
  Future<bridge.ArcaPairStep> next() {
    if (_pending.isNotEmpty) return Future.value(_pending.removeAt(0));
    return (_waiting = Completer<bridge.ArcaPairStep>()).future;
  }

  @override
  Future<void> answer(String typed) async => answers.add(typed);

  @override
  Future<void> cancel() async => cancels++;
}

bridge.ArcaPairStep _offerCode(String code) => bridge.ArcaPairStep(kind: bridge.ArcaPairStepKind.offerCode, code: code);
const _ask = bridge.ArcaPairStep(kind: bridge.ArcaPairStepKind.askFingerprint, code: '');
const _done = bridge.ArcaPairStep(kind: bridge.ArcaPairStepKind.done, code: '');
bridge.ArcaPairStep _failed(bridge.ArcaPairErrorKind kind) =>
    bridge.ArcaPairStep(kind: bridge.ArcaPairStepKind.failed, code: '', failure: bridge.ArcaPairFailure(kind: kind, detail: 'english detail'));

const _fingerprint = 'a1b2-c3d4-e5f6-0718';

void _savedHub() => SharedPreferences.setMockInitialValues({
      'connection.host': 'hub.local',
      'connection.port': 7420,
      'connection.authKey': 'key',
      'connection.useTls': true,
      'connection.deviceId': 'phone-1234',
      'connection.deviceToken.hub.local:7420': 'tok',
    });

Future<void> _show(WidgetTester tester, FakeBackend backend, {Locale locale = const Locale('en')}) async {
  await tester.pumpWidget(testApp(home: ArcaPairScreen(backend: backend), locale: locale));
  await _settle(tester);
}

// Not `pumpAndSettle`: the waiting spinner never settles.
Future<void> _settle(WidgetTester tester) async {
  for (var i = 0; i < 5; i++) {
    await tester.pump(const Duration(milliseconds: 50));
  }
}

Future<void> _startPairing(WidgetTester tester, FakeBackend backend) async {
  await tester.tap(find.byKey(const Key('arca-pair-start')));
  await _settle(tester);
}

void main() {
  group('TypedOnlyFormatter', () {
    final formatter = TypedOnlyFormatter();
    TextEditingValue value(String text) => TextEditingValue(text: text, selection: TextSelection.collapsed(offset: text.length));

    test('lets a person type one character at a time', () {
      expect(formatter.formatEditUpdate(value('a1b'), value('a1b2')).text, 'a1b2');
    });

    test('lets a person delete', () {
      expect(formatter.formatEditUpdate(value('a1b2'), value('a1b')).text, 'a1b');
      expect(formatter.formatEditUpdate(value('a1b2'), value('')).text, '');
    });

    test('refuses what arrives in a lump (paste, drop, autofill) and keeps what was there', () {
      expect(formatter.formatEditUpdate(value(''), value(_fingerprint)).text, '');
      expect(formatter.formatEditUpdate(value('a1'), value('a1$_fingerprint')).text, 'a1');
    });
  });

  group('arcaPairErrorText', () {
    final en = AppLocalizationsEn();
    final pt = AppLocalizationsPt();

    test('has a sentence for every kind, in both languages', () {
      for (final kind in bridge.ArcaPairErrorKind.values) {
        expect(arcaPairErrorText(en, kind), isNotEmpty, reason: 'en $kind');
        expect(arcaPairErrorText(pt, kind), isNotEmpty, reason: 'pt $kind');
        expect(arcaPairErrorText(pt, kind), isNot(arcaPairErrorText(en, kind)), reason: '$kind is translated');
      }
    });

    test('tells the kinds apart, so the person is not told the wrong thing', () {
      final texts = bridge.ArcaPairErrorKind.values.map((k) => arcaPairErrorText(en, k)).toSet();
      expect(texts.length, bridge.ArcaPairErrorKind.values.length);
    });

    test('never shows the hub\'s English detail as the message', () {
      for (final kind in bridge.ArcaPairErrorKind.values) {
        expect(arcaPairErrorText(pt, kind), isNot(contains('english detail')));
      }
    });
  });

  group('ArcaPairScreen', () {
    testWidgets('with no saved connection it says to connect first and offers no way to start', (tester) async {
      SharedPreferences.setMockInitialValues({});
      final backend = FakeBackend();
      await _show(tester, backend);

      expect(find.text(AppLocalizationsEn().arcaPairNoHub), findsOneWidget);
      expect(find.byKey(const Key('arca-pair-start')), findsNothing);
      expect(backend.started, isEmpty);
    });

    testWidgets('starting uses the saved hub, token and device id, and then shows the offer code', (tester) async {
      _savedHub();
      final backend = FakeBackend()..push(_offerCode('abcd-0123-4567-89ef'));
      await _show(tester, backend);

      await _startPairing(tester, backend);

      expect(backend.started, [
        {'hubUrl': 'https://hub.local:7420', 'deviceToken': 'tok', 'deviceId': 'phone-1234'},
      ]);
      expect(find.text('abcd-0123-4567-89ef'), findsOneWidget);
      expect(find.text(AppLocalizationsEn().arcaPairShowCode), findsOneWidget);
    });

    testWidgets('asks for the fingerprint only when the bridge asks, and sends what was typed', (tester) async {
      _savedHub();
      final backend = FakeBackend()
        ..push(_offerCode('abcd-0123-4567-89ef'))
        ..push(_ask);
      await _show(tester, backend);
      await _startPairing(tester, backend);

      final field = find.byKey(const Key('arca-pair-fingerprint'));
      expect(field, findsOneWidget);
      // Typed one character at a time, as a person does.
      for (var i = 1; i <= _fingerprint.length; i++) {
        await tester.enterText(field, _fingerprint.substring(0, i));
      }
      await tester.tap(find.byKey(const Key('arca-pair-confirm')));
      await _settle(tester);

      expect(backend.answers, [_fingerprint]);
    });

    testWidgets('a fingerprint pasted in one go does not get into the field', (tester) async {
      _savedHub();
      final backend = FakeBackend()
        ..push(_offerCode('abcd-0123-4567-89ef'))
        ..push(_ask);
      await _show(tester, backend);
      await _startPairing(tester, backend);

      final field = find.byKey(const Key('arca-pair-fingerprint'));
      await tester.enterText(field, _fingerprint);
      await tester.pump();

      expect(tester.widget<TextField>(field).controller!.text, isEmpty);
      // And the paste menu is not offered either.
      expect(tester.widget<TextField>(field).contextMenuBuilder, isNotNull);
    });

    testWidgets('when it is done, it says the device has the key', (tester) async {
      _savedHub();
      final backend = FakeBackend()
        ..push(_offerCode('abcd-0123-4567-89ef'))
        ..push(_done);
      await _show(tester, backend);
      await _startPairing(tester, backend);

      expect(find.text(AppLocalizationsEn().arcaPairDone), findsOneWidget);
    });

    testWidgets('a failure is shown in the app\'s language, not the hub\'s English', (tester) async {
      _savedHub();
      final backend = FakeBackend()..push(_failed(bridge.ArcaPairErrorKind.fingerprintMismatch));
      await _show(tester, backend, locale: const Locale('pt'));

      await _startPairing(tester, backend);

      expect(find.text(AppLocalizationsPt().arcaPairErrFingerprintMismatch), findsOneWidget);
      expect(find.textContaining('english detail'), findsNothing);
    });

    testWidgets('giving up tells the bridge', (tester) async {
      _savedHub();
      final backend = FakeBackend()..push(_offerCode('abcd-0123-4567-89ef'));
      await _show(tester, backend);
      await _startPairing(tester, backend);

      await tester.tap(find.byKey(const Key('arca-pair-cancel')));
      await _settle(tester);

      expect(backend.cancels, 1);
    });
  });
}
