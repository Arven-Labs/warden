import 'dart:async';
import 'dart:typed_data';

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
import 'package:mobile/src/rust/api/arca_rotation.dart' as bridge_rot;
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

  /// The phrase the bridge "shows": `word01` .. `word24`, and the positions (counting from 1) the person is asked to type back.
  bridge_rot.RotationStartDto rotationStart = bridge_rot.RotationStartDto(phrase: [for (var i = 1; i <= 24; i++) 'word${i.toString().padLeft(2, '0')}'], positions: Uint32List.fromList([3, 11, 20]));
  bridge_rot.ArcaRotationFailure? beginFailure;
  bridge_rot.ArcaRotationFailure? resumeFailure;
  bridge_rot.ArcaRotationFailure? confirmFailure;
  int beginCalls = 0;
  int cancelCalls = 0;
  final resumed = <String>[];
  final confirmed = <({List<String> typedWords, bool revokedOnHub})>[];

  @override
  Future<bridge_rot.RotationStartDto> rotationBegin() async {
    beginCalls++;
    final failure = beginFailure;
    if (failure != null) throw failure;
    return rotationStart;
  }

  @override
  Future<void> rotationResume(String phrase) async {
    resumed.add(phrase);
    final failure = resumeFailure;
    if (failure != null) throw failure;
  }

  @override
  Future<void> rotationConfirm({required List<String> typedWords, required bool revokedOnHub}) async {
    confirmed.add((typedWords: List.of(typedWords), revokedOnHub: revokedOnHub));
    final failure = confirmFailure;
    if (failure != null) throw failure;
  }

  @override
  Future<void> rotationCancel() async {
    cancelCalls++;
  }
}

bridge_rot.ArcaRotationFailure _rotationFailure(bridge_rot.ArcaRotationErrorKind kind) => bridge_rot.ArcaRotationFailure(kind: kind, detail: 'english detail SECRET');

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

  group('arcaRotationErrorText', () {
    test('has a sentence for every kind, in both languages, and it is translated', () {
      for (final kind in bridge_rot.ArcaRotationErrorKind.values) {
        expect(arcaRotationErrorText(en, kind), isNotEmpty, reason: 'en $kind');
        expect(arcaRotationErrorText(pt, kind), isNotEmpty, reason: 'pt $kind');
        expect(arcaRotationErrorText(pt, kind), isNot(arcaRotationErrorText(en, kind)), reason: '$kind is translated');
      }
    });

    test('tells the kinds apart, so the person is not told the wrong thing', () {
      final texts = bridge_rot.ArcaRotationErrorKind.values.map((k) => arcaRotationErrorText(en, k)).toSet();
      expect(texts.length, bridge_rot.ArcaRotationErrorKind.values.length);
    });
  });

  group('the key rotation (P-49)', () {
    // The phrase and the three fields are taller than the default test window: a tall one keeps everything built and tappable.
    Future<FakeBackend> shown(WidgetTester tester, {bool on = true, List<bridge.PendingEpochDto>? epochs, Locale locale = const Locale('en')}) async {
      tester.view.physicalSize = const Size(800, 3200);
      tester.view.devicePixelRatio = 1;
      addTearDown(tester.view.reset);
      _savedHub();
      final backend = FakeBackend(on: on)..epochs = epochs ?? [];
      await tester.pumpWidget(testApp(home: ArcaModeScreen(backend: backend, pairBackend: FakePairBackend()), locale: locale));
      await _settle(tester);
      return backend;
    }

    // Longer than `_settle`: a route has to finish its transition.
    Future<void> settleRoute(WidgetTester tester) async {
      for (var i = 0; i < 10; i++) {
        await tester.pump(const Duration(milliseconds: 100));
      }
    }

    Future<void> tapRoute(WidgetTester tester, String key) async {
      await tester.tap(find.byKey(Key(key)));
      await settleRoute(tester);
    }

    // The person types: a character at a time (the fields refuse a block that arrives at once, see below).
    Future<void> typeInto(WidgetTester tester, String key, String text) async {
      for (var i = 1; i <= text.length; i++) {
        await tester.enterText(find.byKey(Key(key)), text.substring(0, i));
      }
      await tester.pump();
    }

    /// The words the screen asks for, read off what the fake showed.
    List<String> asked(FakeBackend backend) => [for (final position in backend.rotationStart.positions) backend.rotationStart.phrase[position - 1]];

    Future<void> openAndBegin(WidgetTester tester, FakeBackend backend) async {
      await tapRoute(tester, 'arca-mode-rotate');
      await tapRoute(tester, 'arca-rotation-begin');
    }

    Future<void> fillAndDeclare(WidgetTester tester, FakeBackend backend, {bool declare = true}) async {
      final words = asked(backend);
      for (var i = 0; i < words.length; i++) {
        await typeInto(tester, 'arca-rotation-word-$i', words[i]);
      }
      if (declare) await tapRoute(tester, 'arca-rotation-revoked');
    }

    testWidgets('with the mode on, a hub and no newer key pending, the device can start a rotation', (tester) async {
      await shown(tester);

      expect(find.byKey(const Key('arca-mode-rotate')), findsOneWidget);
    });

    testWidgets('no way to rotate with the mode off, without a usable hub, or while a newer key is pending', (tester) async {
      await shown(tester, on: false);
      expect(find.byKey(const Key('arca-mode-rotate')), findsNothing, reason: 'mode off');

      await shown(tester, epochs: [_epoch(1, _hex1)]);
      expect(find.byKey(const Key('arca-mode-rotate')), findsNothing, reason: 'this device holds the old key: it is the one that must replace it');

      SharedPreferences.setMockInitialValues({'connection.host': 'hub.local', 'connection.port': 7420, 'connection.authKey': 'key', 'connection.useTls': true});
      final backend = FakeBackend(on: true);
      await tester.pumpWidget(testApp(home: ArcaModeScreen(backend: backend, pairBackend: FakePairBackend())));
      await _settle(tester);
      expect(find.byKey(const Key('arca-mode-rotate')), findsNothing, reason: 'no token for the hub');
    });

    testWidgets('opening the screen generates nothing: the key is made only on "begin"', (tester) async {
      final backend = await shown(tester);

      await tapRoute(tester, 'arca-mode-rotate');

      expect(backend.beginCalls, 0);
      expect(find.byKey(const Key('arca-rotation-begin')), findsOneWidget);
      expect(find.text('word01'), findsNothing);
    });

    testWidgets('begin shows the 24 words once, numbered, with a warning about screenshots, and asks for exactly 3 words', (tester) async {
      final backend = await shown(tester);

      await openAndBegin(tester, backend);

      expect(backend.beginCalls, 1);
      for (var i = 1; i <= 24; i++) {
        expect(find.text('$i. word${i.toString().padLeft(2, '0')}'), findsOneWidget, reason: 'word $i');
      }
      expect(find.byKey(const Key('arca-rotation-capture-warning')), findsOneWidget);
      expect(find.byKey(const Key('arca-rotation-word-0')), findsOneWidget);
      expect(find.byKey(const Key('arca-rotation-word-1')), findsOneWidget);
      expect(find.byKey(const Key('arca-rotation-word-2')), findsOneWidget);
      expect(find.byKey(const Key('arca-rotation-word-3')), findsNothing);
    });

    testWidgets('a double tap on begin asks the bridge for one key', (tester) async {
      final backend = await shown(tester);
      await tapRoute(tester, 'arca-mode-rotate');

      final onPressed = tester.widget<FilledButton>(find.byKey(const Key('arca-rotation-begin'))).onPressed!;
      onPressed();
      onPressed();
      await settleRoute(tester);

      expect(backend.beginCalls, 1);
    });

    testWidgets('a refusal to begin says why in the app\'s words, never the English detail, and shows no phrase', (tester) async {
      final backend = await shown(tester);
      backend.beginFailure = _rotationFailure(bridge_rot.ArcaRotationErrorKind.notInArcaMode);
      await tapRoute(tester, 'arca-mode-rotate');

      await tapRoute(tester, 'arca-rotation-begin');

      expect(find.text(arcaRotationErrorText(en, bridge_rot.ArcaRotationErrorKind.notInArcaMode)), findsOneWidget);
      expect(find.textContaining('SECRET'), findsNothing);
      expect(find.text('1. word01'), findsNothing);
    });

    testWidgets('the confirm button stays off until the lost device\'s token is declared revoked and all three words are typed', (tester) async {
      final backend = await shown(tester);
      await openAndBegin(tester, backend);
      bool enabled() => tester.widget<FilledButton>(find.byKey(const Key('arca-rotation-confirm'))).onPressed != null;
      expect(enabled(), isFalse, reason: 'nothing typed, nothing declared');

      await fillAndDeclare(tester, backend, declare: false);
      expect(enabled(), isFalse, reason: 'words typed but not declared');

      await tapRoute(tester, 'arca-rotation-revoked');
      expect(enabled(), isTrue);

      await tester.enterText(find.byKey(const Key('arca-rotation-word-1')), '');
      await tester.pump();
      expect(enabled(), isFalse, reason: 'a word was erased');
    });

    testWidgets('the word fields take only typing: no paste, no suggestions, no autofill, and a block that arrives at once is refused', (tester) async {
      final backend = await shown(tester);
      await openAndBegin(tester, backend);

      final field = tester.widget<TextField>(find.byKey(const Key('arca-rotation-word-0')));
      expect(field.enableSuggestions, isFalse);
      expect(field.autocorrect, isFalse);
      expect(field.enableInteractiveSelection, isFalse);
      expect(field.enableIMEPersonalizedLearning, isFalse);
      await tester.enterText(find.byKey(const Key('arca-rotation-word-0')), 'word03');
      expect(tester.widget<TextField>(find.byKey(const Key('arca-rotation-word-0'))).controller!.text, isEmpty, reason: 'six characters at once is a paste, not typing');
    });

    testWidgets('confirm asks first: "not now" and the back button are not a yes, and nothing is sent', (tester) async {
      final backend = await shown(tester);
      await openAndBegin(tester, backend);
      await fillAndDeclare(tester, backend);

      await tapRoute(tester, 'arca-rotation-confirm');
      expect(find.byKey(const Key('arca-rotation-confirm-yes')), findsOneWidget);
      await tapRoute(tester, 'arca-rotation-confirm-no');
      expect(backend.confirmed, isEmpty);

      await tapRoute(tester, 'arca-rotation-confirm');
      await _back(tester);
      expect(backend.confirmed, isEmpty);
      expect(find.byKey(const Key('arca-rotation-confirm-yes')), findsNothing);
      expect(find.text('1. word01'), findsOneWidget, reason: 'the phrase is still there to write down');
    });

    testWidgets('a "yes" confirms with the typed words and the declaration, closes the screen, and syncs with the saved hub so the new epoch goes out', (tester) async {
      final backend = await shown(tester);
      await openAndBegin(tester, backend);
      await fillAndDeclare(tester, backend);

      await tapRoute(tester, 'arca-rotation-confirm');
      await tapRoute(tester, 'arca-rotation-confirm-yes');

      expect(backend.confirmed.length, 1);
      expect(backend.confirmed.single.typedWords, asked(backend));
      expect(backend.confirmed.single.revokedOnHub, isTrue);
      expect(find.byKey(const Key('arca-rotation-begin')), findsNothing, reason: 'back on the Arca mode screen');
      expect(find.text('1. word01'), findsNothing, reason: 'the phrase is gone with the screen');
      expect(backend.syncCalls, [
        {'hubUrl': 'https://hub.local:7420', 'deviceToken': 'tok', 'allowInsecureHttp': false},
      ]);
    });

    testWidgets('a wrong word: the error is shown, the phrase is gone, nothing is synced, and the person must begin again with a new key', (tester) async {
      final backend = await shown(tester);
      backend.confirmFailure = _rotationFailure(bridge_rot.ArcaRotationErrorKind.wordMismatch);
      await openAndBegin(tester, backend);
      await fillAndDeclare(tester, backend);

      await tapRoute(tester, 'arca-rotation-confirm');
      await tapRoute(tester, 'arca-rotation-confirm-yes');

      expect(find.text(arcaRotationErrorText(en, bridge_rot.ArcaRotationErrorKind.wordMismatch)), findsOneWidget);
      expect(find.text('1. word01'), findsNothing, reason: 'the key was dropped, so its phrase must not stay on screen');
      expect(find.byKey(const Key('arca-rotation-begin')), findsOneWidget);
      expect(backend.syncCalls, isEmpty);
    });

    testWidgets('a failure that may have written something opens "resume" with the same phrase to type, and does not make a new key', (tester) async {
      final backend = await shown(tester);
      backend.confirmFailure = _rotationFailure(bridge_rot.ArcaRotationErrorKind.failed);
      await openAndBegin(tester, backend);
      await fillAndDeclare(tester, backend);

      await tapRoute(tester, 'arca-rotation-confirm');
      await tapRoute(tester, 'arca-rotation-confirm-yes');

      expect(find.byKey(const Key('arca-rotation-resume-field')), findsOneWidget);
      expect(find.text(arcaRotationErrorText(en, bridge_rot.ArcaRotationErrorKind.failed)), findsOneWidget);
      expect(find.byKey(const Key('arca-rotation-begin')), findsNothing, reason: 'a new key would not finish the stopped rotation');
      expect(backend.beginCalls, 1);
      expect(backend.syncCalls, isEmpty);
    });

    testWidgets('resuming: the 24 words typed are resumed and confirmed with no words asked back, then the screen closes and syncs', (tester) async {
      final backend = await shown(tester);
      await tapRoute(tester, 'arca-mode-rotate');
      await tapRoute(tester, 'arca-rotation-resume-open');
      final phrase = backend.rotationStart.phrase.join(' ');

      await typeInto(tester, 'arca-rotation-resume-field', phrase);
      await tapRoute(tester, 'arca-rotation-revoked');
      await tapRoute(tester, 'arca-rotation-resume-confirm');
      await tapRoute(tester, 'arca-rotation-confirm-yes');

      expect(backend.resumed, [phrase]);
      expect(backend.confirmed.length, 1);
      expect(backend.confirmed.single.typedWords, isEmpty, reason: 'a resume asks no words back');
      expect(backend.confirmed.single.revokedOnHub, isTrue);
      expect(backend.syncCalls.length, 1);
    });

    testWidgets('the resume field takes only typing too, and a phrase the bridge refuses is told as such and confirms nothing', (tester) async {
      final backend = await shown(tester);
      backend.resumeFailure = _rotationFailure(bridge_rot.ArcaRotationErrorKind.badPhrase);
      await tapRoute(tester, 'arca-mode-rotate');
      await tapRoute(tester, 'arca-rotation-resume-open');

      await tester.enterText(find.byKey(const Key('arca-rotation-resume-field')), backend.rotationStart.phrase.join(' '));
      expect(tester.widget<TextField>(find.byKey(const Key('arca-rotation-resume-field'))).controller!.text, isEmpty, reason: 'a pasted block is refused');

      await typeInto(tester, 'arca-rotation-resume-field', 'not a valid phrase');
      await tapRoute(tester, 'arca-rotation-revoked');
      await tapRoute(tester, 'arca-rotation-resume-confirm');
      await tapRoute(tester, 'arca-rotation-confirm-yes');

      expect(find.text(arcaRotationErrorText(en, bridge_rot.ArcaRotationErrorKind.badPhrase)), findsOneWidget);
      expect(backend.confirmed, isEmpty);
      expect(backend.syncCalls, isEmpty);
    });

    testWidgets('every other failure shows its own sentence and never the English detail', (tester) async {
      for (final kind in [bridge_rot.ArcaRotationErrorKind.notDeclared, bridge_rot.ArcaRotationErrorKind.noRotation, bridge_rot.ArcaRotationErrorKind.other]) {
        // A fresh tree: the screen of the last turn must not stay open on top of the next one.
        await tester.pumpWidget(const SizedBox());
        final backend = await shown(tester);
        backend.confirmFailure = _rotationFailure(kind);
        await openAndBegin(tester, backend);
        await fillAndDeclare(tester, backend);

        await tapRoute(tester, 'arca-rotation-confirm');
        await tapRoute(tester, 'arca-rotation-confirm-yes');

        expect(find.text(arcaRotationErrorText(en, kind)), findsOneWidget, reason: '$kind');
        expect(find.textContaining('SECRET'), findsNothing, reason: '$kind');
        expect(backend.syncCalls, isEmpty, reason: '$kind');
      }
    });

    testWidgets('cancelling drops the key in the bridge and wipes the phrase from the screen', (tester) async {
      final backend = await shown(tester);
      await openAndBegin(tester, backend);

      await tapRoute(tester, 'arca-rotation-cancel');

      expect(backend.cancelCalls, greaterThan(0));
      expect(find.text('1. word01'), findsNothing);
      expect(backend.confirmed, isEmpty);
    });

    testWidgets('leaving the screen with the phrase showing drops the key in the bridge', (tester) async {
      final backend = await shown(tester);
      await openAndBegin(tester, backend);

      await _back(tester);
      await settleRoute(tester);

      expect(backend.cancelCalls, greaterThan(0));
      expect(find.text('1. word01'), findsNothing);
    });

    testWidgets('a double tap on the final "yes" confirms once', (tester) async {
      final backend = await shown(tester);
      await openAndBegin(tester, backend);
      await fillAndDeclare(tester, backend);
      await tapRoute(tester, 'arca-rotation-confirm');

      final onPressed = tester.widget<FilledButton>(find.byKey(const Key('arca-rotation-confirm-yes'))).onPressed!;
      onPressed();
      onPressed();
      await settleRoute(tester);

      expect(backend.confirmed.length, 1);
    });

    testWidgets('the screen speaks Portuguese when the app does', (tester) async {
      final backend = await shown(tester, locale: const Locale('pt'));
      backend.beginFailure = _rotationFailure(bridge_rot.ArcaRotationErrorKind.notInArcaMode);
      await tapRoute(tester, 'arca-mode-rotate');
      await tapRoute(tester, 'arca-rotation-begin');

      expect(find.text(arcaRotationErrorText(pt, bridge_rot.ArcaRotationErrorKind.notInArcaMode)), findsOneWidget);
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
