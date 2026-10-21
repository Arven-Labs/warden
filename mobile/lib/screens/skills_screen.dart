import 'package:flutter/material.dart';

import '../l10n/generated/app_localizations.dart';
import '../services/skills_repository.dart';
import 'skill_diff.dart';

/// P72 (b) — lists, creates, edits and deletes the skills stored in this device's local vault
/// (`skills/<name>.md`, see `SkillsRepository`). Mirrors the desktop `SkillsView.tsx` minus the
/// "describe it and the AI drafts it" box: the phone is a pure `warden-server` client with no model
/// of its own to ask. Skills written here reach the desktop/server through the normal vault sync
/// (`SyncScreen`), and the model only sees the ones in the vault it runs against.
class SkillsScreen extends StatefulWidget {
  const SkillsScreen({super.key, this.repository = const BridgeSkillsRepository()});

  final SkillsRepository repository;

  @override
  State<SkillsScreen> createState() => _SkillsScreenState();
}

class _SkillsScreenState extends State<SkillsScreen> {
  List<SkillDto>? _skills;
  String? _error;

  @override
  void initState() {
    super.initState();
    _load();
  }

  Future<void> _load() async {
    try {
      final skills = await widget.repository.list();
      if (mounted) {
        setState(() {
          _skills = skills;
          _error = null;
        });
      }
    } catch (e) {
      if (mounted) setState(() => _error = e.toString());
    }
  }

  Future<void> _openForm({SkillDto? existing}) async {
    final saved = await Navigator.of(context).push<bool>(
      MaterialPageRoute(builder: (_) => SkillFormScreen(repository: widget.repository, existing: existing)),
    );
    if (saved == true) await _load();
  }

  Future<void> _confirmReject(SkillDto skill) => _confirmDelete(skill, reject: true);

  Future<void> _confirmDelete(SkillDto skill, {bool reject = false}) async {
    final l10n = AppLocalizations.of(context);
    final confirmed = await showDialog<bool>(
      context: context,
      builder: (context) => AlertDialog(
        title: Text(reject ? l10n.skillsRejectTitle(skill.name) : l10n.skillsDeleteTitle(skill.name)),
        content: Text(l10n.skillsDeleteBody),
        actions: [
          TextButton(onPressed: () => Navigator.of(context).pop(false), child: Text(l10n.commonCancel)),
          TextButton(onPressed: () => Navigator.of(context).pop(true), child: Text(reject ? l10n.skillsReject : l10n.commonDelete)),
        ],
      ),
    );
    if (confirmed != true) return;
    try {
      await widget.repository.delete(skill.name);
      await _load();
    } catch (e) {
      if (mounted) setState(() => _error = e.toString());
    }
  }

  /// Accepting is an explicit save with `proposed: false` — the bridge keeps a suggestion pending on
  /// any other save, so editing never activates it by accident.
  Future<void> _accept(SkillDto skill) async {
    try {
      await widget.repository.save(
        SkillDto(name: skill.name, description: skill.description, body: skill.body, proposed: false),
        overwrite: true,
      );
      await _load();
    } catch (e) {
      if (mounted) setState(() => _error = e.toString());
    }
  }

  /// "See what changes" for a suggested revision: the target's current text against the new one.
  Widget _diffView(SkillDto target, SkillDto revision) {
    final diff = diffLines(target.body, revision.body);
    final scheme = Theme.of(context).colorScheme;
    final l10n = AppLocalizations.of(context);
    final mono = Theme.of(context).textTheme.bodySmall?.copyWith(fontFamily: 'monospace');
    return ExpansionTile(
      tilePadding: EdgeInsets.zero,
      title: Text(l10n.skillsSeeChanges(target.name), style: Theme.of(context).textTheme.bodyMedium),
      childrenPadding: const EdgeInsets.only(bottom: 4),
      children: [
        if (!hasChanges(diff))
          Align(alignment: Alignment.centerLeft, child: Text(l10n.skillsNoChange))
        else
          for (final line in withContext(diff))
            Container(
              width: double.infinity,
              color: switch (line.op) {
                DiffOp.add => Colors.green.withValues(alpha: 0.18),
                DiffOp.del => scheme.error.withValues(alpha: 0.15),
                _ => null,
              },
              child: Text(
                '${switch (line.op) { DiffOp.add => '+ ', DiffOp.del => '- ', _ => '  ' }}${line.op == DiffOp.gap && line.count != null ? (line.count == 1 ? l10n.diffGapOne : l10n.diffGapMany(line.count!)) : line.text}',
                style: line.op == DiffOp.same || line.op == DiffOp.gap ? mono?.copyWith(color: scheme.onSurfaceVariant) : mono,
              ),
            ),
      ],
    );
  }

  Widget _suggestedCard(SkillDto skill, List<SkillDto> active) {
    final origin = skill.source;
    final revises = skill.revises;
    final target = revises == null ? null : active.where((s) => s.name == revises).firstOrNull;
    final l10n = AppLocalizations.of(context);
    return Card(
      child: Padding(
        padding: const EdgeInsets.all(12),
        child: Column(
          crossAxisAlignment: CrossAxisAlignment.start,
          children: [
            Row(
              children: [
                Expanded(child: Text(skill.name, style: Theme.of(context).textTheme.titleMedium)),
                Chip(label: Text(skill.revises == null ? l10n.skillsSuggested : l10n.skillsChangeTo(skill.revises!)), visualDensity: VisualDensity.compact),
              ],
            ),
            if (skill.description.isNotEmpty) Padding(padding: const EdgeInsets.only(top: 4), child: Text(skill.description)),
            if (origin != null && origin.isNotEmpty) Padding(padding: const EdgeInsets.only(top: 4), child: Text(l10n.skillsFrom(origin), style: Theme.of(context).textTheme.bodySmall)),
            if (target != null) _diffView(target, skill),
            Padding(
              padding: const EdgeInsets.only(top: 8),
              child: Wrap(
                spacing: 8,
                children: [
                  FilledButton(onPressed: () => _accept(skill), child: Text(l10n.skillsAccept)),
                  OutlinedButton(onPressed: () => _openForm(existing: skill), child: Text(l10n.commonEdit)),
                  TextButton(onPressed: () => _confirmReject(skill), child: Text(l10n.skillsReject)),
                ],
              ),
            ),
          ],
        ),
      ),
    );
  }

  @override
  Widget build(BuildContext context) {
    final skills = _skills;
    final l10n = AppLocalizations.of(context);
    final suggested = (skills ?? const <SkillDto>[]).where((s) => s.proposed).toList();
    final active = (skills ?? const <SkillDto>[]).where((s) => !s.proposed).toList();
    return Scaffold(
      appBar: AppBar(title: Text(l10n.skillsTitle)),
      floatingActionButton: FloatingActionButton.extended(
        onPressed: () => _openForm(),
        icon: const Icon(Icons.add),
        label: Text(l10n.skillsNew),
      ),
      body: _error != null && skills == null
          ? Padding(padding: const EdgeInsets.all(16), child: Text(_error!, style: TextStyle(color: Theme.of(context).colorScheme.error)))
          : skills == null
              ? const Center(child: CircularProgressIndicator())
              : skills.isEmpty
                  ? Center(
                      child: Padding(
                        padding: const EdgeInsets.all(24),
                        child: Text(
                          l10n.skillsNone,
                          textAlign: TextAlign.center,
                        ),
                      ),
                    )
                  : ListView(
                      padding: const EdgeInsets.fromLTRB(16, 8, 16, 88),
                      children: [
                        if (_error != null)
                          Padding(
                            padding: const EdgeInsets.only(bottom: 8),
                            child: Text(_error!, style: TextStyle(color: Theme.of(context).colorScheme.error)),
                          ),
                        if (suggested.isNotEmpty) ...[
                          Padding(
                            padding: const EdgeInsets.only(top: 4, bottom: 4),
                            child: Text(l10n.skillsSuggestedTitle, style: Theme.of(context).textTheme.titleSmall),
                          ),
                          Padding(
                            padding: const EdgeInsets.only(bottom: 8),
                            child: Text(l10n.skillsSuggestedNote),
                          ),
                          for (final skill in suggested) _suggestedCard(skill, active),
                          const SizedBox(height: 16),
                          if (active.isNotEmpty) Text(l10n.skillsYours, style: Theme.of(context).textTheme.titleSmall),
                        ],
                        for (final skill in active)
                          Card(
                            child: ListTile(
                              title: Text(skill.name),
                              subtitle: Text(skill.description.isEmpty ? l10n.skillsNoDescription : skill.description, maxLines: 2, overflow: TextOverflow.ellipsis),
                              onTap: () => _openForm(existing: skill),
                              trailing: IconButton(
                                icon: const Icon(Icons.delete_outline),
                                tooltip: l10n.skillsDeleteTooltip(skill.name),
                                onPressed: () => _confirmDelete(skill),
                              ),
                            ),
                          ),
                      ],
                    ),
    );
  }
}

/// New/edit form. `existing == null` is "create" (name editable, refuses a taken name); otherwise
/// the name — which is the filename — is locked and saving overwrites. Pops `true` once saved.
class SkillFormScreen extends StatefulWidget {
  const SkillFormScreen({super.key, required this.repository, this.existing});

  final SkillsRepository repository;
  final SkillDto? existing;

  @override
  State<SkillFormScreen> createState() => _SkillFormScreenState();
}

class _SkillFormScreenState extends State<SkillFormScreen> {
  late final _name = TextEditingController(text: widget.existing?.name ?? '');
  late final _description = TextEditingController(text: widget.existing?.description ?? '');
  late final _body = TextEditingController(text: widget.existing?.body ?? '');
  bool _saving = false;
  String? _error;
  // Editing a suggestion keeps it pending unless this is switched on.
  bool _accept = false;

  bool get _isEdit => widget.existing != null;
  bool get _isSuggestion => widget.existing?.proposed ?? false;

  @override
  void dispose() {
    _name.dispose();
    _description.dispose();
    _body.dispose();
    super.dispose();
  }

  Future<void> _save() async {
    setState(() {
      _saving = true;
      _error = null;
    });
    try {
      await widget.repository.save(
        SkillDto(
          name: _name.text.trim(),
          description: _description.text,
          body: _body.text,
          proposed: _isSuggestion && !_accept,
          source: widget.existing?.source,
          proposedAt: widget.existing?.proposedAt,
        ),
        overwrite: _isEdit,
      );
      if (mounted) Navigator.of(context).pop(true);
    } catch (e) {
      if (mounted) setState(() => _error = e.toString());
    } finally {
      if (mounted) setState(() => _saving = false);
    }
  }

  @override
  Widget build(BuildContext context) {
    final l10n = AppLocalizations.of(context);
    return Scaffold(
      appBar: AppBar(title: Text(_isEdit ? l10n.skillsEdit : l10n.skillsNew)),
      body: ListView(
        padding: const EdgeInsets.all(16),
        children: [
          TextField(
            controller: _name,
            enabled: !_isEdit,
            autocorrect: false,
            decoration: InputDecoration(
              labelText: l10n.skillsName,
              helperText: l10n.skillsNameHelper,
              border: const OutlineInputBorder(),
            ),
          ),
          const SizedBox(height: 16),
          TextField(
            controller: _description,
            maxLength: 300,
            decoration: InputDecoration(
              labelText: l10n.skillsDescription,
              helperText: l10n.skillsDescriptionHelper,
              border: const OutlineInputBorder(),
            ),
          ),
          const SizedBox(height: 16),
          TextField(
            controller: _body,
            minLines: 8,
            maxLines: null,
            keyboardType: TextInputType.multiline,
            decoration: InputDecoration(
              labelText: l10n.skillsInstructions,
              alignLabelWithHint: true,
              border: const OutlineInputBorder(),
            ),
          ),
          if (_isSuggestion)
            SwitchListTile(
              contentPadding: EdgeInsets.zero,
              title: Text(l10n.skillsAcceptThis),
              subtitle: Text(l10n.skillsAcceptThisHint),
              value: _accept,
              onChanged: (v) => setState(() => _accept = v),
            ),
          if (_error != null)
            Padding(
              padding: const EdgeInsets.only(top: 12),
              child: Text(_error!, style: TextStyle(color: Theme.of(context).colorScheme.error)),
            ),
          const SizedBox(height: 16),
          FilledButton(onPressed: _saving ? null : _save, child: Text(_saving ? l10n.skillsSaving : l10n.commonSave)),
        ],
      ),
    );
  }
}
