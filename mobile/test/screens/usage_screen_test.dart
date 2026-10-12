import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:mobile/protocol/messages.dart';
import 'package:mobile/screens/usage_screen.dart';

UsageSummary _summary({List<LimitMeter> limits = const [], bool limitsEnabled = true}) => UsageSummary(
      totalTokens: 12500,
      calls: 7,
      days: const [
        UsageDay(date: '2026-10-10', calls: 0, tokens: 0),
        UsageDay(date: '2026-10-11', calls: 2, tokens: 2500, costUsd: 0.25),
        UsageDay(date: '2026-10-12', calls: 5, tokens: 10000),
      ],
      limitsEnabled: limitsEnabled,
      limits: limits,
      windowHours: 24,
      costInWindowUsd: 0.25,
    );

Future<void> _open(WidgetTester tester, Future<UsageSummary> Function() load) async {
  // A fresh screen each time: the same widget type in the same place would keep the state (and the report) of the one before.
  await tester.pumpWidget(const SizedBox());
  await tester.pumpWidget(MaterialApp(home: UsageScreen(load: load)));
  await tester.pumpAndSettle();
}

void main() {
  testWidgets('shows the totals, the days that had calls (newest first) and the limit meters, with nothing to allow more (P99)', (tester) async {
    await _open(
      tester,
      () async => _summary(limits: const [
        LimitMeter(id: 'ana-day', windowHours: 24, usedTokens: 200, maxTokens: 150, usedCostUsd: 0, fraction: 1.33, warn: true, exceeded: true),
      ]),
    );

    expect(find.descendant(of: find.byKey(const Key('usage-tokens')), matching: find.text('13k')), findsOneWidget, reason: '12 500 tokens, rounded');
    expect(find.descendant(of: find.byKey(const Key('usage-calls')), matching: find.text('7')), findsOneWidget);
    expect(find.descendant(of: find.byKey(const Key('usage-cost')), matching: find.text('US\$ 0.2500')), findsOneWidget);
    expect(find.byKey(const Key('usage-day-2026-10-10')), findsNothing, reason: 'a day with no call is left out');
    expect(tester.getTopLeft(find.byKey(const Key('usage-day-2026-10-12'))).dy, lessThan(tester.getTopLeft(find.byKey(const Key('usage-day-2026-10-11'))).dy));
    expect(find.text('2.5k tokens · US\$ 0.2500'), findsOneWidget);

    expect(find.byKey(const Key('usage-limit-ana-day')), findsOneWidget);
    expect(find.textContaining('Used up'), findsOneWidget);
    expect(find.textContaining('200 of 150'), findsOneWidget);
    expect(find.textContaining('ask whoever runs this Warden'), findsOneWidget);
    expect(find.textContaining('Allow more'), findsNothing);
  });

  testWidgets('says so when limits are off or none is written for the person', (tester) async {
    await _open(tester, () async => _summary(limitsEnabled: false));
    expect(find.text('Spending limits are switched off on this hub.'), findsOneWidget);

    await _open(tester, () async => _summary());
    expect(find.text('No limit is set for you.'), findsOneWidget);
  });

  testWidgets("shows the hub's refusal instead of an empty screen", (tester) async {
    await _open(tester, () async => throw Exception('your data is locked'));
    expect(find.byKey(const Key('usage-error')), findsOneWidget);
    expect(find.textContaining('your data is locked'), findsOneWidget);
  });
}
