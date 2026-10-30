import 'package:flutter/material.dart';

import '../l10n/generated/app_localizations.dart';
import '../services/skill_draft.dart';
import '../services/skills_repository.dart';
import 'catalog_descriptions.dart';
import 'skill_diff.dart';

/// P72 (b) — lists, creates, edits and deletes the skills stored in this device's local vault
/// (`skills/<name>.md`, see `SkillsRepository`). Mirrors the desktop `SkillsView.tsx`. The phone has no model of its
/// own, so the "describe it and the AI drafts it" box asks the hub, and only shows while connected to one
/// ([draftBackend]). Skills written here reach the desktop/server through the normal vault sync
/// (`SyncScreen`), and the model only sees the ones in the vault it runs against.
class SkillsScreen extends StatefulWidget {
  const SkillsScreen({super.key, this.repository = const BridgeSkillsRepository(), this.draftBackend});

  final SkillsRepository repository;

  /// The hub connection that writes drafts; null when there is none, and the draft box is left out.
  final SkillDraftBackend? draftBackend;

  @override
  State<SkillsScreen> createState() => _SkillsScreenState();
}

class _SkillsScreenState extends State<SkillsScreen> {
  List<SkillDto>? _skills;
  // The gallery of ready-made skills (P140); stays null if it can't be read, and the section is then left out.
  List<CatalogSkillDto>? _catalog;
  String? _installing;
  String? _confirmReplace;
  String? _error;

  @override
  void initState() {
    super.initState();
    _load();
  }

  Future<void> _load() async {
    try {
      final skills = await widget.repository.list();
      final catalog = await _readCatalog();
      if (mounted) {
        setState(() {
          _skills = skills;
          _catalog = catalog;
          _error = null;
        });
      }
    } catch (e) {
      if (mounted) setState(() => _error = e.toString());
    }
  }

  Future<List<CatalogSkillDto>?> _readCatalog() async {
    try {
      return await widget.repository.listCatalog();
    } catch (_) {
      return null;
    }
  }

  Future<void> _install(String name, {required bool overwrite}) async {
    setState(() {
      _installing = name;
      _error = null;
    });
    try {
      await widget.repository.installCatalog(name, overwrite: overwrite);
      _confirmReplace = null;
      await _load();
    } catch (e) {
      if (mounted) setState(() => _error = AppLocalizations.of(context).skillsInstallFailed(e.toString()));
    } finally {
      if (mounted) setState(() => _installing = null);
    }
  }

  /// The gallery section: each entry is installed, installable, or differs from a skill of the same name
  /// that the person already has (replacing it takes a second, explicit tap).
  Widget _gallery(List<CatalogSkillDto> catalog) {
    final l10n = AppLocalizations.of(context);
    final locale = Localizations.localeOf(context);
    final theme = Theme.of(context);
    return Column(
      crossAxisAlignment: CrossAxisAlignment.start,
      children: [
        Padding(padding: const EdgeInsets.only(top: 16, bottom: 4), child: Text(l10n.skillsGallery, style: theme.textTheme.titleSmall)),
        Padding(padding: const EdgeInsets.only(bottom: 8), child: Text(l10n.skillsGalleryIntro)),
        for (final entry in catalog)
          Card(
            child: Padding(
              padding: const EdgeInsets.all(12),
              child: Column(
                crossAxisAlignment: CrossAxisAlignment.start,
                children: [
                  Row(
                    children: [
                      Expanded(child: Text(entry.name, style: theme.textTheme.titleMedium)),
                      if (entry.status == 'installed')
                        Text(l10n.skillsGalleryInstalled, style: theme.textTheme.bodySmall)
                      else if (entry.status == 'outdated')
                        FilledButton(
                          onPressed: _installing == entry.name ? null : () => _install(entry.name, overwrite: false),
                          child: Text(_installing == entry.name ? l10n.skillsUpdating : l10n.skillsUpdate),
                        )
                      else if (entry.status != 'modified')
                        FilledButton(
                          onPressed: _installing == entry.name ? null : () => _install(entry.name, overwrite: false),
                          child: Text(_installing == entry.name ? l10n.skillsInstalling : l10n.skillsInstall),
                        ),
                    ],
                  ),
                  Padding(padding: const EdgeInsets.only(top: 4), child: Text(catalogDescription(locale, entry.name, entry.description))),
                  if (entry.status == 'outdated')
                    Padding(padding: const EdgeInsets.only(top: 4), child: Text(l10n.skillsUpdateAvailable, style: theme.textTheme.bodySmall)),
                  if (entry.status == 'modified')
                    Padding(
                      padding: const EdgeInsets.only(top: 4),
                      child: _confirmReplace == entry.name
                          ? Wrap(
                              spacing: 8,
                              children: [
                                FilledButton(
                                  onPressed: _installing == entry.name ? null : () => _install(entry.name, overwrite: true),
                                  child: Text(l10n.skillsReplaceMine),
                                ),
                                TextButton(onPressed: () => setState(() => _confirmReplace = null), child: Text(l10n.skillsKeepMine)),
                              ],
                            )
                          : Row(
                              children: [
                                Expanded(child: Text(l10n.skillsOtherSameName, style: theme.textTheme.bodySmall)),
                                TextButton(onPressed: () => setState(() => _confirmReplace = entry.name), child: Text(l10n.skillsUseGallery)),
                              ],
                            ),
                    ),
                  ExpansionTile(
                    tilePadding: EdgeInsets.zero,
                    title: Text(l10n.skillsViewInstructions, style: theme.textTheme.bodyMedium),
                    childrenPadding: const EdgeInsets.only(bottom: 4),
                    children: [Align(alignment: Alignment.centerLeft, child: Text(entry.body))],
                  ),
                ],
              ),
            ),
          ),
      ],
    );
  }

  Future<void> _openForm({SkillDto? existing}) async {
    final saved = await Navigator.of(context).push<bool>(
      MaterialPageRoute(builder: (_) => SkillFormScreen(repository: widget.repository, existing: existing, draftBackend: widget.draftBackend)),
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
              : ListView(
                      padding: const EdgeInsets.fromLTRB(16, 8, 16, 88),
                      children: [
                        if (_error != null)
                          Padding(
                            padding: const EdgeInsets.only(bottom: 8),
                            child: Text(_error!, style: TextStyle(color: Theme.of(context).colorScheme.error)),
                          ),
                        if (skills.isEmpty)
                          Padding(
                            padding: const EdgeInsets.all(24),
                            child: Text(
                              l10n.skillsNone,
                              textAlign: TextAlign.center,
                            ),
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
                        if (_catalog != null && _catalog!.isNotEmpty) _gallery(_catalog!),
                      ],
                    ),
    );
  }
}

/// New/edit form. `existing == null` is "create" (name editable, refuses a taken name); otherwise
/// the name — which is the filename — is locked and saving overwrites. Pops `true` once saved.
class SkillFormScreen extends StatefulWidget {
  const SkillFormScreen({super.key, required this.repository, this.existing, this.draftBackend});

  final SkillsRepository repository;
  final SkillDto? existing;

  /// Writes a draft into the form; only offered when creating, and only with a hub connection.
  final SkillDraftBackend? draftBackend;

  @override
  State<SkillFormScreen> createState() => _SkillFormScreenState();
}

class _SkillFormScreenState extends State<SkillFormScreen> {
  late final _name = TextEditingController(text: widget.existing?.name ?? '');
  late final _description = TextEditingController(text: widget.existing?.description ?? '');
  late final _body = TextEditingController(text: widget.existing?.body ?? '');
  final _draftRequest = TextEditingController();
  bool _saving = false;
  bool _drafting = false;
  String? _error;
  // Shown under the draft box, not at the bottom of the form, where the person is not looking.
  String? _draftError;
  // Editing a suggestion keeps it pending unless this is switched on.
  bool _accept = false;

  bool get _isEdit => widget.existing != null;
  bool get _isSuggestion => widget.existing?.proposed ?? false;

  @override
  void dispose() {
    _name.dispose();
    _description.dispose();
    _body.dispose();
    _draftRequest.dispose();
    super.dispose();
  }

  // The hub's model writes the three fields; nothing is saved until the person reviews them and taps Save.
  Future<void> _draft() async {
    final l10n = AppLocalizations.of(context);
    setState(() {
      _drafting = true;
      _draftError = null;
    });
    try {
      final draft = await widget.draftBackend!.draftSkill(_draftRequest.text.trim());
      if (!mounted) return;
      setState(() {
        _name.text = draft.name;
        _description.text = draft.description;
        _body.text = draft.body;
        _draftRequest.clear();
      });
    } catch (e) {
      if (mounted) setState(() => _draftError = l10n.skillsDraftFailed(e.toString()));
    } finally {
      if (mounted) setState(() => _drafting = false);
    }
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
          if (!_isEdit && widget.draftBackend != null) ...[
            TextField(
              controller: _draftRequest,
              maxLength: 2000,
              minLines: 2,
              maxLines: 4,
              onChanged: (_) => setState(() {}),
              decoration: InputDecoration(
                labelText: l10n.skillsDraftLabel,
                helperText: l10n.skillsDraftHint,
                helperMaxLines: 3,
                border: const OutlineInputBorder(),
              ),
            ),
            const SizedBox(height: 8),
            Align(
              alignment: Alignment.centerRight,
              child: OutlinedButton(
                onPressed: _drafting || _saving || _draftRequest.text.trim().isEmpty ? null : _draft,
                child: Text(_drafting ? l10n.skillsDrafting : l10n.skillsDraftButton),
              ),
            ),
            if (_draftError != null)
              Padding(
                padding: const EdgeInsets.only(top: 8),
                child: Text(_draftError!, style: TextStyle(color: Theme.of(context).colorScheme.error)),
              ),
            const SizedBox(height: 16),
          ],
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
