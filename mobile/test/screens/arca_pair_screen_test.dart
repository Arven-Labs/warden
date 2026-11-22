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
  FakeBackend({this.playedRole = bridge.ArcaPairRole.offer});

  final bridge.ArcaPairRole playedRole;
  final started = <Map<String, String>>[];
  final startedAsGrant = <Map<String, String>>[];
  final picks = <int>[];
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
  Future<bridge.ArcaPairRole> role() async {
    roleCalls++;
    return playedRole;
  }

  @override
  Future<void> startGrant({required String hubUrl, required String deviceToken}) async {
    startedAsGrant.add({'hubUrl': hubUrl, 'deviceToken': deviceToken});
  }

  final startedSwapGrant = <Map<String, String>>[];
  final startedSwapReplace = <Map<String, String>>[];
  int roleCalls = 0;

  @override
  Future<void> startSwapGrant({required String hubUrl, required String deviceToken}) async {
    startedSwapGrant.add({'hubUrl': hubUrl, 'deviceToken': deviceToken});
  }

  @override
  Future<void> startSwapReplace({required String hubUrl, required String deviceToken, required String deviceId}) async {
    startedSwapReplace.add({'hubUrl': hubUrl, 'deviceToken': deviceToken, 'deviceId': deviceId});
  }

  @override
  Future<void> pick(int index) async => picks.add(index);

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

bridge.ArcaPairStep _step(bridge.ArcaPairStepKind kind, {String code = '', List<String> offers = const [], String fingerprint = '', bridge.ArcaPairFailure? failure}) =>
    bridge.ArcaPairStep(kind: kind, code: code, offers: offers, fingerprint: fingerprint, failure: failure);

bridge.ArcaPairStep _offerCode(String code) => _step(bridge.ArcaPairStepKind.offerCode, code: code);
final _ask = _step(bridge.ArcaPairStepKind.askFingerprint);
final _done = _step(bridge.ArcaPairStepKind.done);
bridge.ArcaPairStep _failed(bridge.ArcaPairErrorKind kind) => _step(bridge.ArcaPairStepKind.failed, failure: bridge.ArcaPairFailure(kind: kind, detail: 'english detail'));
bridge.ArcaPairStep _chooseOffer(List<String> ids) => _step(bridge.ArcaPairStepKind.chooseOffer, offers: ids);
final _askOfferCode = _step(bridge.ArcaPairStepKind.askOfferCode);
bridge.ArcaPairStep _showFingerprint(String fingerprint) => _step(bridge.ArcaPairStepKind.showFingerprint, fingerprint: fingerprint);

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

  group('ArcaPairScreen as the device that grants', () {
    const offerCode = 'abcd-0123-4567-89ef';

    Future<FakeBackend> startedAsGrant(WidgetTester tester, {List<bridge.ArcaPairStep> steps = const []}) async {
      _savedHub();
      final backend = FakeBackend(playedRole: bridge.ArcaPairRole.grant);
      for (final step in steps) {
        backend.push(step);
      }
      await _show(tester, backend);
      await _startPairing(tester, backend);
      return backend;
    }

    testWidgets('starting hands the saved hub and token to the granting side, not the new-device one', (tester) async {
      final backend = await startedAsGrant(tester, steps: [_chooseOffer(['phone-a'])]);

      expect(backend.startedAsGrant, [
        {'hubUrl': 'https://hub.local:7420', 'deviceToken': 'tok'},
      ]);
      expect(backend.started, isEmpty);
    });

    testWidgets('lists the waiting devices by id and tells the bridge which one was picked', (tester) async {
      final backend = await startedAsGrant(tester, steps: [_chooseOffer(['phone-a', 'phone-b'])]);

      expect(find.text('phone-a'), findsOneWidget);
      expect(find.text('phone-b'), findsOneWidget);
      await tester.tap(find.byKey(const Key('arca-pair-offer-1')));
      await _settle(tester);

      expect(backend.picks, [1]);
    });

    testWidgets('asks for the code the new device shows and sends what was typed', (tester) async {
      final backend = await startedAsGrant(tester, steps: [_chooseOffer(['phone-a'])]);
      await tester.tap(find.byKey(const Key('arca-pair-offer-0')));
      await _settle(tester);
      backend.push(_askOfferCode);
      await _settle(tester);

      final field = find.byKey(const Key('arca-pair-code'));
      expect(field, findsOneWidget);
      for (var i = 1; i <= offerCode.length; i++) {
        await tester.enterText(field, offerCode.substring(0, i));
      }
      await tester.tap(find.byKey(const Key('arca-pair-confirm')));
      await _settle(tester);

      expect(backend.answers, [offerCode]);
    });

    testWidgets('a code pasted in one go does not get into the field', (tester) async {
      final backend = await startedAsGrant(tester, steps: [_chooseOffer(['phone-a'])]);
      await tester.tap(find.byKey(const Key('arca-pair-offer-0')));
      await _settle(tester);
      backend.push(_askOfferCode);
      await _settle(tester);

      final field = find.byKey(const Key('arca-pair-code'));
      await tester.enterText(field, offerCode);
      await tester.pump();

      expect(tester.widget<TextField>(field).controller!.text, isEmpty);
      expect(tester.widget<TextField>(field).contextMenuBuilder, isNotNull);
    });

    testWidgets('after the key is sent it keeps the fingerprint on screen, for the person to type on the new device', (tester) async {
      await startedAsGrant(tester, steps: [_showFingerprint(_fingerprint), _done]);

      expect(find.text(_fingerprint), findsOneWidget);
      expect(find.text(AppLocalizationsEn().arcaPairGranted), findsOneWidget);
    });

    testWidgets('a code that did not match is told in the app\'s language', (tester) async {
      _savedHub();
      final backend = FakeBackend(playedRole: bridge.ArcaPairRole.grant)..push(_failed(bridge.ArcaPairErrorKind.codeMismatch));
      await _show(tester, backend, locale: const Locale('pt'));

      await _startPairing(tester, backend);

      expect(find.text(AppLocalizationsPt().arcaPairErrCodeMismatch), findsOneWidget);
    });
  });

  group('ArcaPairScreen as the key swap (P-49)', () {
    const headCode = '0123-4567-89ab-cdef-0123-4567-89ab-cdef';
    const typedLine = '$_fingerprint $headCode';

    Future<FakeBackend> startedSwap(WidgetTester tester, ArcaSwapSide side, {List<bridge.ArcaPairStep> steps = const [], Locale locale = const Locale('en')}) async {
      _savedHub();
      final backend = FakeBackend();
      for (final step in steps) {
        backend.push(step);
      }
      await tester.pumpWidget(testApp(home: ArcaPairScreen(backend: backend, swap: side), locale: locale));
      await _settle(tester);
      await _startPairing(tester, backend);
      return backend;
    }

    testWidgets('the device that rotated starts the swap grant with the saved hub, and not the pairing ones', (tester) async {
      final backend = await startedSwap(tester, ArcaSwapSide.grant, steps: [_chooseOffer(['phone-a'])]);

      expect(backend.startedSwapGrant, [
        {'hubUrl': 'https://hub.local:7420', 'deviceToken': 'tok'},
      ]);
      expect(backend.startedAsGrant, isEmpty);
      expect(backend.started, isEmpty);
      expect(backend.startedSwapReplace, isEmpty);
    });

    testWidgets('the stranded device starts the swap replace with hub, token and device id, and shows its offer code', (tester) async {
      final backend = await startedSwap(tester, ArcaSwapSide.replace, steps: [_offerCode('abcd-0123-4567-89ef')]);

      expect(backend.startedSwapReplace, [
        {'hubUrl': 'https://hub.local:7420', 'deviceToken': 'tok', 'deviceId': 'phone-1234'},
      ]);
      expect(backend.started, isEmpty);
      expect(backend.startedSwapGrant, isEmpty);
      expect(find.text('abcd-0123-4567-89ef'), findsOneWidget);
    });

    testWidgets('it does not ask the bridge which role this device plays: the person already said', (tester) async {
      final backend = await startedSwap(tester, ArcaSwapSide.replace, steps: [_offerCode('abcd-0123-4567-89ef')]);

      expect(backend.roleCalls, 0);
    });

    testWidgets('it has its own title and its own introduction for each half', (tester) async {
      _savedHub();
      await tester.pumpWidget(testApp(home: ArcaPairScreen(backend: FakeBackend(), swap: ArcaSwapSide.replace)));
      await _settle(tester);
      expect(find.text(AppLocalizationsEn().arcaSwapTitle), findsOneWidget);
      expect(find.text(AppLocalizationsEn().arcaSwapReplaceIntro), findsOneWidget);

      await tester.pumpWidget(testApp(home: ArcaPairScreen(backend: FakeBackend(), swap: ArcaSwapSide.grant)));
      await _settle(tester);
      expect(find.text(AppLocalizationsEn().arcaSwapGrantIntro), findsOneWidget);
    });

    testWidgets('the replacing side asks for the fingerprint and the head code in the swap\'s words, and sends what was typed', (tester) async {
      final backend = await startedSwap(tester, ArcaSwapSide.replace, steps: [_offerCode('abcd-0123-4567-89ef'), _ask]);

      expect(find.text(AppLocalizationsEn().arcaSwapAskFingerprint), findsOneWidget);
      expect(find.text(AppLocalizationsEn().arcaPairAskFingerprint), findsNothing);
      final field = find.byKey(const Key('arca-pair-fingerprint'));
      for (var i = 1; i <= typedLine.length; i++) {
        await tester.enterText(field, typedLine.substring(0, i));
      }
      await tester.tap(find.byKey(const Key('arca-pair-confirm')));
      await _settle(tester);

      expect(backend.answers, [typedLine]);
    });

    testWidgets('a fingerprint and head code pasted in one go do not get into the field', (tester) async {
      await startedSwap(tester, ArcaSwapSide.replace, steps: [_offerCode('abcd-0123-4567-89ef'), _ask]);

      final field = find.byKey(const Key('arca-pair-fingerprint'));
      await tester.enterText(field, typedLine);
      await tester.pump();

      expect(tester.widget<TextField>(field).controller!.text, isEmpty);
      expect(tester.widget<TextField>(field).contextMenuBuilder, isNotNull);
    });

    testWidgets('the side that rotated keeps the fingerprint and the head code on screen, for the person to type on the other device', (tester) async {
      await startedSwap(tester, ArcaSwapSide.grant, steps: [_showFingerprint(typedLine), _done]);

      expect(find.text(typedLine), findsOneWidget);
      expect(find.text(AppLocalizationsEn().arcaSwapGranted), findsOneWidget);
      expect(find.text(AppLocalizationsEn().arcaPairGranted), findsNothing);
    });

    testWidgets('when the replacing side is done it says the key is the newer one, not that the device was paired', (tester) async {
      await startedSwap(tester, ArcaSwapSide.replace, steps: [_offerCode('abcd-0123-4567-89ef'), _done]);

      expect(find.text(AppLocalizationsEn().arcaSwapDone), findsOneWidget);
      expect(find.text(AppLocalizationsEn().arcaPairDone), findsNothing);
    });

    testWidgets('each way a swap can refuse is told in the app\'s language, and the hub-not-to-trust one says so', (tester) async {
      final pt = AppLocalizationsPt();
      final cases = {
        bridge.ArcaPairErrorKind.notInArcaMode: pt.arcaPairErrNotInArcaMode,
        bridge.ArcaPairErrorKind.nothingNewer: pt.arcaPairErrNothingNewer,
        bridge.ArcaPairErrorKind.notNewer: pt.arcaPairErrNotNewer,
        bridge.ArcaPairErrorKind.hashMismatch: pt.arcaPairErrHashMismatch,
      };
      for (final entry in cases.entries) {
        await startedSwap(tester, ArcaSwapSide.replace, steps: [_failed(entry.key)], locale: const Locale('pt'));
        expect(find.text(entry.value), findsOneWidget, reason: '${entry.key}');
      }
      expect(pt.arcaPairErrHashMismatch, contains('não confie'));
    });

    testWidgets('after a failure the same button starts the swap again', (tester) async {
      final backend = await startedSwap(tester, ArcaSwapSide.replace, steps: [_failed(bridge.ArcaPairErrorKind.timedOut)]);

      await _startPairing(tester, backend);

      expect(backend.startedSwapReplace.length, 2);
      expect(backend.started, isEmpty);
    });

    testWidgets('giving up during a swap tells the bridge, with the swap\'s words on the button', (tester) async {
      final backend = await startedSwap(tester, ArcaSwapSide.replace, steps: [_offerCode('abcd-0123-4567-89ef')]);

      expect(find.text(AppLocalizationsEn().arcaSwapCancel), findsOneWidget);
      await tester.tap(find.byKey(const Key('arca-pair-cancel')));
      await _settle(tester);

      expect(backend.cancels, 1);
    });
  });
}
