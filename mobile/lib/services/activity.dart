import '../l10n/generated/app_localizations.dart';
import '../protocol/messages.dart';

// P121 — the feed of activity: who did what among the agents, newest first. Pure; the screen only draws what comes out of here.
// Mirrors `web/src/hub/activity.ts`; the words come from the app's catalog (P136).

/// What the screen calls the assistant that answers with no agent picked (an event with no `actor`).
String _who(String name, AppLocalizations l10n) => name.trim().isEmpty ? l10n.activityNoAgent : name;

/// The runs of scheduled tasks and webhooks: their target is the id of the task or the webhook, not an agent.
const _runKinds = {'scheduled_ran', 'scheduled_failed', 'webhook_ran', 'webhook_failed'};

/// The agent the event is done to, when its target is an agent.
String? _targetAgent(ActivityEvent event) => _runKinds.contains(event.kind) ? null : event.target;

/// The sentence of an event, without its text (the text is the objective, the answer or the message).
String activityHeadline(ActivityEvent event, AppLocalizations l10n) {
  final actor = _who(event.actor, l10n);
  final target = event.target == null ? '' : _who(event.target!, l10n);
  return switch (event.kind) {
    'created_agent' => l10n.activityCreatedAgent(actor, target),
    'removed_agent' => l10n.activityRemovedAgent(actor, target),
    'updated_agent' => l10n.activityUpdatedAgent(actor, target),
    'scheduled_ran' => l10n.activityScheduledRan(actor, target),
    'scheduled_failed' => l10n.activityScheduledFailed(target),
    'webhook_ran' => l10n.activityWebhookRan(actor, target),
    'webhook_failed' => l10n.activityWebhookFailed(target),
    'delegated' => l10n.activityDelegated(actor, target),
    'started' => l10n.activityStarted(actor),
    'done' => l10n.activityDone(actor),
    'failed' => l10n.activityFailed(actor),
    'cancelled' => l10n.activityCancelled(actor),
    'note' => l10n.activityNote(actor, target),
    'reply' => l10n.activityReply(actor, target),
    'messaged_user' => l10n.activityMessagedUser(actor),
    _ => '$actor: ${event.kind}',
  };
}

/// A short mark per kind, to scan the list by eye.
String activityMark(String kind) => switch (kind) {
      'delegated' => '→',
      'started' => '▶',
      'done' => '✓',
      'failed' || 'scheduled_failed' || 'webhook_failed' => '✕',
      'cancelled' => '■',
      'created_agent' => '+',
      'removed_agent' => '−',
      'updated_agent' => '~',
      'scheduled_ran' => '⏱',
      'webhook_ran' => '⚡',
      'note' || 'reply' => '✉',
      'messaged_user' => '●',
      _ => '·',
    };

/// The events an agent appears in, doing or receiving.
List<ActivityEvent> activityInvolving(List<ActivityEvent> events, String agent) =>
    [for (final e in events) if (e.actor == agent || _targetAgent(e) == agent) e];

/// The agents that appear in the events, alphabetical, without the nameless assistant (nor the id of a scheduled task or a webhook).
List<String> activityAgents(List<ActivityEvent> events) {
  final names = <String>{};
  for (final e in events) {
    if (e.actor.trim().isNotEmpty) names.add(e.actor);
    final target = _targetAgent(e);
    if (target != null && target.trim().isNotEmpty) names.add(target);
  }
  return names.toList()..sort((a, b) => a.compareTo(b));
}

enum ActivityTarget { tasks, conversation, channel }

/// What a click on an event opens: the agent's tasks, the conversation between two agents, or the agent's channel.
class ActivityDestination {
  const ActivityDestination(this.target, {this.agent, this.conversationId});

  final ActivityTarget target;
  final String? agent;
  final String? conversationId;

  @override
  bool operator ==(Object other) =>
      other is ActivityDestination && other.target == target && other.agent == agent && other.conversationId == conversationId;

  @override
  int get hashCode => Object.hash(target, agent, conversationId);
}

ActivityDestination? activityDestination(ActivityEvent event) {
  if (event.kind == 'messaged_user') return ActivityDestination(ActivityTarget.channel, agent: event.actor);
  final conversation = event.conversationId;
  if (conversation != null) return ActivityDestination(ActivityTarget.conversation, conversationId: conversation);
  if (event.taskId != null) {
    final agent = event.kind == 'delegated' ? event.target : event.actor;
    return agent == null || agent.isEmpty ? null : ActivityDestination(ActivityTarget.tasks, agent: agent);
  }
  return null;
}

/// The events of one day, with its label ("Today", "Yesterday" or the date).
class ActivityDay {
  const ActivityDay(this.label, this.events);

  final String label;
  final List<ActivityEvent> events;
}

int _dayKey(DateTime d) => d.year * 10000 + d.month * 100 + d.day;

/// The events (already newest first) split by day, each day with its label.
List<ActivityDay> activityByDay(List<ActivityEvent> events, DateTime now, AppLocalizations l10n) {
  final today = _dayKey(now);
  final yesterday = _dayKey(now.subtract(const Duration(days: 1)));
  final days = <ActivityDay>[];
  for (final event in events) {
    final at = DateTime.fromMillisecondsSinceEpoch(event.atMs);
    final key = _dayKey(at);
    final label = key == today
        ? l10n.activityToday
        : key == yesterday
            ? l10n.activityYesterday
            : l10n.activityDate(at.day, '${at.month}', at.year);
    if (days.isNotEmpty && days.last.label == label) {
      days.last.events.add(event);
    } else {
      days.add(ActivityDay(label, [event]));
    }
  }
  return days;
}
