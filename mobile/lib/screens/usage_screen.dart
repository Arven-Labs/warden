import 'package:flutter/material.dart';

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
    return Scaffold(
      appBar: AppBar(title: const Text('Usage')),
      body: RefreshIndicator(
        onRefresh: _refresh,
        child: ListView(
          physics: const AlwaysScrollableScrollPhysics(),
          padding: const EdgeInsets.all(16),
          children: [
            if (_error != null) Text("Couldn't load the usage: $_error", key: const Key('usage-error'), style: TextStyle(color: Theme.of(context).colorScheme.error)),
            if (summary == null && _error == null) const Center(child: Padding(padding: EdgeInsets.all(32), child: CircularProgressIndicator())),
            if (summary != null) ..._content(context, summary),
          ],
        ),
      ),
    );
  }

  List<Widget> _content(BuildContext context, UsageSummary summary) {
    final theme = Theme.of(context);
    final days = summary.days.where((day) => day.calls > 0).toList().reversed.toList();
    return [
      Row(
        children: [
          Expanded(child: _Tile(key: const Key('usage-tokens'), label: 'Tokens', value: _compact(summary.totalTokens))),
          const SizedBox(width: 12),
          Expanded(child: _Tile(key: const Key('usage-calls'), label: 'Model calls', value: '${summary.calls}')),
          if (summary.windowHours > 0) ...[
            const SizedBox(width: 12),
            Expanded(
              child: _Tile(
                key: const Key('usage-cost'),
                label: 'Spent, last ${_window(summary.windowHours)}',
                value: summary.costInWindowUsd > 0 ? _usd(summary.costInWindowUsd) : '—',
              ),
            ),
          ],
        ],
      ),
      const SizedBox(height: 24),
      Text('Limits', style: theme.textTheme.titleMedium),
      const SizedBox(height: 8),
      if (!summary.limitsEnabled)
        const Text('Spending limits are switched off on this hub.')
      else if (summary.limits.isEmpty)
        const Text('No limit is set for you.')
      else
        for (final limit in summary.limits) _LimitCard(limit: limit),
      if (summary.limitsEnabled) Text('Only the limits written for you show here. To allow more, ask whoever runs this Warden.', style: theme.textTheme.bodySmall),
      const SizedBox(height: 24),
      Text('By day', style: theme.textTheme.titleMedium),
      const SizedBox(height: 8),
      if (days.isEmpty)
        const Text('No conversation yet.')
      else
        for (final day in days)
          ListTile(
            key: Key('usage-day-${day.date}'),
            dense: true,
            contentPadding: EdgeInsets.zero,
            title: Text(_shortDate(day.date)),
            subtitle: Text('${day.calls} ${day.calls == 1 ? 'call' : 'calls'}'),
            trailing: Text(day.costUsd != null && day.costUsd! > 0 ? '${_compact(day.tokens)} tokens · ${_usd(day.costUsd!)}' : '${_compact(day.tokens)} tokens'),
          ),
    ];
  }
}

class _Tile extends StatelessWidget {
  const _Tile({super.key, required this.label, required this.value});

  final String label;
  final String value;

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
    final color = limit.exceeded ? theme.colorScheme.error : (limit.warn ? Colors.orange : theme.colorScheme.primary);
    final parts = [
      if (limit.maxTokens != null) '${_compact(limit.usedTokens)} of ${_compact(limit.maxTokens!)} tokens',
      if (limit.maxCostUsd != null) '${_usd(limit.usedCostUsd)} of ${_usd(limit.maxCostUsd!)}',
    ];
    final state = limit.exceeded ? 'Used up' : (limit.warn ? 'Close to the limit' : 'Plenty left');
    return Card(
      key: Key('usage-limit-${limit.id}'),
      child: Padding(
        padding: const EdgeInsets.all(12),
        child: Column(
          crossAxisAlignment: CrossAxisAlignment.start,
          children: [
            Text('${limit.id} · last ${_window(limit.windowHours)}', style: theme.textTheme.titleSmall),
            const SizedBox(height: 8),
            LinearProgressIndicator(value: limit.fraction.clamp(0, 1).toDouble(), color: color, minHeight: 8),
            const SizedBox(height: 8),
            Text('$state · ${(limit.fraction * 100).round()}% · ${parts.join(' · ')}', style: theme.textTheme.bodySmall),
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

String _window(int hours) => hours % 24 == 0 ? (hours == 24 ? '24 h' : '${hours ~/ 24} days') : '$hours h';

/// `2026-09-25` → `25/09`.
String _shortDate(String date) {
  final parts = date.split('-');
  return parts.length == 3 ? '${parts[2]}/${parts[1]}' : date;
}
