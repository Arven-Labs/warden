import 'package:flutter_test/flutter_test.dart';
import 'package:mobile/protocol/messages.dart';
import 'package:mobile/services/workdir.dart';

/// P102 — a working folder is a path of the hub, or `node:<id>:<path>` on a node; these are the same rules as the web's `hub/workdir.ts`.
void main() {
  const nodes = [NodeInfo(deviceId: 'node-a', name: 'Home PC', online: true, approved: true, enabled: true, sharesFiles: true)];

  test('a node folder is read apart, and a folder of the hub is not one', () {
    expect(parseNodeFolder('node:node-a:projects/web'), (node: 'node-a', path: 'projects/web'));
    expect(parseNodeFolder('node:node-a:'), (node: 'node-a', path: ''), reason: 'the folder the node lends, itself');
    expect(parseNodeFolder('/srv/work'), isNull);
    expect(parseNodeFolder('node:'), isNull);
    expect(parseNodeFolder('node::x'), isNull);
    expect(nodeFolderRef('node-a', 'docs'), 'node:node-a:docs');
    expect(parseNodeFolder(nodeFolderRef('node-a', 'a/b')), (node: 'node-a', path: 'a/b'));
  });

  test('the label names the folder, and the machine when it is a node', () {
    expect(folderLabel('/srv/work/alpha'), 'alpha');
    expect(folderLabel('/srv/work/alpha/', nodes), 'alpha');
    expect(folderLabel('node:node-a:projects/web', nodes), 'web · Home PC');
    expect(folderLabel('node:node-a:', nodes), 'shared folder · Home PC');
    expect(folderLabel('node:node-z:docs', nodes), 'docs · node-z', reason: 'a node not known by name shows its id');
  });

  test('the place is the full path, with the node by name', () {
    expect(folderPlace('/srv/work'), '/srv/work');
    expect(folderPlace('node:node-a:projects/web', nodes), 'Home PC: /projects/web');
    expect(folderPlace('node:node-a:', nodes), 'Home PC: /');
  });
}
