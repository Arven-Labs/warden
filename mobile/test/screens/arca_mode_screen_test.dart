import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:mobile/l10n/generated/app_localizations_en.dart';
import 'package:mobile/l10n/generated/app_localizations_pt.dart';
import 'package:mobile/screens/arca_mode_screen.dart';
import 'package:mobile/services/arca_mode_flow.dart';
import 'package:mobile/src/rust/api/arca_mode.dart' as bridge;
import 'package:shared_preferences/shared_preferences.dart';

import '../helpers/test_app.dart';

/// A stand-in for the Rust bridge: records what the screen asks and answers from what the test set up.
class FakeBackend implements ArcaModeBackend {
  FakeBackend({this.on = false, this.sendPending = false});

  bool on;
  bool sendPending;

  /// Thrown by `enable` when set; `enableLeavesItOn` says whether the mode ended up on anyway (the process died after saving it).
  bridge.ArcaModeFailure? enableFailure;
  bool enableLeavesItOn = false;
  bridge.ArcaModeFailure? syncFailure;
  bridge.ArcaSyncDto syncResult = const bridge.ArcaSyncDto(snapshotCreated: false, pulled: 2, merges: 0, conflicts: 0, filesChanged: 2, pushed: 0, vaultEmptied: false);
  List<bridge.PendingEpochDto> epochs = [];
  int acknowledgeReturns = 1;

  int statusCalls = 0;
  int enableCalls = 0;
  int pendingCalls = 0;
  final syncCalls = <Map<String, Object>>[];
  final acknowledged = <List<bridge.PendingEpochDto>>[];

  @override
  Future<bridge.ArcaModeStatusDto> status() async {
    statusCalls++;
    return bridge.ArcaModeStatusDto(on_: on, sendPending: sendPending);
  }

  @override
  Future<void> enable() async {
    enableCalls++;
    final failure = enableFailure;
    if (failure != null) {
      if (enableLeavesItOn) on = true;
      throw failure;
    }
    on = true;
  }

  @override
  Future<bridge.ArcaSyncDto> syncRun({required String hubUrl, required String deviceToken, required bool allowInsecureHttp}) async {
    syncCalls.add({'hubUrl': hubUrl, 'deviceToken': deviceToken, 'allowInsecureHttp': allowInsecureHttp});
    final failure = syncFailure;
    if (failure != null) throw failure;
    return syncResult;
  }

  @override
  Future<List<bridge.PendingEpochDto>> epochsPending() async {
    pendingCalls++;
    return List.of(epochs);
  }

  @override
  Future<int> epochsAcknowledge(List<bridge.PendingEpochDto> seen) async {
    acknowledged.add(List.of(seen));
    return acknowledgeReturns;
  }
}

bridge.ArcaModeFailure _failure(bridge.ArcaModeErrorKind kind) => bridge.ArcaModeFailure(kind: kind, detail: 'english detail SECRET');

bridge.PendingEpochDto _epoch(int number, String hex) => bridge.PendingEpochDto(epoch: number, id: 'manifest/$hex');

const _hex1 = 'abcdef0123456789abcdef0123456789abcdef0123456789abcdef0123456789';
const _hex2 = '00112233445566778899aabbccddeeff00112233445566778899aabbccddeeff';

void _savedHub({bool tls = true}) => SharedPreferences.setMockInitialValues({
      'connection.host': 'hub.local',
      'connection.port': 7420,
      'connection.authKey': 'key',
      'connection.useTls': tls,
      'connection.deviceId': 'phone-1234',
      'connection.deviceToken.hub.local:7420': 'tok',
    });

Future<void> _show(WidgetTester tester, FakeBackend backend, {Locale locale = const Locale('en')}) async {
  await tester.pumpWidget(testApp(home: ArcaModeScreen(backend: backend), locale: locale));
  await _settle(tester);
}

// Not `pumpAndSettle`: a spinner never settles.
Future<void> _settle(WidgetTester tester) async {
  for (var i = 0; i < 5; i++) {
    await tester.pump(const Duration(milliseconds: 50));
  }
}

Future<void> _tap(WidgetTester tester, String key) async {
  await tester.tap(find.byKey(Key(key)));
  await _settle(tester);
}

void main() {
  final en = AppLocalizationsEn();
  final pt = AppLocalizationsPt();

  group('arcaModeErrorText', () {
    test('has a sentence for every kind, in both languages, and it is translated', () {
      for (final kind in bridge.ArcaModeErrorKind.values) {
        expect(arcaModeErrorText(en, kind), isNotEmpty, reason: 'en $kind');
        expect(arcaModeErrorText(pt, kind), isNotEmpty, reason: 'pt $kind');
        expect(arcaModeErrorText(pt, kind), isNot(arcaModeErrorText(en, kind)), reason: '$kind is translated');
      }
    });

    test('tells the kinds apart, so the person is not told the wrong thing', () {
      final texts = bridge.ArcaModeErrorKind.values.map((k) => arcaModeErrorText(en, k)).toSet();
      expect(texts.length, bridge.ArcaModeErrorKind.values.length);
    });
  });

  group('epochShortId', () {
    test('keeps the first 12 hex digits after the kind', () {
      expect(epochShortId('manifest/$_hex1'), 'abcdef012345');
      expect(epochShortId('abcdef0123456789'), 'abcdef012345', reason: 'without a kind it is the same text');
      expect(epochShortId('manifest/ab'), 'ab');
    });
  });

  group('ArcaModeScreen', () {
    testWidgets('with no saved connection it says to connect first and offers no way to sync', (tester) async {
      SharedPreferences.setMockInitialValues({});
      final backend = FakeBackend(on: true);
      await _show(tester, backend);

      expect(find.text(en.arcaModeNoHub), findsOneWidget);
      expect(find.byKey(const Key('arca-mode-sync')), findsNothing);
      expect(backend.syncCalls, isEmpty);
    });

    testWidgets('with the mode off it offers to turn it on and hides the sync', (tester) async {
      _savedHub();
      await _show(tester, FakeBackend());

      expect(find.text(en.arcaModeStatusOff), findsOneWidget);
      expect(find.byKey(const Key('arca-mode-enable')), findsOneWidget);
      expect(find.byKey(const Key('arca-mode-sync')), findsNothing);
    });

    testWidgets('turning it on asks first, says what it costs, and does nothing if the person says not now', (tester) async {
      _savedHub();
      final backend = FakeBackend();
      await _show(tester, backend);

      await _tap(tester, 'arca-mode-enable');
      expect(find.text(en.arcaModeEnableWarning), findsOneWidget);
      expect(backend.enableCalls, 0, reason: 'nothing happens before the answer');

      await _tap(tester, 'arca-mode-enable-no');
      expect(backend.enableCalls, 0);
      expect(find.text(en.arcaModeStatusOff), findsOneWidget);
    });

    testWidgets('after the yes it turns the mode on and shows the sync', (tester) async {
      _savedHub();
      final backend = FakeBackend();
      await _show(tester, backend);

      await _tap(tester, 'arca-mode-enable');
      await _tap(tester, 'arca-mode-enable-yes');

      expect(backend.enableCalls, 1);
      expect(find.text(en.arcaModeStatusOn), findsOneWidget);
      expect(find.byKey(const Key('arca-mode-sync')), findsOneWidget);
    });

    testWidgets('a Send waiting for the phone is told in words and the mode stays off', (tester) async {
      _savedHub();
      final backend = FakeBackend(sendPending: true)..enableFailure = _failure(bridge.ArcaModeErrorKind.sendPending);
      await _show(tester, backend);

      await _tap(tester, 'arca-mode-enable');
      await _tap(tester, 'arca-mode-enable-yes');

      expect(find.text(en.arcaModeErrSendPending), findsOneWidget);
      expect(find.textContaining('english detail'), findsNothing);
      expect(find.byKey(const Key('arca-mode-sync')), findsNothing);
    });

    testWidgets('"already on" is not a failure: the screen just shows the mode on', (tester) async {
      _savedHub();
      final backend = FakeBackend()
        ..enableFailure = _failure(bridge.ArcaModeErrorKind.alreadyOn)
        ..enableLeavesItOn = true;
      await _show(tester, backend);

      await _tap(tester, 'arca-mode-enable');
      await _tap(tester, 'arca-mode-enable-yes');

      expect(find.text(en.arcaModeStatusOn), findsOneWidget);
      expect(find.text(en.arcaModeErrAlreadyOn), findsNothing);
    });

    testWidgets('after any error on turning it on, the status is read again (the process may have saved the mode before it failed)', (tester) async {
      _savedHub();
      final backend = FakeBackend()
        ..enableFailure = _failure(bridge.ArcaModeErrorKind.other)
        ..enableLeavesItOn = true;
      await _show(tester, backend);
      final before = backend.statusCalls;

      await _tap(tester, 'arca-mode-enable');
      await _tap(tester, 'arca-mode-enable-yes');

      expect(backend.statusCalls, greaterThan(before));
      expect(find.text(en.arcaModeStatusOn), findsOneWidget);
    });

    testWidgets('with a TLS hub, sync uses https and the saved token, and never the insecure permission', (tester) async {
      _savedHub();
      final backend = FakeBackend(on: true);
      await _show(tester, backend);

      await _tap(tester, 'arca-mode-sync');

      expect(backend.syncCalls, [
        {'hubUrl': 'https://hub.local:7420', 'deviceToken': 'tok', 'allowInsecureHttp': false},
      ]);
      expect(find.text(en.arcaModeInsecureTitle), findsNothing);
      expect(find.text(en.arcaModeSynced(2)), findsOneWidget);
    });

    testWidgets('with a plain-http hub it warns first, with the address, and does not call the hub yet', (tester) async {
      _savedHub(tls: false);
      final backend = FakeBackend(on: true);
      await _show(tester, backend);

      await _tap(tester, 'arca-mode-sync');

      expect(find.text(en.arcaModeInsecureTitle), findsOneWidget);
      expect(find.text(en.arcaModeInsecureWarning('http://hub.local:7420')), findsOneWidget);
      expect(backend.syncCalls, isEmpty, reason: 'the token does not leave before the person agrees');
    });

    testWidgets('refusing the warning does not call the hub', (tester) async {
      _savedHub(tls: false);
      final backend = FakeBackend(on: true);
      await _show(tester, backend);

      await _tap(tester, 'arca-mode-sync');
      await _tap(tester, 'arca-mode-insecure-no');

      expect(backend.syncCalls, isEmpty);
      expect(find.text(en.arcaModeInsecureTitle), findsNothing);
    });

    testWidgets('after the person agrees, the cycle runs over http with the permission, and the next one in this screen does not ask again', (tester) async {
      _savedHub(tls: false);
      final backend = FakeBackend(on: true);
      await _show(tester, backend);

      await _tap(tester, 'arca-mode-sync');
      await _tap(tester, 'arca-mode-insecure-yes');
      expect(backend.syncCalls, [
        {'hubUrl': 'http://hub.local:7420', 'deviceToken': 'tok', 'allowInsecureHttp': true},
      ]);

      await _tap(tester, 'arca-mode-sync');
      expect(find.text(en.arcaModeInsecureTitle), findsNothing, reason: 'same address, same screen');
      expect(backend.syncCalls.length, 2);
      expect(backend.syncCalls.last['allowInsecureHttp'], true);
    });

    testWidgets('the agreement is not kept: opening the screen again asks again', (tester) async {
      _savedHub(tls: false);
      final backend = FakeBackend(on: true);
      await _show(tester, backend);
      await _tap(tester, 'arca-mode-sync');
      await _tap(tester, 'arca-mode-insecure-yes');

      // A new screen, as when the person leaves and comes back.
      await tester.pumpWidget(const SizedBox.shrink());
      await _show(tester, backend);
      await _tap(tester, 'arca-mode-sync');

      expect(find.text(en.arcaModeInsecureTitle), findsOneWidget);
      expect(backend.syncCalls.length, 1, reason: 'the second cycle waits for the new agreement');
    });

    testWidgets('a hub that does not know the device is told in the app\'s language, not in the hub\'s English', (tester) async {
      _savedHub();
      final backend = FakeBackend(on: true)..syncFailure = _failure(bridge.ArcaModeErrorKind.unauthorized);
      await _show(tester, backend, locale: const Locale('pt'));

      await _tap(tester, 'arca-mode-sync');

      expect(find.text(pt.arcaModeErrUnauthorized), findsOneWidget);
      expect(find.textContaining('english detail'), findsNothing);
      expect(find.textContaining('SECRET'), findsNothing);
    });

    testWidgets('a vault that was left out of the cycle because it is empty is told, not reported as synced', (tester) async {
      _savedHub();
      final backend = FakeBackend(on: true)
        ..syncResult = const bridge.ArcaSyncDto(snapshotCreated: false, pulled: 0, merges: 0, conflicts: 0, filesChanged: 0, pushed: 0, vaultEmptied: true);
      await _show(tester, backend);

      await _tap(tester, 'arca-mode-sync');

      expect(find.text(en.arcaModeVaultEmptied), findsOneWidget);
      expect(find.text(en.arcaModeSynced(0)), findsNothing);
    });

    testWidgets('a pending newer key is listed with its epoch and the short id', (tester) async {
      _savedHub();
      final backend = FakeBackend(on: true)..epochs = [_epoch(1, _hex1), _epoch(2, _hex2)];
      await _show(tester, backend);

      expect(find.text(en.arcaModeEpochsTitle), findsOneWidget);
      expect(find.text(en.arcaModeEpochItem(1, 'abcdef012345')), findsOneWidget);
      expect(find.text(en.arcaModeEpochItem(2, '001122334455')), findsOneWidget);
      expect(find.byKey(const Key('arca-mode-ack')), findsOneWidget);
    });

    testWidgets('with nothing pending there is no list and no acknowledge button', (tester) async {
      _savedHub();
      await _show(tester, FakeBackend(on: true));

      expect(find.text(en.arcaModeEpochsTitle), findsNothing);
      expect(find.byKey(const Key('arca-mode-ack')), findsNothing);
    });

    testWidgets('acknowledging sends the whole pairs the person saw and says how many were acknowledged', (tester) async {
      _savedHub();
      final backend = FakeBackend(on: true)
        ..epochs = [_epoch(1, _hex1), _epoch(2, _hex2)]
        ..acknowledgeReturns = 2;
      await _show(tester, backend);

      await _tap(tester, 'arca-mode-ack');

      expect(backend.acknowledged, [
        [_epoch(1, _hex1), _epoch(2, _hex2)],
      ]);
      expect(find.text(en.arcaModeEpochsAcknowledged(2)), findsOneWidget);
    });

    testWidgets('when nothing was acknowledged the list is read again and the person is told it changed', (tester) async {
      _savedHub();
      final backend = FakeBackend(on: true)
        ..epochs = [_epoch(1, _hex1)]
        ..acknowledgeReturns = 0;
      await _show(tester, backend);
      final before = backend.pendingCalls;

      // A cycle brought another epoch in while the person was looking.
      backend.epochs = [_epoch(1, _hex1), _epoch(2, _hex2)];
      await _tap(tester, 'arca-mode-ack');

      expect(find.text(en.arcaModeEpochsChanged), findsOneWidget);
      expect(find.text(en.arcaModeEpochsAcknowledged(0)), findsNothing, reason: '"acknowledged" would be a lie');
      expect(backend.pendingCalls, greaterThan(before));
      expect(find.text(en.arcaModeEpochItem(2, '001122334455')), findsOneWidget, reason: 'the new one is on the screen now');
    });
  });
}
