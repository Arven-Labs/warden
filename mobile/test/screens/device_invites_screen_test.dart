import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:mobile/protocol/messages.dart' show PendingDeviceInvite;
import 'package:mobile/screens/device_invites_screen.dart';
import 'package:mobile/services/device_invites.dart';
import 'package:mobile/services/server_connection.dart' show HubRequestException;

/// A hub in memory: it checks the pairing key like the real one and records what the screen asked for.
class _FakeBackend implements DeviceInvitesBackend {
  _FakeBackend({List<String> waiting = const []}) : _waiting = List.of(waiting);

  static const pairingKey = 'right-key';

  final invites = <(String, int)>[];
  final List<String> _waiting;
  var rotations = 0;

  List<PendingDeviceInvite> get _pending =>
      [for (final name in _waiting) PendingDeviceInvite(name: name, createdMs: 1, expiresMs: DateTime(2026, 10, 13, 12, 30).millisecondsSinceEpoch)];

  @override
  Future<List<PendingDeviceInvite>> listDeviceInvites() async => _pending;

  @override
  Future<List<PendingDeviceInvite>> cancelDeviceInvite(String key, String name) async {
    if (key != pairingKey) throw const HubRequestException('wrong key', authRejected: true);
    if (!_waiting.remove(name)) throw HubRequestException("no invite called '$name' is waiting");
    return _pending;
  }

  @override
  Future<DeviceInvite> createDeviceInvite(String key, String name, int ttlMinutes) async {
    if (key != pairingKey) throw const HubRequestException('wrong key', authRejected: true);
    invites.add((name, ttlMinutes));
    _waiting.add(name);
    return DeviceInvite(name: name, code: 'code-for-$name', expiresMs: DateTime(2026, 10, 13, 12, 30).millisecondsSinceEpoch);
  }

  @override
  Future<String> rotatePairingKey(String key) async {
    if (key != pairingKey) throw const HubRequestException('wrong key', authRejected: true);
    rotations++;
    return 'brand-new-key';
  }
}

Future<void> _open(WidgetTester tester, _FakeBackend backend) async {
  await tester.pumpWidget(MaterialApp(home: DeviceInvitesScreen(backend: backend)));
}

Future<void> _typeKey(WidgetTester tester, String key) async {
  await tester.enterText(find.byKey(const Key('pairing-key-field')), key);
  await tester.pump();
  await tester.tap(find.byKey(const Key('pairing-key-confirm')));
  await tester.pumpAndSettle();
}

void main() {
  testWidgets('an invite is made with the pairing key and its code shows once', (tester) async {
    final backend = _FakeBackend();
    await _open(tester, backend);

    await tester.enterText(find.byKey(const Key('invite-name')), "Ana's phone");
    await tester.enterText(find.byKey(const Key('invite-minutes')), '30');
    await tester.tap(find.byKey(const Key('invite-create')));
    await tester.pumpAndSettle();

    await _typeKey(tester, 'wrong');
    expect(find.text('Wrong pairing key.'), findsOneWidget, reason: 'a wrong key keeps the dialog open');
    expect(backend.invites, isEmpty);

    await _typeKey(tester, _FakeBackend.pairingKey);
    expect(backend.invites.single, ("Ana's phone", 30));
    expect(find.byKey(const Key('invite-code')), findsOneWidget);
    expect(find.text("code-for-Ana's phone"), findsOneWidget);
  });

  testWidgets('an invite needs a name and a validity, before any key is asked', (tester) async {
    final backend = _FakeBackend();
    await _open(tester, backend);

    await tester.tap(find.byKey(const Key('invite-create')));
    await tester.pumpAndSettle();

    expect(find.byKey(const Key('devices-error')), findsOneWidget);
    expect(find.byKey(const Key('pairing-key-field')), findsNothing);
    expect(backend.invites, isEmpty);
  });

  testWidgets('the invites that wait are listed on opening, and a new one joins the list', (tester) async {
    final backend = _FakeBackend(waiting: ['Bia']);
    await _open(tester, backend);
    await tester.pumpAndSettle();
    expect(find.byKey(const Key('waiting-Bia')), findsOneWidget);

    await tester.enterText(find.byKey(const Key('invite-name')), 'Ana');
    await tester.tap(find.byKey(const Key('invite-create')));
    await tester.pumpAndSettle();
    await _typeKey(tester, _FakeBackend.pairingKey);

    expect(find.byKey(const Key('waiting-Ana')), findsOneWidget);
    expect(find.byKey(const Key('waiting-Bia')), findsOneWidget);
  });

  testWidgets('cancelling an invite asks for the pairing key and takes it off the list', (tester) async {
    final backend = _FakeBackend(waiting: ['Ana', 'Bia']);
    await _open(tester, backend);
    await tester.pumpAndSettle();

    await tester.tap(find.byKey(const Key('cancel-Ana')));
    await tester.pumpAndSettle();
    await _typeKey(tester, 'wrong');
    expect(find.text('Wrong pairing key.'), findsOneWidget, reason: 'a wrong key keeps the dialog open');
    expect(find.byKey(const Key('waiting-Ana')), findsOneWidget);

    await _typeKey(tester, _FakeBackend.pairingKey);
    expect(find.byKey(const Key('waiting-Ana')), findsNothing);
    expect(find.byKey(const Key('waiting-Bia')), findsOneWidget);
    expect(find.byKey(const Key('invite-cancelled')), findsOneWidget);
  });

  testWidgets('changing the pairing key shows the new one once', (tester) async {
    final backend = _FakeBackend();
    await _open(tester, backend);

    await tester.tap(find.byKey(const Key('rotate-key')));
    await tester.pumpAndSettle();
    await _typeKey(tester, _FakeBackend.pairingKey);

    expect(backend.rotations, 1);
    expect(find.text('brand-new-key'), findsOneWidget);
  });
}
