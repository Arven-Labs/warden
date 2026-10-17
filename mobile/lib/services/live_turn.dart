import '../protocol/messages.dart';

/// P124 — what a code engine (or the code tasks of a manager) has done so far in the turn a conversation is waiting on, built
/// from the hub's `chatEvent`s and shown in place of the "Thinking…" line until the turn's answer comes. A timeline, because
/// the work and the words alternate: some text, a tool, more text. Same rules as the web's `liveTurn.ts`.

/// How much of one block of words a live bubble keeps: the latest, so a task that writes a lot doesn't grow it without end.
/// Large enough for any chat answer (P157: the model's words are shown as it writes them), which would otherwise lose its start.
const liveTextLimit = 100000;

sealed class LiveItem {
  const LiveItem();
}

final class LiveText extends LiveItem {
  const LiveText(this.text, {this.source});

  final String text;

  /// The task the words are from, when several are told at once.
  final String? source;
}

final class LiveTool extends LiveItem {
  const LiveTool({required this.callId, required this.tool, required this.title, required this.status});

  final String callId;
  final String tool;
  final String title;
  final ToolStatus status;
}

final class LiveTurn {
  const LiveTurn({this.items = const [], this.notice});

  final List<LiveItem> items;

  /// The engine saying something about itself (a retry, a wait): shown under the work, replaced by the next.
  final String? notice;

  LiveTurn apply(ChatEvent event) {
    switch (event) {
      case ChatEventText(:final text, :final source):
        final last = items.isEmpty ? null : items.last;
        // More words continue the last block of text if they are from the same task; a tool in between, or another task, starts a new one.
        if (last is LiveText && last.source == source) {
          return LiveTurn(items: [...items.take(items.length - 1), LiveText(_latest(last.text + text), source: source)], notice: notice);
        }
        return LiveTurn(items: [...items, LiveText(_latest(text), source: source)], notice: notice);
      case ChatEventTool(:final callId, :final tool, :final title, :final status):
        final line = LiveTool(callId: callId, tool: tool, title: title, status: status);
        final at = items.indexWhere((item) => item is LiveTool && item.callId == callId);
        // A later stage of the same call updates its line, wherever it was left.
        if (at >= 0) {
          return LiveTurn(items: [for (var i = 0; i < items.length; i++) i == at ? line : items[i]], notice: notice);
        }
        return LiveTurn(items: [...items, line], notice: notice);
      case ChatEventNotice(:final text):
        return LiveTurn(items: items, notice: text);
    }
  }

  static String _latest(String text) => text.length <= liveTextLimit ? text : text.substring(text.length - liveTextLimit);
}
