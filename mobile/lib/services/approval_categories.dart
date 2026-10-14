/// P122 — the kinds of action an agent can be made to get approved even when it acts on its own. The ids are those of
/// `warden_core::autonomy::Category`, as the hub and `AgentInfo.approvalRequired` write them (the same list as the web's).
const approvalCategories = <({String id, String label})>[
  (id: 'delete_data', label: 'Delete data'),
  (id: 'spend_money', label: 'Spend money'),
  (id: 'critical_infra', label: 'Touch critical infrastructure (shell, SSH, nodes)'),
  (id: 'external_message', label: 'Send an external message (browser, outside services)'),
  (id: 'publish_code', label: 'Publish code'),
  (id: 'important_config', label: 'Change important configuration (scheduled tasks)'),
  (id: 'elevated_agent', label: 'Create or change agents'),
];

/// The five autonomy levels, in one line each.
const autonomyLevels = <({int level, String label})>[
  (level: 1, label: '1 — only answers, no tools'),
  (level: 2, label: '2 — suggests: reads, refuses the rest with a suggestion'),
  (level: 3, label: '3 — asks for your yes before any change'),
  (level: 4, label: '4 — acts on its own (the checked kinds still ask)'),
  (level: 5, label: '5 — also manages its reports on its own'),
];

/// The name of a kind of action, or its own id when a newer hub sends one this screen does not know.
String approvalCategoryLabel(String id) {
  for (final c in approvalCategories) {
    if (c.id == id) return c.label;
  }
  return id;
}

/// [marked] in a fixed order, each once: the kinds this screen knows in the list's order, then the ones only the hub knows, so the
/// same set always makes the same edit.
List<String> orderedCategories(Iterable<String> marked) {
  final set = marked.toSet();
  final known = [for (final c in approvalCategories) if (set.contains(c.id)) c.id];
  final unknown = [for (final id in marked.toSet()) if (!approvalCategories.any((c) => c.id == id)) id];
  return [...known, ...unknown];
}
