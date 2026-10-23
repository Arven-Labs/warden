import 'package:flutter/material.dart';

import '../l10n/generated/app_localizations.dart';
import '../protocol/messages.dart';

/// P99 — the person's own usage: tokens and calls, the dollars the hub's ledger still holds, a line per day and the limits that apply
/// to them. Read only. A member's report holds only their own conversations, calls and limits (the hub makes it so), and they cannot
/// allow more: that is the owner's.
class UsageScreen extends StatefulWidget {
  const UsageScreen({super.key, required this.load});

  /// Asks the hub for the report.
  final Future<UsageSummary> Function() load;

  @override
  State<UsageScreen> createState() => _UsageScreenState();
}

class _UsageScreenState extends State<UsageScreen> {
  UsageSummary? _summary;
  String? _error;

  @override
  void initState() {
    super.initState();
    _refresh();
  }

  Future<void> _refresh() async {
    try {
      final summary = await widget.load();
      if (!mounted) return;
      setState(() {
        _summary = summary;
        _error = null;
      });
    } catch (e) {
      if (!mounted) return;
      setState(() => _error = e is Exception ? e.toString() : '$e');
    }
  }

  @override
  Widget build(BuildContext context) {
    final summary = _summary;
    final l10n = AppLocalizations.of(context);
    return Scaffold(
      appBar: AppBar(title: Text(l10n.usageTitle)),
      body: RefreshIndicator(
        onRefresh: _refresh,
        child: ListView(
          physics: const AlwaysScrollableScrollPhysics(),
          padding: const EdgeInsets.all(16),
          children: [
            if (_error != null) Text(l10n.usageLoadFailed('$_error'), key: const Key('usage-error'), style: TextStyle(color: Theme.of(context).colorScheme.error)),
            if (summary == null && _error == null) const Center(child: Padding(padding: EdgeInsets.all(32), child: CircularProgressIndicator())),
            if (summary != null) ..._content(context, summary),
          ],
        ),
      ),
    );
  }

  List<Widget> _content(BuildContext context, UsageSummary summary) {
    final theme = Theme.of(context);
    final l10n = AppLocalizations.of(context);
    final days = summary.days.where((day) => day.calls > 0).toList().reversed.toList();
    final credit = summary.credit;
    return [
      if (credit != null) ...[
        if (credit.isOut) Text(l10n.usageCreditEmpty, key: const Key('usage-credit-empty'), style: TextStyle(color: theme.colorScheme.error)),
        _Tile(
          key: const Key('usage-credit'),
          label: l10n.usageCredit,
          value: _usd(credit.balanceUsd),
          note: credit.hasHolds ? l10n.usageCreditAvailable(_usd(credit.availableUsd)) : null,
        ),
        const SizedBox(height: 12),
      ],
      Row(
        children: [
          Expanded(child: _Tile(key: const Key('usage-tokens'), label: l10n.usageTokens, value: _compact(summary.totalTokens))),
          const SizedBox(width: 12),
          Expanded(child: _Tile(key: const Key('usage-calls'), label: l10n.usageCalls, value: '${summary.calls}')),
          if (summary.windowHours > 0) ...[
            const SizedBox(width: 12),
            Expanded(
              child: _Tile(
                key: const Key('usage-cost'),
                label: l10n.usageSpent(_window(summary.windowHours, l10n)),
                value: summary.costInWindowUsd > 0 ? _usd(summary.costInWindowUsd) : '—',
              ),
            ),
          ],
        ],
      ),
      const SizedBox(height: 24),
      Text(l10n.usageLimits, style: theme.textTheme.titleMedium),
      const SizedBox(height: 8),
      if (!summary.limitsEnabled)
        Text(l10n.usageLimitsOff)
      else if (summary.limits.isEmpty)
        Text(l10n.usageLimitsNone)
      else
        for (final limit in summary.limits) _LimitCard(limit: limit),
      if (summary.limitsEnabled) Text(l10n.usageLimitsNote, style: theme.textTheme.bodySmall),
      const SizedBox(height: 24),
      Text(l10n.usageByDay, style: theme.textTheme.titleMedium),
      const SizedBox(height: 8),
      if (days.isEmpty)
        Text(l10n.usageNoConversation)
      else
        for (final day in days)
          ListTile(
            key: Key('usage-day-${day.date}'),
            dense: true,
            contentPadding: EdgeInsets.zero,
            title: Text(_shortDate(day.date)),
            subtitle: Text(l10n.usageCallsCount(day.calls)),
            trailing: Text(day.costUsd != null && day.costUsd! > 0 ? l10n.usageTokensCost(_compact(day.tokens), _usd(day.costUsd!)) : l10n.usageTokensCount(_compact(day.tokens))),
          ),
      _Breakdown(title: l10n.usageByAgent, prefix: 'agent', emptyLabel: l10n.usageNoAgent, buckets: summary.byAgent),
      _Breakdown(title: l10n.usageByProvider, prefix: 'provider', emptyLabel: l10n.usageDefaultProvider, buckets: summary.byProvider),
      _Breakdown(title: l10n.usageByCombo, prefix: 'combo', emptyLabel: l10n.usageNoCombo, buckets: summary.byCombo),
      if (summary.comboSpend.isNotEmpty) ...[
        const SizedBox(height: 16),
        Text(l10n.usageSpentByCombo(_window(summary.windowHours, l10n)), style: theme.textTheme.titleSmall),
        for (final spend in [...summary.comboSpend]..sort((a, b) => b.costUsd.compareTo(a.costUsd)))
          ListTile(
            key: Key('usage-spend-combo-${spend.key}'),
            dense: true,
            contentPadding: EdgeInsets.zero,
            title: Text(spend.key),
            trailing: Text(
              spend.unpricedCalls > 0 ? l10n.usageUnpriced(_usd(spend.costUsd), spend.unpricedCalls) : _usd(spend.costUsd),
            ),
          ),
      ],
    ];
  }
}

/// Tokens by agent, provider or combo (P126): a bar per bucket against the biggest one. Nothing to show hides it (an older hub sends none).
class _Breakdown extends StatelessWidget {
  const _Breakdown({required this.title, required this.prefix, required this.emptyLabel, required this.buckets});

  final String title;

  /// Starts the rows' keys: `usage-<prefix>-<key>`, with `none` for the bucket that has no key.
  final String prefix;
  final String emptyLabel;
  final List<UsageBucket> buckets;

  @override
  Widget build(BuildContext context) {
    if (buckets.isEmpty) return const SizedBox.shrink();
    final theme = Theme.of(context);
    final l10n = AppLocalizations.of(context);
    final biggest = buckets.map((b) => b.tokens).reduce((a, b) => a > b ? a : b);
    return Column(
      crossAxisAlignment: CrossAxisAlignment.start,
      children: [
        const SizedBox(height: 24),
        Text(title, key: Key('usage-$prefix-title'), style: theme.textTheme.titleMedium),
        const SizedBox(height: 8),
        for (final bucket in buckets)
          Padding(
            key: Key('usage-$prefix-${bucket.key ?? 'none'}'),
            padding: const EdgeInsets.symmetric(vertical: 4),
            child: Column(
              crossAxisAlignment: CrossAxisAlignment.start,
              children: [
                Row(
                  children: [
                    Expanded(child: Text(bucket.key ?? emptyLabel, overflow: TextOverflow.ellipsis)),
                    Text(l10n.usageBucketLine(_compact(bucket.tokens), l10n.usageCallsCount(bucket.calls)), style: theme.textTheme.bodySmall),
                  ],
                ),
                const SizedBox(height: 4),
                LinearProgressIndicator(value: biggest > 0 ? bucket.tokens / biggest : 0, minHeight: 6),
              ],
            ),
          ),
      ],
    );
  }
}

class _Tile extends StatelessWidget {
  const _Tile({super.key, required this.label, required this.value, this.note});

  final String label;
  final String value;

  /// A line under the label; a phone has no hover for a hint.
  final String? note;

  @override
  Widget build(BuildContext context) {
    final theme = Theme.of(context);
    return Card(
      child: Padding(
        padding: const EdgeInsets.all(12),
        child: Column(
          crossAxisAlignment: CrossAxisAlignment.start,
          children: [
            Text(value, style: theme.textTheme.titleLarge),
            Text(label, style: theme.textTheme.bodySmall),
            if (note != null) Text(note!, style: theme.textTheme.bodySmall),
          ],
        ),
      ),
    );
  }
}

class _LimitCard extends StatelessWidget {
  const _LimitCard({required this.limit});

  final LimitMeter limit;

  @override
  Widget build(BuildContext context) {
    final theme = Theme.of(context);
    final l10n = AppLocalizations.of(context);
    final color = limit.exceeded ? theme.colorScheme.error : (limit.warn ? Colors.orange : theme.colorScheme.primary);
    final parts = [
      if (limit.maxTokens != null) l10n.usageLimitTokens(_compact(limit.usedTokens), _compact(limit.maxTokens!)),
      if (limit.maxCostUsd != null) l10n.usageLimitCost(_usd(limit.usedCostUsd), _usd(limit.maxCostUsd!)),
    ];
    final state = limit.exceeded ? l10n.usageStateOver : (limit.warn ? l10n.usageStateWarn : l10n.usageStateOk);
    return Card(
      key: Key('usage-limit-${limit.id}'),
      child: Padding(
        padding: const EdgeInsets.all(12),
        child: Column(
          crossAxisAlignment: CrossAxisAlignment.start,
          children: [
            Text(l10n.usageLimitTitle(limit.id, _window(limit.windowHours, l10n)), style: theme.textTheme.titleSmall),
            const SizedBox(height: 8),
            LinearProgressIndicator(value: limit.fraction.clamp(0, 1).toDouble(), color: color, minHeight: 8),
            const SizedBox(height: 8),
            Text(l10n.usageLimitState(state, (limit.fraction * 100).round(), parts.join(' · ')), style: theme.textTheme.bodySmall),
          ],
        ),
      ),
    );
  }
}

String _compact(int value) {
  if (value >= 1000000) return '${(value / 1000000).toStringAsFixed(value >= 10000000 ? 0 : 1)}M';
  if (value >= 1000) return '${(value / 1000).toStringAsFixed(value >= 10000 ? 0 : 1)}k';
  return '$value';
}

String _usd(double value) => 'US\$ ${value.toStringAsFixed(value < 1 ? 4 : 2)}';

String _window(int hours, AppLocalizations l10n) => hours % 24 == 0 ? (hours == 24 ? l10n.usageWindowDay : l10n.usageWindowDays(hours ~/ 24)) : l10n.usageWindowHours(hours);

/// `2026-09-25` → `25/09`.
String _shortDate(String date) {
  final parts = date.split('-');
  return parts.length == 3 ? '${parts[2]}/${parts[1]}' : date;
}
