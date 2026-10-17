import 'package:flutter_test/flutter_test.dart';
import 'package:mobile/protocol/messages.dart';
import 'package:mobile/services/live_turn.dart';

void main() {
  test('a long chat answer written bit by bit keeps its start (P157)', () {
    // The model's words come with no source, a few hundred characters at a time; a long answer must not lose its beginning.
    final piece = 'word ' * 100;
    var turn = const LiveTurn();
    for (var i = 0; i < 60; i++) {
      turn = turn.apply(ChatEventText(piece));
    }
    expect(turn.items, hasLength(1));
    final text = (turn.items.single as LiveText).text;
    expect(text.length, piece.length * 60, reason: '6000 characters, past the old 4000 limit');
    expect(text.startsWith('word word'), isTrue);
  });

  test('a block of words is still cut at the limit, keeping the latest', () {
    var turn = const LiveTurn().apply(ChatEventText('x' * liveTextLimit));
    turn = turn.apply(const ChatEventText('END'));
    final text = (turn.items.single as LiveText).text;
    expect(text.length, liveTextLimit);
    expect(text.endsWith('END'), isTrue);
  });
}
