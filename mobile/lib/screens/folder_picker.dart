import 'package:flutter/material.dart';

import '../l10n/generated/app_localizations.dart';
import '../protocol/messages.dart';
import '../services/workdir.dart';

/// P102 — the last segment of a folder path, for the chip ("/srv/work/alpha" → "alpha").
String folderName(String path) {
  final parts = path.split('/').where((p) => p.isNotEmpty);
  return parts.isEmpty ? path : parts.last;
}

typedef ListDirs = Future<DirListMessage> Function([String? path]);

/// Opens the folder browser as a bottom sheet and returns the folder picked, or null when it was cancelled.
/// [initialPath] is where it opens (the current choice); a folder that is gone falls back to the start.
/// [nodes] are the machines besides the hub's a folder can be picked on (the owner's; none for a member), chosen with a selector.
Future<String?> showFolderPicker(BuildContext context, {required ListDirs listDirs, String? initialPath, List<NodeInfo> nodes = const []}) {
  return showModalBottomSheet<String>(
    context: context,
    isScrollControlled: true,
    useSafeArea: true,
    builder: (_) => FolderPicker(listDirs: listDirs, initialPath: initialPath, nodes: nodes),
  );
}

/// Browses the folders of the hub's machine to choose the one a conversation works in (P102). The hub lists
/// folders only; a member sees just the ones the owner allowed, so at the top their list has no path to pick.
class FolderPicker extends StatefulWidget {
  const FolderPicker({super.key, required this.listDirs, this.initialPath, this.nodes = const []});

  final ListDirs listDirs;
  final String? initialPath;

  /// The nodes a folder can be picked on besides the hub's machine; with none the machine selector is not shown.
  final List<NodeInfo> nodes;

  @override
  State<FolderPicker> createState() => _FolderPickerState();
}

class _FolderPickerState extends State<FolderPicker> {
  DirListMessage? _listing;
  String? _error;
  bool _loading = true;

  @override
  void initState() {
    super.initState();
    _openInitial();
  }

  /// Opens once. A remembered folder that is gone falls back to the start instead of an error.
  Future<void> _openInitial() async {
    try {
      DirListMessage listing;
      try {
        listing = await widget.listDirs(widget.initialPath);
      } catch (_) {
        if (widget.initialPath == null) rethrow;
        listing = await widget.listDirs();
      }
      if (!mounted) return;
      setState(() {
        _listing = listing;
        _loading = false;
      });
    } catch (e) {
      if (!mounted) return;
      setState(() {
        _error = e.toString();
        _loading = false;
      });
    }
  }

  Future<void> _open(String? path) async {
    setState(() {
      _loading = true;
      _error = null;
    });
    try {
      final listing = await widget.listDirs(path);
      if (!mounted) return;
      setState(() {
        _listing = listing;
        _loading = false;
      });
    } catch (e) {
      if (!mounted) return;
      setState(() {
        _error = e.toString();
        _loading = false;
      });
    }
  }

  @override
  Widget build(BuildContext context) {
    final listing = _listing;
    final here = listing?.path ?? '';
    final parent = listing?.parent;
    final machine = parseNodeFolder(here)?.node ?? '';
    final l10n = AppLocalizations.of(context);
    return SafeArea(
      child: Padding(
        padding: const EdgeInsets.all(16),
        child: Column(
          mainAxisSize: MainAxisSize.min,
          crossAxisAlignment: CrossAxisAlignment.start,
          children: [
            Text(l10n.folderTitle, style: Theme.of(context).textTheme.titleLarge),
            const SizedBox(height: 4),
            Text(l10n.folderHint, style: Theme.of(context).textTheme.bodySmall),
            const SizedBox(height: 12),
            if (widget.nodes.isNotEmpty) ...[
              DropdownButton<String>(
                key: const Key('folder-machine'),
                isExpanded: true,
                // A node that is not in the list (gone offline since) shows as the hub, which is where "Up" and the start lead.
                value: widget.nodes.any((n) => n.deviceId == machine) ? machine : '',
                items: [
                  DropdownMenuItem(value: '', child: Text(l10n.folderThisHub)),
                  for (final node in widget.nodes) DropdownMenuItem(value: node.deviceId, child: Text(node.name.isEmpty ? node.deviceId : node.name)),
                ],
                onChanged: _loading ? null : (id) => _open(id == null || id.isEmpty ? null : nodeFolderRef(id, '')),
              ),
              const SizedBox(height: 8),
            ],
            Text(here.isEmpty ? l10n.folderYours : folderPlace(here, widget.nodes), key: const Key('folder-here'), style: const TextStyle(fontFamily: 'monospace')),
            if (_error != null) ...[
              const SizedBox(height: 8),
              Text(_error!, key: const Key('folder-error'), style: TextStyle(color: Theme.of(context).colorScheme.error)),
            ],
            const SizedBox(height: 8),
            Flexible(
              child: _loading && listing == null
                  ? const Center(child: Padding(padding: EdgeInsets.all(24), child: CircularProgressIndicator()))
                  : ListView(
                      shrinkWrap: true,
                      children: [
                        if (parent != null)
                          ListTile(
                            key: const Key('folder-up'),
                            leading: const Icon(Icons.arrow_upward),
                            title: Text(l10n.folderUp),
                            enabled: !_loading,
                            // An empty parent is the top of a member's list.
                            onTap: () => _open(parent.isEmpty ? null : parent),
                          ),
                        for (final dir in listing?.dirs ?? const <DirEntry>[])
                          ListTile(
                            leading: const Icon(Icons.folder_outlined),
                            title: Text(dir.name),
                            enabled: !_loading,
                            onTap: () => _open(dir.path),
                          ),
                        if (listing != null && listing.dirs.isEmpty && !_loading)
                          Padding(padding: const EdgeInsets.all(12), child: Text(l10n.folderEmpty, key: const Key('folder-empty'))),
                      ],
                    ),
            ),
            const SizedBox(height: 8),
            Row(
              mainAxisAlignment: MainAxisAlignment.end,
              children: [
                TextButton(onPressed: () => Navigator.of(context).pop(), child: Text(l10n.commonCancel)),
                const SizedBox(width: 8),
                FilledButton(
                  key: const Key('folder-use'),
                  onPressed: here.isEmpty || _loading ? null : () => Navigator.of(context).pop(here),
                  child: Text(l10n.folderUse),
                ),
              ],
            ),
          ],
        ),
      ),
    );
  }
}
