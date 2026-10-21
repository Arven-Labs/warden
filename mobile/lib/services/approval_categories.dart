import '../l10n/generated/app_localizations.dart';

/// P122 — the kinds of action an agent can be made to get approved even when it acts on its own. The ids are those of
/// `warden_core::autonomy::Category`, as the hub and `AgentInfo.approvalRequired` write them (the same list as the web's).
const approvalCategoryIds = <String>['delete_data', 'spend_money', 'critical_infra', 'external_message', 'publish_code', 'important_config', 'elevated_agent'];

/// The five autonomy levels (the words of each are `autonomyLabel`).
const autonomyLevels = <int>[1, 2, 3, 4, 5];

/// One line for an autonomy level, in the language of the app.
String autonomyLabel(int level, AppLocalizations l10n) => switch (level) {
      1 => l10n.autonomy1,
      2 => l10n.autonomy2,
      3 => l10n.autonomy3,
      4 => l10n.autonomy4,
      _ => l10n.autonomy5,
    };

/// The level in two or three words, for the badge on an agent's card.
String autonomyShortLabel(int level, AppLocalizations l10n) => switch (level) {
      1 => l10n.autonomyShort1,
      2 => l10n.autonomyShort2,
      3 => l10n.autonomyShort3,
      4 => l10n.autonomyShort4,
      5 => l10n.autonomyShort5,
      _ => '',
    };

/// The name of a kind of action, or its own id when a newer hub sends one this screen does not know.
String approvalCategoryLabel(String id, AppLocalizations l10n) => switch (id) {
      'delete_data' => l10n.categoryDeleteData,
      'spend_money' => l10n.categorySpendMoney,
      'critical_infra' => l10n.categoryCriticalInfra,
      'external_message' => l10n.categoryExternalMessage,
      'publish_code' => l10n.categoryPublishCode,
      'important_config' => l10n.categoryImportantConfig,
      'elevated_agent' => l10n.categoryElevatedAgent,
      _ => id,
    };

/// [marked] in a fixed order, each once: the kinds this screen knows in the list's order, then the ones only the hub knows, so the
/// same set always makes the same edit.
List<String> orderedCategories(Iterable<String> marked) {
  final set = marked.toSet();
  final known = [for (final id in approvalCategoryIds) if (set.contains(id)) id];
  final unknown = [for (final id in marked.toSet()) if (!approvalCategoryIds.contains(id)) id];
  return [...known, ...unknown];
}
