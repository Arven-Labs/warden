import '../protocol/messages.dart';

/// P72 — what `SkillFormScreen` needs from the hub to write a draft: `ServerConnection` in the app, a fake in tests, like
/// [AgentsBackend]. The draft is never saved; the screen loads it into its form.
abstract interface class SkillDraftBackend {
  /// Asks the hub's model for a skill described by [description]. Throws a `ConversationException` with the hub's text when
  /// it refuses (empty or too long description, spending limit) or can't be asked (a timeout, a closed connection).
  Future<SkillDraft> draftSkill(String description);
}
