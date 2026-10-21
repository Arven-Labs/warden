import 'package:flutter_test/flutter_test.dart';
import 'package:mobile/l10n/generated/app_localizations_en.dart';
import 'package:mobile/l10n/generated/app_localizations_pt.dart';
import 'package:mobile/services/approval_categories.dart';

void main() {
  final en = AppLocalizationsEn();

  test('the screen offers the five levels and the seven kinds of action the hub has (P122)', () {
    expect(autonomyLevels, [1, 2, 3, 4, 5]);
    expect(
      approvalCategoryIds,
      ['delete_data', 'spend_money', 'critical_infra', 'external_message', 'publish_code', 'important_config', 'elevated_agent'],
    );
  });

  test('the checked kinds come out in the list order, once each, however they were checked', () {
    expect(orderedCategories(['publish_code', 'delete_data', 'publish_code', 'spend_money']), ['delete_data', 'spend_money', 'publish_code']);
    expect(orderedCategories(const []), isEmpty);
  });

  test('a kind only a newer hub knows is kept after the known ones, and is named by its own id', () {
    expect(orderedCategories(['brand_new', 'spend_money']), ['spend_money', 'brand_new']);
    expect(approvalCategoryLabel('brand_new', en), 'brand_new');
    expect(approvalCategoryLabel('spend_money', en), 'Spend money');
  });

  test('every level and every kind has words in both languages', () {
    final pt = AppLocalizationsPt();
    for (final level in autonomyLevels) {
      expect(autonomyLabel(level, en), startsWith('$level'));
      expect(autonomyLabel(level, pt), startsWith('$level'));
    }
    for (final id in approvalCategoryIds) {
      expect(approvalCategoryLabel(id, en), isNot(id));
      expect(approvalCategoryLabel(id, pt), isNot(approvalCategoryLabel(id, en)));
    }
  });
}
