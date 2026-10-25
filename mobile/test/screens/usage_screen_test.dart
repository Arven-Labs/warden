import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:mobile/protocol/messages.dart';
import 'package:mobile/screens/usage_screen.dart';
import 'package:mobile/services/top_up.dart';

import '../helpers/test_app.dart';

UsageSummary _summary({
  List<LimitMeter> limits = const [],
  bool limitsEnabled = true,
  List<UsageBucket> byAgent = const [],
  List<UsageBucket> byProvider = const [],
  List<UsageBucket> byCombo = const [],
  List<SpendBucket> comboSpend = const [],
  HostedCredit? credit,
  String? topupUrl,
}) =>
    UsageSummary(
      credit: credit,
      topupUrl: topupUrl,
      comboSpend: comboSpend,
      byAgent: byAgent,
      byProvider: byProvider,
      byCombo: byCombo,
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

Future<void> _open(WidgetTester tester, Future<UsageSummary> Function() load, {UrlLauncher? launchUrl}) async {
  // A fresh screen each time: the same widget type in the same place would keep the state (and the report) of the one before.
  await tester.pumpWidget(const SizedBox());
  await tester.pumpWidget(testApp(home: launchUrl == null ? UsageScreen(load: load) : UsageScreen(load: load, launchUrl: launchUrl)));
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

  testWidgets('shows the tokens by agent, provider and combo, the bucket with no key by its own name (P126)', (tester) async {
    await _open(
      tester,
      () async => _summary(
        byAgent: const [UsageBucket(key: 'writer', calls: 2, tokens: 30000), UsageBucket(key: null, calls: 1, tokens: 15000)],
        byProvider: const [UsageBucket(key: null, calls: 3, tokens: 45000)],
        byCombo: const [UsageBucket(key: 'fast', calls: 1, tokens: 2500)],
      ),
    );
    await tester.scrollUntilVisible(find.byKey(const Key('usage-combo-fast')), 200, scrollable: find.byType(Scrollable).first);

    expect(find.byKey(const Key('usage-agent-title')), findsOneWidget);
    expect(find.descendant(of: find.byKey(const Key('usage-agent-writer')), matching: find.text('30k tokens · 2 calls')), findsOneWidget);
    expect(find.descendant(of: find.byKey(const Key('usage-agent-none')), matching: find.text('No agent')), findsOneWidget);
    expect(find.descendant(of: find.byKey(const Key('usage-agent-none')), matching: find.text('15k tokens · 1 call')), findsOneWidget, reason: 'one call, singular');
    expect(find.descendant(of: find.byKey(const Key('usage-provider-none')), matching: find.text('Default provider')), findsOneWidget);
    expect(find.descendant(of: find.byKey(const Key('usage-combo-fast')), matching: find.text('2.5k tokens · 1 call')), findsOneWidget);
    expect(tester.getTopLeft(find.byKey(const Key('usage-agent-writer'))).dy, lessThan(tester.getTopLeft(find.byKey(const Key('usage-agent-none'))).dy), reason: "the hub's order");
  });

  testWidgets('shows no breakdown at all for an older hub that sends none (P126)', (tester) async {
    await _open(tester, () async => _summary());
    for (final prefix in ['agent', 'provider', 'combo']) {
      expect(find.byKey(Key('usage-$prefix-title')), findsNothing);
    }
    expect(find.textContaining('Spent by combo'), findsNothing);
  });

  testWidgets('shows the dollars by combo of the ledger window, the biggest first, with the unpriced calls (P126)', (tester) async {
    await _open(
      tester,
      () async => _summary(comboSpend: const [
        SpendBucket(key: 'slow', costUsd: 0.5, unpricedCalls: 0),
        SpendBucket(key: 'fast', costUsd: 1.25, unpricedCalls: 2),
      ]),
    );
    await tester.scrollUntilVisible(find.byKey(const Key('usage-spend-combo-slow')), 200, scrollable: find.byType(Scrollable).first);

    expect(find.text('Spent by combo, last 24 h'), findsOneWidget);
    expect(find.descendant(of: find.byKey(const Key('usage-spend-combo-fast')), matching: find.text('US\$ 1.25 (+2 unpriced)')), findsOneWidget);
    expect(find.descendant(of: find.byKey(const Key('usage-spend-combo-slow')), matching: find.text('US\$ 0.5000')), findsOneWidget);
    expect(tester.getTopLeft(find.byKey(const Key('usage-spend-combo-fast'))).dy, lessThan(tester.getTopLeft(find.byKey(const Key('usage-spend-combo-slow'))).dy));
  });

  testWidgets('shows the hosted credit, with what is available only while calls in progress hold part of it (P168)', (tester) async {
    await _open(tester, () async => _summary(credit: const HostedCredit(balanceMicroUsd: 5000000, availableMicroUsd: 4000000)));
    expect(find.descendant(of: find.byKey(const Key('usage-credit')), matching: find.text('US\$ 5.00')), findsOneWidget);
    expect(find.text('US\$ 4.00 available after the calls in progress'), findsOneWidget);
    expect(find.byKey(const Key('usage-credit-empty')), findsNothing);

    await _open(tester, () async => _summary(credit: const HostedCredit(balanceMicroUsd: 5000000, availableMicroUsd: 5000000)));
    expect(find.descendant(of: find.byKey(const Key('usage-credit')), matching: find.text('US\$ 5.00')), findsOneWidget);
    expect(find.textContaining('available after'), findsNothing, reason: 'nothing is held, so the balance is all there is');
  });

  testWidgets('offers the hub\'s top-up address next to the credit and opens it outside the app (P168)', (tester) async {
    final opened = <Uri>[];
    const credit = HostedCredit(balanceMicroUsd: 5000000, availableMicroUsd: 5000000);
    await _open(
      tester,
      () async => _summary(credit: credit, topupUrl: 'https://example.com/credits'),
      launchUrl: (uri) async {
        opened.add(uri);
        return true;
      },
    );
    await tester.tap(find.byKey(const Key('usage-credit-topup')));
    expect(find.text('Top up credit'), findsOneWidget);
    expect(opened, [Uri.parse('https://example.com/credits')]);

    // No address configured, or one that is not a plain https address: no button, and nothing to open.
    for (final topupUrl in [null, 'http://example.com/credits', 'javascript:alert(1)', 'https://user:pass@example.com/']) {
      await _open(tester, () async => _summary(credit: credit, topupUrl: topupUrl));
      expect(find.byKey(const Key('usage-credit-topup')), findsNothing, reason: '$topupUrl');
    }
    // An address with no credit has nothing to top up.
    await _open(tester, () async => _summary(topupUrl: 'https://example.com/credits'));
    expect(find.byKey(const Key('usage-credit-topup')), findsNothing);
  });

  testWidgets('warns when no credit is left, even with a balance held by calls in progress (P168)', (tester) async {
    await _open(tester, () async => _summary(credit: const HostedCredit(balanceMicroUsd: 3000000, availableMicroUsd: 0)));
    expect(find.byKey(const Key('usage-credit-empty')), findsOneWidget);
    expect(find.textContaining('Out of credits'), findsOneWidget);
  });

  testWidgets('shows no credit for a hub that has none, and reads it in Portuguese (P168)', (tester) async {
    await _open(tester, () async => _summary());
    expect(find.byKey(const Key('usage-credit')), findsNothing);
    expect(find.byKey(const Key('usage-credit-empty')), findsNothing);

    await tester.pumpWidget(const SizedBox());
    await tester.pumpWidget(testApp(locale: const Locale('pt'), home: UsageScreen(load: () async => _summary(credit: const HostedCredit(balanceMicroUsd: 0, availableMicroUsd: 0)))));
    await tester.pumpAndSettle();
    expect(find.text('Créditos'), findsOneWidget);
    expect(find.textContaining('Sem créditos'), findsOneWidget);
  });

  testWidgets("shows the hub's refusal instead of an empty screen", (tester) async {
    await _open(tester, () async => throw Exception('your data is locked'));
    expect(find.byKey(const Key('usage-error')), findsOneWidget);
    expect(find.textContaining('your data is locked'), findsOneWidget);
  });
}
