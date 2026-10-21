import 'package:flutter/material.dart';
import 'package:qr_flutter/qr_flutter.dart';

import '../l10n/generated/app_localizations.dart';
import '../services/vault_paths.dart';
import '../src/rust/api/sync.dart' as bridge;

/// Fase 4.4 — vault + `config.toml` sync via Arweave (paid through the TruthID app), same engine
/// (`warden-sync`) the desktop `SyncView.tsx`/`warden-cli`'s `/sync` already drive. Reachable
/// independent of the `warden-server` connection (`ConnectionScreen`'s AppBar) — sync never talks
/// to `warden-server` at all, only to the paired device (LAN) and Arweave/TruthID.
class SyncScreen extends StatefulWidget {
  const SyncScreen({super.key});

  @override
  State<SyncScreen> createState() => _SyncScreenState();
}

enum _PairingHostStatus { idle, hosting, completed, failed }

class _SyncScreenState extends State<SyncScreen> {
  VaultPaths? _paths;
  bridge.SyncStatusDto? _status;
  String? _error;
  bool _busy = false;

  bridge.PushBeginDto? _pushBegin;
  bridge.PushResultDto? _pushResult;
  bridge.PullResultDto? _pullResult;

  _PairingHostStatus _pairingHostStatus = _PairingHostStatus.idle;
  String? _pairingCode;

  final _joinCodeController = TextEditingController();
  final _joinHostController = TextEditingController();
  bool _joining = false;

  @override
  void initState() {
    super.initState();
    _joinCodeController.addListener(() => setState(() {}));
    _load();
  }

  @override
  void dispose() {
    _joinCodeController.dispose();
    _joinHostController.dispose();
    super.dispose();
  }

  Future<void> _load() async {
    final paths = await VaultPaths.resolve();
    setState(() => _paths = paths);
    await _refreshStatus();
  }

  Future<void> _refreshStatus() async {
    final paths = _paths;
    if (paths == null) return;
    try {
      final status = bridge.bridgeStatus(
        vaultRoot: paths.vaultRoot,
        configPath: paths.configPath,
        secretsPath: paths.secretsPath,
        manifestPath: paths.manifestPath,
      );
      if (mounted) setState(() => _status = status);
    } catch (e) {
      if (mounted) setState(() => _error = e.toString());
    }
  }

  Future<void> _handleInit() async {
    final paths = _paths;
    if (paths == null) return;
    setState(() {
      _busy = true;
      _error = null;
    });
    try {
      await bridge.bridgeInitFresh(
        vaultRoot: paths.vaultRoot,
        configPath: paths.configPath,
        secretsPath: paths.secretsPath,
        manifestPath: paths.manifestPath,
      );
      await _refreshStatus();
    } catch (e) {
      setState(() => _error = e.toString());
    } finally {
      if (mounted) setState(() => _busy = false);
    }
  }

  Future<void> _handleSend() async {
    final paths = _paths;
    if (paths == null) return;
    final l10n = AppLocalizations.of(context);
    setState(() {
      _busy = true;
      _error = null;
      _pushResult = null;
    });
    try {
      final begin = await bridge.bridgePushBegin(
        vaultRoot: paths.vaultRoot,
        configPath: paths.configPath,
        secretsPath: paths.secretsPath,
        manifestPath: paths.manifestPath,
      );
      if (begin == null) {
        setState(() => _error = l10n.syncNothingToSend);
        return;
      }
      setState(() => _pushBegin = begin);
      final result = await bridge.bridgePushAwait(manifestPath: paths.manifestPath, hosts: const []);
      setState(() {
        _pushResult = result;
        _pushBegin = null;
      });
      await _refreshStatus();
    } catch (e) {
      setState(() {
        _error = e.toString();
        _pushBegin = null;
      });
    } finally {
      if (mounted) setState(() => _busy = false);
    }
  }

  Future<void> _handlePull() async {
    final paths = _paths;
    if (paths == null) return;
    setState(() {
      _busy = true;
      _error = null;
      _pullResult = null;
    });
    try {
      final result = await bridge.bridgePull(
        vaultRoot: paths.vaultRoot,
        configPath: paths.configPath,
        secretsPath: paths.secretsPath,
        manifestPath: paths.manifestPath,
      );
      setState(() => _pullResult = result);
      await _refreshStatus();
    } catch (e) {
      setState(() => _error = e.toString());
    } finally {
      if (mounted) setState(() => _busy = false);
    }
  }

  Future<void> _handleStartPairingHost() async {
    final paths = _paths;
    if (paths == null) return;
    setState(() {
      _error = null;
      _pairingHostStatus = _PairingHostStatus.hosting;
    });
    try {
      final code = await bridge.bridgePairingHostStart(
        vaultRoot: paths.vaultRoot,
        configPath: paths.configPath,
        secretsPath: paths.secretsPath,
        manifestPath: paths.manifestPath,
      );
      if (!mounted) return;
      setState(() => _pairingCode = code);
      await bridge.bridgePairingHostWait();
      if (!mounted) return;
      setState(() => _pairingHostStatus = _PairingHostStatus.completed);
      await _refreshStatus();
    } catch (e) {
      if (!mounted) return;
      setState(() {
        _pairingHostStatus = _PairingHostStatus.failed;
        _error = e.toString();
      });
    }
  }

  Future<void> _handleJoin() async {
    final paths = _paths;
    if (paths == null) return;
    final code = _joinCodeController.text.trim();
    if (code.isEmpty) return;
    final hostOverride = _joinHostController.text.trim();

    setState(() {
      _joining = true;
      _error = null;
    });
    try {
      await bridge.bridgePairingJoin(
        vaultRoot: paths.vaultRoot,
        configPath: paths.configPath,
        secretsPath: paths.secretsPath,
        manifestPath: paths.manifestPath,
        code: code,
        hosts: hostOverride.isEmpty ? const [] : [hostOverride],
      );
      _joinCodeController.clear();
      await _refreshStatus();
    } catch (e) {
      setState(() => _error = e.toString());
    } finally {
      if (mounted) setState(() => _joining = false);
    }
  }

  Widget _buildPairingHostStatus(AppLocalizations l10n) {
    switch (_pairingHostStatus) {
      case _PairingHostStatus.idle:
        return OutlinedButton(onPressed: _handleStartPairingHost, child: Text(l10n.syncShowCode));
      case _PairingHostStatus.hosting:
        return Column(
          crossAxisAlignment: CrossAxisAlignment.start,
          children: [
            if (_pairingCode case final code?) _PairingCodeText(code: code),
            const SizedBox(height: 4),
            Text(l10n.syncWaitingDevice),
          ],
        );
      case _PairingHostStatus.completed:
        return _SuccessBanner(message: l10n.syncPaired);
      case _PairingHostStatus.failed:
        return _pairingCode == null ? const SizedBox.shrink() : _PairingCodeText(code: _pairingCode!);
    }
  }

  @override
  Widget build(BuildContext context) {
    final status = _status;
    final l10n = AppLocalizations.of(context);
    return Scaffold(
      appBar: AppBar(title: Text(l10n.syncTitle)),
      body: status == null
          ? const Center(child: CircularProgressIndicator())
          : ListView(
              padding: const EdgeInsets.all(16),
              children: [
                Text(l10n.syncIntro),
                const SizedBox(height: 16),
                if (_error != null) _ErrorBanner(message: _error!),
                _StatusCard(status: status),
                const SizedBox(height: 16),
                if (!status.paired) ...[
                  _Section(
                    title: l10n.syncSetUp,
                    children: [
                      Text(l10n.syncSetUpHint),
                      const SizedBox(height: 12),
                      FilledButton(
                        onPressed: _busy ? null : _handleInit,
                        child: Text(l10n.syncInit),
                      ),
                    ],
                  ),
                ] else ...[
                  _Section(
                    title: l10n.syncSendPull,
                    children: [
                      Row(
                        children: [
                          FilledButton(onPressed: _busy ? null : _handleSend, child: Text(l10n.syncSend)),
                          const SizedBox(width: 12),
                          OutlinedButton(onPressed: _busy ? null : _handlePull, child: Text(l10n.syncPull)),
                        ],
                      ),
                      if (_pushBegin case final begin?) _PushQrCard(begin: begin),
                      if (_pushResult case final result?) ...[
                        const SizedBox(height: 12),
                        _SuccessBanner(
                          message: l10n.syncSent(result.txId, result.filesChanged),
                        ),
                      ],
                      if (_pullResult case final result?) ...[
                        const SizedBox(height: 12),
                        _SuccessBanner(
                          message: l10n.syncPulled(result.filesWritten, result.filesDeleted, result.configUpdated ? l10n.syncConfigUpdated : ''),
                        ),
                        for (final warning in result.warnings)
                          Padding(padding: const EdgeInsets.only(top: 4), child: Text('• $warning')),
                      ],
                    ],
                  ),
                ],
                const SizedBox(height: 16),
                _Section(
                  title: l10n.syncPairDevice,
                  children: [
                    if (status.paired) ...[
                      Text(l10n.syncShowThisCode),
                      const SizedBox(height: 8),
                      _buildPairingHostStatus(l10n),
                      const SizedBox(height: 16),
                    ],
                    Text(l10n.syncHaveCode),
                    const SizedBox(height: 8),
                    TextField(
                      controller: _joinCodeController,
                      enabled: !_joining,
                      decoration: InputDecoration(labelText: l10n.syncCodeLabel, hintText: 'XXXXXXXX'),
                      textCapitalization: TextCapitalization.characters,
                    ),
                    const SizedBox(height: 8),
                    TextField(
                      controller: _joinHostController,
                      enabled: !_joining,
                      decoration: InputDecoration(
                        labelText: l10n.syncHostLabel,
                        hintText: l10n.syncHostHint,
                      ),
                    ),
                    const SizedBox(height: 12),
                    FilledButton(
                      onPressed: _joining || _joinCodeController.text.trim().isEmpty ? null : _handleJoin,
                      child: Text(l10n.syncPair),
                    ),
                  ],
                ),
              ],
            ),
    );
  }
}

class _StatusCard extends StatelessWidget {
  const _StatusCard({required this.status});

  final bridge.SyncStatusDto status;

  String _formatTimestamp(int? ms, AppLocalizations l10n) {
    if (ms == null) return l10n.syncNever;
    return DateTime.fromMillisecondsSinceEpoch(ms).toLocal().toString();
  }

  @override
  Widget build(BuildContext context) {
    final l10n = AppLocalizations.of(context);
    return _Section(
      title: l10n.syncStatus,
      children: [
        _StatusRow(label: l10n.syncStatusPaired, value: status.paired ? l10n.syncYes : l10n.syncNo),
        _StatusRow(label: l10n.syncOwner, value: status.ownerAddress ?? '—'),
        _StatusRow(label: l10n.syncLast, value: _formatTimestamp(status.lastSyncedAtMs, l10n)),
        _StatusRow(
          label: l10n.syncPending,
          value: l10n.syncPendingValue(status.pendingVaultChanges, status.pendingConfigChanged ? l10n.syncPlusConfig : ''),
        ),
      ],
    );
  }
}

class _StatusRow extends StatelessWidget {
  const _StatusRow({required this.label, required this.value});

  final String label;
  final String value;

  @override
  Widget build(BuildContext context) {
    return Padding(
      padding: const EdgeInsets.symmetric(vertical: 2),
      child: Row(
        mainAxisAlignment: MainAxisAlignment.spaceBetween,
        children: [
          Text(label, style: Theme.of(context).textTheme.bodyMedium),
          Text(value, style: Theme.of(context).textTheme.bodyMedium?.copyWith(fontWeight: FontWeight.w600)),
        ],
      ),
    );
  }
}

class _PushQrCard extends StatelessWidget {
  const _PushQrCard({required this.begin});

  final bridge.PushBeginDto begin;

  @override
  Widget build(BuildContext context) {
    final l10n = AppLocalizations.of(context);
    return Padding(
      padding: const EdgeInsets.only(top: 12),
      child: Column(
        children: [
          Text(l10n.syncScan(begin.filesChanged, begin.configChanged ? l10n.syncPlusConfig : '')),
          const SizedBox(height: 12),
          QrImageView(data: begin.qrPayloadJson, size: 220),
          const SizedBox(height: 8),
          Text(l10n.syncWaitingApproval),
        ],
      ),
    );
  }
}

class _PairingCodeText extends StatelessWidget {
  const _PairingCodeText({required this.code});

  final String code;

  @override
  Widget build(BuildContext context) {
    return SelectableText(code, style: Theme.of(context).textTheme.headlineSmall?.copyWith(letterSpacing: 4));
  }
}

class _Section extends StatelessWidget {
  const _Section({required this.title, required this.children});

  final String title;
  final List<Widget> children;

  @override
  Widget build(BuildContext context) {
    return Card(
      child: Padding(
        padding: const EdgeInsets.all(16),
        child: Column(
          crossAxisAlignment: CrossAxisAlignment.start,
          children: [
            Text(title, style: Theme.of(context).textTheme.titleMedium),
            const SizedBox(height: 12),
            ...children,
          ],
        ),
      ),
    );
  }
}

class _ErrorBanner extends StatelessWidget {
  const _ErrorBanner({required this.message});

  final String message;

  @override
  Widget build(BuildContext context) {
    return Container(
      margin: const EdgeInsets.only(bottom: 12),
      padding: const EdgeInsets.all(12),
      decoration: BoxDecoration(color: Theme.of(context).colorScheme.errorContainer, borderRadius: BorderRadius.circular(8)),
      child: Text(message, style: TextStyle(color: Theme.of(context).colorScheme.onErrorContainer)),
    );
  }
}

class _SuccessBanner extends StatelessWidget {
  const _SuccessBanner({required this.message});

  final String message;

  @override
  Widget build(BuildContext context) {
    return Container(
      padding: const EdgeInsets.all(12),
      decoration: BoxDecoration(color: Theme.of(context).colorScheme.secondaryContainer, borderRadius: BorderRadius.circular(8)),
      child: Text(message, style: TextStyle(color: Theme.of(context).colorScheme.onSecondaryContainer)),
    );
  }
}
