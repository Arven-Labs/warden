import '../protocol/messages.dart';

/// P157 — how the drawer lists the conversations: the pinned ones first, the rest by how recently they were used, and the archived
/// ones apart. Pure, so it can be tested without a screen. The same rule as the desktop's and the web's.

/// One block of the list: its label and its conversations, in the order they came.
class ConversationBucket {
  const ConversationBucket(this.label, this.items);

  final String label;
  final List<ConversationSummary> items;
}

class GroupedConversations {
  const GroupedConversations({required this.pinned, required this.buckets, required this.archived});

  final List<ConversationSummary> pinned;
  final List<ConversationBucket> buckets;
  final List<ConversationSummary> archived;
}

const _labels = ['Today', 'Yesterday', 'Previous 7 days', 'Previous 30 days', 'Older'];

/// Which block a conversation last used at [updatedAt] (milliseconds) goes in. Days count on the calendar of this device, not as 24
/// hours back: "Yesterday" is the whole of yesterday.
String bucketLabel(int updatedAt, DateTime now) {
  DateTime startOfDay(int daysBack) => DateTime(now.year, now.month, now.day - daysBack);
  final at = DateTime.fromMillisecondsSinceEpoch(updatedAt);
  if (!at.isBefore(startOfDay(0))) return _labels[0];
  if (!at.isBefore(startOfDay(1))) return _labels[1];
  if (!at.isBefore(startOfDay(7))) return _labels[2];
  if (!at.isBefore(startOfDay(30))) return _labels[3];
  return _labels[4];
}

/// Splits [conversations] into the pinned, the blocks by date and the archived. The order each came in is kept, and a block with
/// nothing in it is left out. An archived conversation is only ever in `archived`, whatever else it says.
GroupedConversations groupConversations(List<ConversationSummary> conversations, DateTime now) {
  final pinned = <ConversationSummary>[];
  final archived = <ConversationSummary>[];
  final byLabel = <String, List<ConversationSummary>>{};
  for (final c in conversations) {
    if (c.archived) {
      archived.add(c);
    } else if (c.pinned) {
      pinned.add(c);
    } else {
      byLabel.putIfAbsent(bucketLabel(c.updatedAt, now), () => []).add(c);
    }
  }
  return GroupedConversations(
    pinned: pinned,
    buckets: [for (final label in _labels) if (byLabel.containsKey(label)) ConversationBucket(label, byLabel[label]!)],
    archived: archived,
  );
}
