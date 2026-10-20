import 'dart:async';
import 'dart:convert';

import 'package:async/async.dart';
import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:mobile/screens/connection_screen.dart';
import 'package:mobile/screens/folder_picker.dart';
import 'package:mobile/services/server_connection.dart';
import 'package:shared_preferences/shared_preferences.dart';
import 'package:stream_channel/stream_channel.dart';

import '../helpers/test_app.dart';

/// P102 — the working folder picked on the phone: the real `ServerConnection` over an in-process channel
/// (`controller.local` plays warden-server, which lists folders and records the `chat` frames), through the real
/// connection screen and chat screen.
void main() {
  late StreamChannelController<dynamic> controller;
  late StreamQueue<dynamic> fromClient;
  late List<Map<String, dynamic>> chats;

  /// What the hub answers to `listDirs` with no path: a folder to go into, or nothing at all (a member with no folder).
  late Map<String, dynamic> Function(int id) topReply;

  /// What the hub answers to `listNodes` (P102): the nodes the owner may pick a folder on.
  late List<Map<String, dynamic>> nodes;

  Map<String, dynamic> dirList(int id, String path, String? parent, List<List<String>> dirs) => {
        'type': 'dirList',
        'requestId': id,
        'path': path,
        'parent': ?parent,
        'dirs': [for (final d in dirs) {'name': d[0], 'path': d[1]}],
      };

  setUp(() {
    SharedPreferences.setMockInitialValues({});
    controller = StreamChannelController<dynamic>();
    fromClient = StreamQueue<dynamic>(controller.local.stream);
    chats = [];
    nodes = [];
    topReply = (id) => dirList(id, '', null, [
          ['work', '/srv/work']
        ]);
  });

  void hubHears(dynamic raw) {
    final frame = jsonDecode(raw as String) as Map<String, dynamic>;
    final id = frame['requestId'] as int?;
    switch (frame['type']) {
      case 'listDirs':
        final reply = switch (frame['path'] as String?) {
          null => topReply(id!),
          '/srv/work' => dirList(id!, '/srv/work', '', [
              ['alpha', '/srv/work/alpha']
            ]),
          '/srv/work/alpha' => dirList(id!, '/srv/work/alpha', '/srv/work', []),
          'node:node-a:' => dirList(id!, 'node:node-a:', null, [
              ['web', 'node:node-a:web']
            ]),
          'node:node-a:web' => dirList(id!, 'node:node-a:web', 'node:node-a:', []),
          _ => {'type': 'dirError', 'requestId': id, 'message': 'that folder is not one of yours'},
        };
        controller.local.sink.add(jsonEncode(reply));
      case 'listNodes':
        controller.local.sink.add(jsonEncode({'type': 'nodeList', 'requestId': id, 'nodes': nodes}));
      case 'chat':
        chats.add(frame);
    }
  }

  Future<ServerConnection> fakeConnector({
    required String host,
    required int port,
    bool secure = false,
    required String deviceId,
    required String deviceName,
    required String authKey,
    String? deviceToken,
    String? username,
    String? password,
    Duration handshakeTimeout = ServerConnection.defaultHandshakeTimeout,
    List<Map<String, dynamic>> toolSpecs = const [],
    Map<String, ToolHandler> toolHandlers = const {},
  }) {
    unawaited(fromClient.next.then((_) {
      controller.local.sink.add('{"type":"helloAck","serverName":"test-hub"}');
      fromClient.rest.listen(hubHears);
    }));
    return ServerConnection.connectOverChannel(
      channel: controller.foreign,
      deviceId: deviceId,
      deviceName: deviceName,
      authKey: authKey,
    );
  }

  /// Real async (the channel's streams) between frames of the widget tree.
  Future<void> hubRound(WidgetTester tester) async {
    await tester.runAsync(() => Future<void>.delayed(const Duration(milliseconds: 80)));
    await tester.pump();
  }

  Future<void> openChat(WidgetTester tester) async {
    await tester.runAsync(() async {});
    await tester.pumpWidget(testApp(home: ConnectionScreen(connector: fakeConnector)));
    await tester.pump();
    await tester.enterText(find.widgetWithText(TextField, 'Server host'), '10.0.0.1');
    await tester.enterText(find.widgetWithText(TextField, 'Auth key'), 'secret');
    await tester.tap(find.widgetWithText(FilledButton, 'Connect'));
    await tester.runAsync(() => Future<void>.delayed(const Duration(milliseconds: 100)));
    await tester.pumpAndSettle();
    expect(find.widgetWithText(AppBar, 'test-hub'), findsOneWidget);
  }

  testWidgets('a folder is browsed, picked before the first message, travels with it and is then only shown', (tester) async {
    await openChat(tester);
    expect(find.text('Working folder: none'), findsOneWidget);

    await tester.tap(find.byKey(const Key('folder-button')));
    await hubRound(tester);
    await tester.pumpAndSettle();
    // The top of the list has no path to pick, only the folders allowed.
    expect(find.text('Your folders'), findsOneWidget);
    expect(tester.widget<FilledButton>(find.byKey(const Key('folder-use'))).onPressed, isNull);
    expect(find.text('work'), findsOneWidget);

    await tester.tap(find.text('work'));
    await hubRound(tester);
    expect(tester.widget<Text>(find.byKey(const Key('folder-here'))).data, '/srv/work');
    await tester.tap(find.text('alpha'));
    await hubRound(tester);
    expect(tester.widget<Text>(find.byKey(const Key('folder-here'))).data, '/srv/work/alpha');
    expect(find.byKey(const Key('folder-empty')), findsOneWidget);

    // Up goes back one folder; a parent that is empty is the top of the list.
    await tester.tap(find.byKey(const Key('folder-up')));
    await hubRound(tester);
    expect(tester.widget<Text>(find.byKey(const Key('folder-here'))).data, '/srv/work');
    await tester.tap(find.byKey(const Key('folder-up')));
    await hubRound(tester);
    expect(find.text('Your folders'), findsOneWidget);

    await tester.tap(find.text('work'));
    await hubRound(tester);
    await tester.tap(find.text('alpha'));
    await hubRound(tester);
    await tester.tap(find.byKey(const Key('folder-use')));
    await tester.pumpAndSettle();
    expect(find.text('alpha'), findsOneWidget, reason: 'the chip shows the folder name');

    await tester.enterText(find.byType(TextField), 'organize this');
    await tester.tap(find.byIcon(Icons.send));
    await hubRound(tester);
    expect(chats, hasLength(1));
    expect(chats.single['message'], 'organize this');
    expect(chats.single['workdir'], '/srv/work/alpha', reason: 'the first message carries the folder it starts in');

    // Once the conversation exists the folder is only shown: no button to pick another, no way to clear it.
    expect(find.byKey(const Key('folder-button')), findsNothing);
    expect(find.byKey(const Key('folder-clear')), findsNothing);
    expect(find.byKey(const Key('folder-chip')), findsOneWidget);
  });

  testWidgets('cancelling and clearing leave no folder, and a conversation without one sends none', (tester) async {
    await openChat(tester);

    await tester.tap(find.byKey(const Key('folder-button')));
    await hubRound(tester);
    await tester.pumpAndSettle();
    await tester.tap(find.text('Cancel'));
    await tester.pumpAndSettle();
    expect(find.text('Working folder: none'), findsOneWidget);

    await tester.tap(find.byKey(const Key('folder-button')));
    await hubRound(tester);
    await tester.pumpAndSettle();
    await tester.tap(find.text('work'));
    await hubRound(tester);
    await tester.tap(find.byKey(const Key('folder-use')));
    await tester.pumpAndSettle();
    expect(find.text('work'), findsOneWidget);
    await tester.tap(find.byKey(const Key('folder-clear')));
    await tester.pumpAndSettle();
    expect(find.text('Working folder: none'), findsOneWidget);

    await tester.enterText(find.byType(TextField), 'hi');
    await tester.tap(find.byIcon(Icons.send));
    await hubRound(tester);
    expect(chats.single['message'], 'hi');
    expect(chats.single.containsKey('workdir'), isFalse);
  });

  testWidgets('the owner picks a folder on a node: the machine selector, the node by name, the chip and the first message (P102)', (tester) async {
    nodes = [
      {'deviceId': 'node-a', 'name': 'Home PC', 'online': true, 'approved': true, 'enabled': true, 'offer': {'files': true}},
      {'deviceId': 'node-b', 'name': 'Offline PC', 'online': false, 'approved': true, 'enabled': true, 'offer': {'files': true}},
    ];
    await openChat(tester);

    await tester.tap(find.byKey(const Key('folder-button')));
    await hubRound(tester);
    await hubRound(tester);
    await tester.pumpAndSettle();
    expect(find.byKey(const Key('folder-machine')), findsOneWidget);

    await tester.tap(find.byKey(const Key('folder-machine')));
    await tester.pumpAndSettle();
    expect(find.text('Offline PC'), findsNothing, reason: 'only a node a folder can be picked on now');
    await tester.tap(find.text('Home PC').last);
    await hubRound(tester);
    await tester.pumpAndSettle();
    expect(tester.widget<Text>(find.byKey(const Key('folder-here'))).data, 'Home PC: /', reason: 'the machine by name, not the raw reference');
    expect(find.byKey(const Key('folder-up')), findsNothing, reason: 'the top of what the node lends');

    await tester.tap(find.text('web'));
    await hubRound(tester);
    expect(tester.widget<Text>(find.byKey(const Key('folder-here'))).data, 'Home PC: /web');
    await tester.tap(find.byKey(const Key('folder-use')));
    await tester.pumpAndSettle();
    expect(find.text('web · Home PC'), findsOneWidget, reason: 'the chip names the folder and the machine');

    await tester.enterText(find.byType(TextField), 'work there');
    await tester.tap(find.byIcon(Icons.send));
    await hubRound(tester);
    expect(chats.single['workdir'], 'node:node-a:web');
  });

  testWidgets('with no node to pick on the machine selector is not shown (P102)', (tester) async {
    await openChat(tester);

    await tester.tap(find.byKey(const Key('folder-button')));
    await hubRound(tester);
    await hubRound(tester);
    await tester.pumpAndSettle();
    expect(find.text('Your folders'), findsOneWidget);
    expect(find.byKey(const Key('folder-machine')), findsNothing);
  });

  testWidgets('a member with no folder allowed gets an empty list that cannot be used, not a failure', (tester) async {
    topReply = (id) => dirList(id, '', null, []);
    await openChat(tester);

    await tester.tap(find.byKey(const Key('folder-button')));
    await hubRound(tester);
    await tester.pumpAndSettle();
    expect(find.byKey(const Key('folder-empty')), findsOneWidget);
    expect(find.byKey(const Key('folder-error')), findsNothing);
    expect(tester.widget<FilledButton>(find.byKey(const Key('folder-use'))).onPressed, isNull);
  });

  testWidgets('a refusal from the hub is shown in the picker', (tester) async {
    topReply = (id) => {'type': 'dirError', 'requestId': id, 'message': 'the hub could not list its folders'};
    await openChat(tester);

    await tester.tap(find.byKey(const Key('folder-button')));
    await hubRound(tester);
    await tester.pumpAndSettle();
    expect(find.byKey(const Key('folder-error')), findsOneWidget);
    expect(find.textContaining('could not list its folders'), findsOneWidget);
    expect(tester.widget<FilledButton>(find.byKey(const Key('folder-use'))).onPressed, isNull);
  });

  test('folderName is the last segment of the path', () {
    expect(folderName('/srv/work/alpha'), 'alpha');
    expect(folderName('/srv/work/alpha/'), 'alpha');
    expect(folderName('/'), '/');
  });
}
