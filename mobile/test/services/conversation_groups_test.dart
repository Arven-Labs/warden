import 'package:flutter_test/flutter_test.dart';
import 'package:mobile/protocol/messages.dart';
import 'package:mobile/services/conversation_groups.dart';

// Noon on a Wednesday, on this machine's clock: whatever the time zone, the days around it are the calendar's.
final now = DateTime(2026, 10, 14, 12);

int at(int daysAgo, [int hour = 9]) => DateTime(2026, 10, 14 - daysAgo, hour).millisecondsSinceEpoch;

ConversationSummary conversation(String id, int updatedAt, {bool pinned = false, bool archived = false}) =>
    ConversationSummary(id: id, title: id, createdAt: 1, updatedAt: updatedAt, pinned: pinned, archived: archived);

List<String> ids(List<ConversationSummary> items) => [for (final c in items) c.id];

void main() {
  group('how recent a conversation is', () {
    test('counts on the calendar: this morning is today, last night is yesterday', () {
      expect(bucketLabel(at(0, 0), now), 'Today');
      expect(bucketLabel(at(0, 11), now), 'Today');
      expect(bucketLabel(DateTime(2026, 10, 13, 23, 59, 59).millisecondsSinceEpoch, now), 'Yesterday');
      expect(bucketLabel(at(1, 0), now), 'Yesterday');
    });

    test('goes back through the week and the month and then is older', () {
      expect(bucketLabel(at(2), now), 'Previous 7 days');
      expect(bucketLabel(at(7, 0), now), 'Previous 7 days', reason: 'the seven days before yesterday: 2 to 7 days back');
      expect(bucketLabel(at(8, 23), now), 'Previous 30 days');
      expect(bucketLabel(at(30, 0), now), 'Previous 30 days');
      expect(bucketLabel(at(31, 23), now), 'Older');
      expect(bucketLabel(0, now), 'Older');
    });
  });

  group('the grouped list', () {
    test('puts the pinned first, the rest in blocks by date, the archived apart, and leaves empty blocks out', () {
      final grouped = groupConversations([
        conversation('new', at(0)),
        conversation('pin', at(40), pinned: true),
        conversation('yes', at(1)),
        conversation('old', at(90)),
        conversation('shelf', at(0), archived: true),
        conversation('week', at(3)),
        conversation('new2', at(0, 8)),
      ], now);
      expect(ids(grouped.pinned), ['pin']);
      expect([for (final b in grouped.buckets) b.label], ['Today', 'Yesterday', 'Previous 7 days', 'Older']);
      expect([for (final b in grouped.buckets) ids(b.items).join(',')], ['new,new2', 'yes', 'week', 'old']);
      expect(ids(grouped.archived), ['shelf']);
    });

    test('an archived conversation is only in the archived, even when it says it is pinned too', () {
      final grouped = groupConversations([conversation('both', at(0), pinned: true, archived: true)], now);
      expect(grouped.pinned, isEmpty);
      expect(grouped.buckets, isEmpty);
      expect(ids(grouped.archived), ['both']);
    });

    test('an empty list is three empty parts', () {
      final grouped = groupConversations(const [], now);
      expect(grouped.pinned, isEmpty);
      expect(grouped.buckets, isEmpty);
      expect(grouped.archived, isEmpty);
    });
  });
}
