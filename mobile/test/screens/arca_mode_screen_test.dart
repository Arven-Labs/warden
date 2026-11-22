import 'dart:async';

import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:mobile/l10n/generated/app_localizations_en.dart';
import 'package:mobile/l10n/generated/app_localizations_pt.dart';
import 'package:mobile/screens/arca_mode_screen.dart';
import 'package:mobile/services/arca_mode_flow.dart';
import 'package:mobile/services/arca_pair_flow.dart';
import 'package:mobile/services/connection_settings.dart';
import 'package:mobile/src/rust/api/arca_mode.dart' as bridge;
import 'package:mobile/src/rust/api/arca_pair.dart' as bridge_pair;
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

  /// When set, reading the mode (or the newer key epochs) fails, as a hub or a disk that does not answer would make it.
  bool statusFails = false;
  bool epochsFail = false;

  int statusCalls = 0;
  int enableCalls = 0;
  int pendingCalls = 0;
  final syncCalls = <Map<String, Object>>[];
  final acknowledged = <List<bridge.PendingEpochDto>>[];

  @override
  Future<bridge.ArcaModeStatusDto> status() async {
    statusCalls++;
    if (statusFails) throw _failure(bridge.ArcaModeErrorKind.other);
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
    if (epochsFail) throw _failure(bridge.ArcaModeErrorKind.other);
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

void _savedHub({bool tls = true, String host = 'hub.local'}) => SharedPreferences.setMockInitialValues({
      'connection.host': host,
      'connection.port': 7420,
      'connection.authKey': 'key',
      'connection.useTls': tls,
      'connection.deviceId': 'phone-1234',
      'connection.deviceToken.$host:7420': 'tok',
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

/// Leaves the open dialog the way the Android back button does: no answer at all (`null`), neither "yes" nor "no".
Future<void> _back(WidgetTester tester) async {
  await tester.state<NavigatorState>(find.byType(Navigator).first).maybePop();
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

    testWidgets('a hub out on the internet over plain http gets the same warning, and the token only goes after the yes', (tester) async {
      // Any network is allowed (the person's decision), so the warning is the only thing between the token and the wire.
      _savedHub(tls: false, host: 'hub.example.com');
      final backend = FakeBackend(on: true);
      await _show(tester, backend);

      await _tap(tester, 'arca-mode-sync');
      expect(find.text(en.arcaModeInsecureWarning('http://hub.example.com:7420')), findsOneWidget);
      expect(backend.syncCalls, isEmpty);

      await _tap(tester, 'arca-mode-insecure-yes');
      expect(backend.syncCalls, [
        {'hubUrl': 'http://hub.example.com:7420', 'deviceToken': 'tok', 'allowInsecureHttp': true},
      ]);
    });

    testWidgets('an IPv6 hub is written with brackets, in the warning and in the call', (tester) async {
      _savedHub(tls: false, host: '2001:db8::1');
      final backend = FakeBackend(on: true);
      await _show(tester, backend);

      await _tap(tester, 'arca-mode-sync');
      expect(find.text(en.arcaModeInsecureWarning('http://[2001:db8::1]:7420')), findsOneWidget);
      await _tap(tester, 'arca-mode-insecure-yes');

      expect(backend.syncCalls.single['hubUrl'], 'http://[2001:db8::1]:7420');
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
      // What was sent is what the person was looking at (the first list), never the one read after it.
      expect(backend.acknowledged, [
        [_epoch(1, _hex1)],
      ]);
    });

    // ---------- what the audit of this screen found ----------

    testWidgets('a mode that could not be read is not shown as off, and nothing irreversible is offered', (tester) async {
      _savedHub();
      final backend = FakeBackend()..statusFails = true;
      await _show(tester, backend);

      expect(find.text(en.arcaModeErrOther), findsOneWidget);
      expect(find.text(en.arcaModeStatusOff), findsNothing);
      expect(find.byKey(const Key('arca-mode-enable')), findsNothing, reason: 'it could already be on');
      expect(find.byKey(const Key('arca-mode-sync')), findsNothing);
    });

    testWidgets('newer keys that could not be read are not shown as "nothing pending"', (tester) async {
      _savedHub();
      final backend = FakeBackend(on: true)..epochsFail = true;
      await _show(tester, backend);

      expect(find.text(en.arcaModeStatusOn), findsOneWidget, reason: 'the mode itself was read');
      expect(find.byKey(const Key('arca-mode-epochs-unreadable')), findsOneWidget);
      expect(find.byKey(const Key('arca-mode-sync')), findsOneWidget);
    });

    testWidgets('leaving the "turn on" dialog without answering does not turn it on, and tapping outside does not leave it', (tester) async {
      _savedHub();
      final backend = FakeBackend();
      await _show(tester, backend);

      await _tap(tester, 'arca-mode-enable');
      await tester.tapAt(const Offset(2, 2));
      await _settle(tester);
      expect(find.text(en.arcaModeEnableWarning), findsOneWidget, reason: 'a tap outside is not an answer');

      await _back(tester);
      expect(find.text(en.arcaModeEnableWarning), findsNothing);
      expect(backend.enableCalls, 0);
      expect(find.text(en.arcaModeStatusOff), findsOneWidget);
    });

    testWidgets('leaving the plain-http warning without answering sends nothing, and tapping outside does not leave it', (tester) async {
      _savedHub(tls: false);
      final backend = FakeBackend(on: true);
      await _show(tester, backend);

      await _tap(tester, 'arca-mode-sync');
      await tester.tapAt(const Offset(2, 2));
      await _settle(tester);
      expect(find.text(en.arcaModeInsecureTitle), findsOneWidget, reason: 'a tap outside is not an agreement');

      await _back(tester);
      expect(backend.syncCalls, isEmpty, reason: 'no answer is not a yes: the token stays on the phone');
    });

    testWidgets('a second tap while the warning is opening does not stack another dialog', (tester) async {
      _savedHub(tls: false);
      final backend = FakeBackend(on: true);
      await _show(tester, backend);

      final onPressed = tester.widget<FilledButton>(find.byKey(const Key('arca-mode-sync'))).onPressed!;
      onPressed();
      onPressed();
      await _settle(tester);

      expect(find.text(en.arcaModeInsecureTitle), findsOneWidget);
      await _tap(tester, 'arca-mode-insecure-no');
      expect(find.text(en.arcaModeInsecureTitle), findsNothing, reason: 'there was only one to close');
    });

    testWidgets('if the hub changed since the person agreed, the warning comes again with the new address', (tester) async {
      _savedHub(tls: false);
      final backend = FakeBackend(on: true);
      await _show(tester, backend);
      await _tap(tester, 'arca-mode-sync');
      await _tap(tester, 'arca-mode-insecure-yes');
      expect(backend.syncCalls.length, 1);

      // Saved again by the Connection screen while this one stayed open.
      final store = ConnectionSettingsStore();
      await store.save(const ConnectionSettings(host: 'other.local', port: 7420, authKey: 'key', deviceName: 'phone', useTls: false));
      await store.saveDeviceToken('other.local', 7420, 'tok-other');
      await _tap(tester, 'arca-mode-sync');

      expect(find.text(en.arcaModeInsecureWarning('http://other.local:7420')), findsOneWidget, reason: 'the agreement was for the other address');
      expect(backend.syncCalls.length, 1, reason: 'nothing was sent to the new address');
    });

    testWidgets('a saved hub with no token for it offers no sync', (tester) async {
      SharedPreferences.setMockInitialValues({'connection.host': 'hub.local', 'connection.port': 7420, 'connection.authKey': 'key', 'connection.useTls': true});
      final backend = FakeBackend(on: true);
      await _show(tester, backend);

      expect(find.text(en.arcaModeNoHub), findsOneWidget);
      expect(find.byKey(const Key('arca-mode-sync')), findsNothing);
    });
  });

  group('the key swap buttons (P-49)', () {
    Future<(FakeBackend, FakePairBackend)> shown(WidgetTester tester, {bool on = true, List<bridge.PendingEpochDto>? epochs}) async {
      _savedHub();
      final backend = FakeBackend(on: on)..epochs = epochs ?? [];
      final pair = FakePairBackend();
      await tester.pumpWidget(testApp(home: ArcaModeScreen(backend: backend, pairBackend: pair)));
      await _settle(tester);
      return (backend, pair);
    }

    testWidgets('with the mode on and a hub, the device can hand the new key to another one, and the screen it opens starts the swap grant', (tester) async {
      final (_, pair) = await shown(tester);

      await _tap(tester, 'arca-mode-swap-grant');
      expect(find.text(en.arcaSwapTitle), findsOneWidget);
      await tester.tap(find.byKey(const Key('arca-pair-start')));
      await _settle(tester);

      expect(pair.startedSwapGrant, [
        {'hubUrl': 'https://hub.local:7420', 'deviceToken': 'tok'},
      ]);
      expect(pair.startedSwapReplace, isEmpty);
    });

    testWidgets('with no newer key epoch pending there is no way to replace this device\'s key', (tester) async {
      await shown(tester);

      expect(find.byKey(const Key('arca-mode-swap-replace')), findsNothing);
    });

    testWidgets('with a newer key epoch pending the device can replace its key', (tester) async {
      await shown(tester, epochs: [_epoch(1, _hex1)]);

      expect(find.byKey(const Key('arca-mode-swap-replace')), findsOneWidget);
    });

    testWidgets('while a newer key is pending this device does not offer to hand over its (old) key', (tester) async {
      await shown(tester, epochs: [_epoch(1, _hex1)]);

      expect(find.byKey(const Key('arca-mode-swap-grant')), findsNothing);
      expect(find.byKey(const Key('arca-mode-swap-replace')), findsOneWidget);
    });

    testWidgets('neither button shows with the mode off', (tester) async {
      await shown(tester, on: false, epochs: [_epoch(1, _hex1)]);

      expect(find.byKey(const Key('arca-mode-swap-grant')), findsNothing);
      expect(find.byKey(const Key('arca-mode-swap-replace')), findsNothing);
    });

    testWidgets('neither button shows without a hub the sync can use', (tester) async {
      SharedPreferences.setMockInitialValues({'connection.host': 'hub.local', 'connection.port': 7420, 'connection.authKey': 'key', 'connection.useTls': true});
      final backend = FakeBackend(on: true)..epochs = [_epoch(1, _hex1)];
      await tester.pumpWidget(testApp(home: ArcaModeScreen(backend: backend, pairBackend: FakePairBackend())));
      await _settle(tester);

      expect(find.byKey(const Key('arca-mode-swap-grant')), findsNothing);
      expect(find.byKey(const Key('arca-mode-swap-replace')), findsNothing);
    });

    testWidgets('replacing reads the newer epochs again at the tap and asks about those, not the ones the screen had', (tester) async {
      final (backend, pair) = await shown(tester, epochs: [_epoch(1, _hex1)]);
      final readsBefore = backend.pendingCalls;
      backend.epochs = [_epoch(2, _hex2)];

      await _tap(tester, 'arca-mode-swap-replace');

      expect(backend.pendingCalls, greaterThan(readsBefore));
      expect(find.text(en.arcaSwapReplaceConfirmTitle), findsOneWidget);
      expect(find.text(en.arcaModeEpochItem(2, epochShortId('manifest/$_hex2'))), findsWidgets);
      // Nothing started on the tap alone.
      expect(pair.startedSwapReplace, isEmpty);
      expect(find.text(en.arcaSwapTitle), findsNothing);
    });

    testWidgets('"not now" on the confirmation changes nothing and opens nothing', (tester) async {
      final (_, pair) = await shown(tester, epochs: [_epoch(1, _hex1)]);

      await _tap(tester, 'arca-mode-swap-replace');
      await _tap(tester, 'arca-mode-swap-no');

      expect(find.text(en.arcaSwapReplaceConfirmTitle), findsNothing);
      expect(find.text(en.arcaSwapTitle), findsNothing);
      expect(pair.startedSwapReplace, isEmpty);
    });

    testWidgets('leaving the confirmation with the back button is not a yes', (tester) async {
      final (_, pair) = await shown(tester, epochs: [_epoch(1, _hex1)]);

      await _tap(tester, 'arca-mode-swap-replace');
      await _back(tester);

      expect(find.text(en.arcaSwapTitle), findsNothing);
      expect(pair.startedSwapReplace, isEmpty);
    });

    testWidgets('"replace the key" on the confirmation opens the swap screen, and only its start button starts the swap replace', (tester) async {
      final (_, pair) = await shown(tester, epochs: [_epoch(1, _hex1)]);

      await _tap(tester, 'arca-mode-swap-replace');
      await _tap(tester, 'arca-mode-swap-yes');
      expect(find.text(en.arcaSwapTitle), findsOneWidget);
      expect(pair.startedSwapReplace, isEmpty, reason: 'the swap starts on the start button, not on the confirmation');
      await tester.tap(find.byKey(const Key('arca-pair-start')));
      await _settle(tester);

      expect(pair.startedSwapReplace, [
        {'hubUrl': 'https://hub.local:7420', 'deviceToken': 'tok', 'deviceId': 'phone-1234'},
      ]);
    });

    testWidgets('if the newer epoch is gone by the tap, it says so and asks nothing', (tester) async {
      final (backend, _) = await shown(tester, epochs: [_epoch(1, _hex1)]);
      backend.epochs = [];

      await _tap(tester, 'arca-mode-swap-replace');

      expect(find.text(en.arcaSwapNothingPending), findsOneWidget);
      expect(find.text(en.arcaSwapReplaceConfirmTitle), findsNothing);
    });

    testWidgets('the confirmation is in the app\'s language', (tester) async {
      _savedHub();
      final backend = FakeBackend(on: true)..epochs = [_epoch(1, _hex1)];
      await tester.pumpWidget(testApp(home: ArcaModeScreen(backend: backend, pairBackend: FakePairBackend()), locale: const Locale('pt')));
      await _settle(tester);

      await _tap(tester, 'arca-mode-swap-replace');

      expect(find.text(pt.arcaSwapReplaceConfirmTitle), findsOneWidget);
      expect(find.text(pt.arcaSwapReplaceConfirmBody), findsOneWidget);
    });
  });
}

/// A stand-in for the bridge of the swap screens the Arca mode screen opens: records what they start, and never answers.
class FakePairBackend implements ArcaPairBackend {
  final startedSwapGrant = <Map<String, String>>[];
  final startedSwapReplace = <Map<String, String>>[];

  @override
  Future<bridge_pair.ArcaPairRole> role() async => bridge_pair.ArcaPairRole.grant;

  @override
  Future<void> start({required String hubUrl, required String deviceToken, required String deviceId}) async {}

  @override
  Future<void> startGrant({required String hubUrl, required String deviceToken}) async {}

  @override
  Future<void> startSwapGrant({required String hubUrl, required String deviceToken}) async {
    startedSwapGrant.add({'hubUrl': hubUrl, 'deviceToken': deviceToken});
  }

  @override
  Future<void> startSwapReplace({required String hubUrl, required String deviceToken, required String deviceId}) async {
    startedSwapReplace.add({'hubUrl': hubUrl, 'deviceToken': deviceToken, 'deviceId': deviceId});
  }

  @override
  Future<void> pick(int index) async {}

  @override
  Future<bridge_pair.ArcaPairStep> next() => Completer<bridge_pair.ArcaPairStep>().future;

  @override
  Future<void> answer(String typed) async {}

  @override
  Future<void> cancel() async {}
}
