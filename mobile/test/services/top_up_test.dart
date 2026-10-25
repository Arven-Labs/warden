import 'package:flutter_test/flutter_test.dart';
import 'package:mobile/protocol/messages.dart';
import 'package:mobile/services/top_up.dart';

void main() {
  group('topUpUri (P168)', () {
    test('a plain https address passes', () {
      expect(topUpUri('https://example.com/credits'), Uri.parse('https://example.com/credits'));
    });

    test('nothing, something that is not an address, or anything but https with no login in it is "no link"', () {
      for (final bad in [null, '', 'http://example.com/credits', 'javascript:alert(1)', 'tel:+5511999999999', 'intent://x#Intent;end', 'https://user:pass@example.com/', 'https:///nohost', 'not a url']) {
        expect(topUpUri(bad), isNull, reason: '$bad');
      }
    });
  });

  group('the usage report carries the address to buy more credit', () {
    Map<String, dynamic> report(Map<String, dynamic> extra) => {'total': {'totalTokens': 10}, 'messageCount': 1, ...extra};

    test('with the credit', () {
      final summary = UsageSummary.fromJson(report({
        'credit': {'balanceMicroUsd': 5000000, 'availableMicroUsd': 4000000},
        'topupUrl': 'https://example.com/credits',
      }));
      expect(summary.credit?.balanceMicroUsd, 5000000);
      expect(summary.topupUrl, 'https://example.com/credits');
    });

    test('a hub that has no address (or is older) sends none', () {
      final summary = UsageSummary.fromJson(report({'credit': {'balanceMicroUsd': 5000000, 'availableMicroUsd': 4000000}}));
      expect(summary.credit, isNotNull);
      expect(summary.topupUrl, isNull);
    });

    test('an address without a credit has nothing to top up', () {
      final summary = UsageSummary.fromJson(report({'topupUrl': 'https://example.com/credits'}));
      expect(summary.credit, isNull);
      expect(summary.topupUrl, isNull);
    });
  });
}
