// A conversation's working folder (P102) is one string: a path of the hub's own machine ("/srv/work"), or a folder on a node, written
// `node:<device id>:<path>` with the path relative to the folder the node lends ("" is that folder itself). Mirrors the web's `hub/workdir.ts`.

import '../protocol/messages.dart';

const _nodePrefix = 'node:';

/// `(node, path)` of a folder on a node, or null for a folder of the hub's own machine.
({String node, String path})? parseNodeFolder(String workdir) {
  if (!workdir.startsWith(_nodePrefix)) return null;
  final rest = workdir.substring(_nodePrefix.length);
  final colon = rest.indexOf(':');
  if (colon <= 0) return null;
  return (node: rest.substring(0, colon), path: rest.substring(colon + 1));
}

String nodeFolderRef(String node, String path) => '$_nodePrefix$node:$path';

String _lastName(String path) {
  final parts = path.split('/').where((p) => p.isNotEmpty);
  return parts.isEmpty ? path : parts.last;
}

String _machineName(String node, List<NodeInfo> nodes) {
  for (final n in nodes) {
    if (n.deviceId == node && n.name.isNotEmpty) return n.name;
  }
  return node;
}

/// What the chip shows: the folder's name, and which machine when it isn't the hub's ("web · Home PC").
String folderLabel(String workdir, [List<NodeInfo> nodes = const []]) {
  final onNode = parseNodeFolder(workdir);
  if (onNode == null) return _lastName(workdir);
  return '${onNode.path.isEmpty ? 'shared folder' : _lastName(onNode.path)} · ${_machineName(onNode.node, nodes)}';
}

/// The full place, for the browser's header and the chip's tooltip ("Home PC: /projects/web").
String folderPlace(String workdir, [List<NodeInfo> nodes = const []]) {
  final onNode = parseNodeFolder(workdir);
  if (onNode == null) return workdir;
  return '${_machineName(onNode.node, nodes)}: /${onNode.path}';
}
