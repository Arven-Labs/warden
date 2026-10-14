import 'dart:convert';

import 'package:flutter_test/flutter_test.dart';
import 'package:mobile/protocol/messages.dart';

// P120, P123 — the wire shape of the agent tasks and of the organization edits, locked the same way
// `crates/warden-server-protocol/src/protocol.rs` locks it on the Rust side.
void main() {
  group('client messages', () {
    test('listing and controlling the delegated tasks', () {
      expect(const ListAgentTasksMessage(4).toJson(), {'type': 'listAgentTasks', 'requestId': 4});
      expect(
        const ControlAgentTaskMessage(5, 'k', 'at-1', 'pause').toJson(),
        {'type': 'controlAgentTask', 'requestId': 5, 'pairingKey': 'k', 'taskId': 'at-1', 'action': 'pause'},
      );
    });

    test('asking for the feed of activity', () {
      expect(const ListActivityMessage(9).toJson(), {'type': 'listActivity', 'requestId': 9});
    });

    test('an organization edit leaves out a blank role and a missing superior', () {
      expect(
        jsonDecode(const EditAgentOrgMessage(6, 'k', SetPositionEdit('dev', role: 'Backend', reportsTo: 'lead')).encode()),
        {
          'type': 'editAgentOrg',
          'requestId': 6,
          'pairingKey': 'k',
          'edit': {'kind': 'setPosition', 'id': 'dev', 'role': 'Backend', 'reportsTo': 'lead'},
        },
      );
      expect(const SetPositionEdit('dev', role: '  ', reportsTo: '').toJson(), {'kind': 'setPosition', 'id': 'dev'});
      expect(
        const AddReportEdit(' qa ', ' tests ', role: 'QA').toJson(),
        {'kind': 'addReport', 'id': 'qa', 'persona': 'tests', 'role': 'QA'},
      );
      expect(const RemoveAgentEdit('dev').toJson(), {'kind': 'remove', 'id': 'dev'});
    });

    test('the model limit and the policies go as the hub reads them, with a description always present', () {
      expect(const SetDelegationModelsEdit('chief', ['fast', 'main']).toJson(), {'kind': 'setDelegationModels', 'id': 'chief', 'models': ['fast', 'main']});
      expect(const SetDelegationModelsEdit('chief', []).toJson(), {'kind': 'setDelegationModels', 'id': 'chief', 'models': []});
      expect(
        const SetModelPoliciesEdit([ModelPolicy(id: 'fast', model: 'main', description: 'quick'), ModelPolicy(id: 'deep', model: 'spare')]).toJson(),
        {
          'kind': 'setModelPolicies',
          'policies': [
            {'id': 'fast', 'model': 'main', 'description': 'quick'},
            {'id': 'deep', 'model': 'spare', 'description': ''},
          ],
        },
      );
    });
  });

  group('server messages', () {
    test('settings carry the organization and the model policies', () {
      final msg = ServerMessage.decode(jsonEncode({
        'type': 'settings',
        'requestId': 3,
        'version': 'v',
        'secretsWritable': false,
        'settings': {
          'agents': [
            {
              'id': 'chief',
              'role': 'CTO',
              'canDelegateToAgents': true,
              'canManageAgents': true,
              'autonomy': 3,
              'approvalRequired': ['spend_money'],
              'delegationModels': ['fast'],
            },
            {'id': 'dev', 'reportsTo': 'chief'},
          ],
          'modelPolicies': [
            {'id': 'fast', 'model': 'gpt-mini', 'description': 'quick'},
          ],
        },
      })) as SettingsMessage;
      expect(msg.agentIds, ['chief', 'dev']);
      expect(msg.agents[0].role, 'CTO');
      expect(msg.agents[0].canDelegateToAgents, isTrue);
      expect(msg.agents[0].autonomy, 3);
      expect(msg.agents[0].approvalRequired, ['spend_money']);
      expect(msg.agents[0].delegationModels, ['fast']);
      expect(msg.agents[1].reportsTo, 'chief');
      expect(msg.modelPolicies.single.model, 'gpt-mini');
    });

    test('a hub that leaves fields out gets the defaults', () {
      final msg = ServerMessage.decode('{"type":"settings","requestId":1,"settings":{"agents":[{"id":"solo"}]}}') as SettingsMessage;
      expect(msg.modelPolicies, isEmpty);
      expect(msg.agents.single.autonomy, 4);
      expect(msg.agents.single.delegationModels, isEmpty);
      expect(msg.agents.single.reportsTo, isNull);
    });

    test('settings carry the ids of the providers and combos, apart from the policies', () {
      final msg = ServerMessage.decode(jsonEncode({
        'type': 'settings',
        'requestId': 1,
        'settings': {
          'agents': <dynamic>[],
          'providers': [
            {'id': 'main'},
            {'id': 'spare'},
          ],
          'combos': [
            {'id': 'both'},
          ],
          'modelPolicies': [
            {'id': 'fast', 'model': 'main', 'description': ''},
          ],
        },
      })) as SettingsMessage;
      expect(msg.modelIds, ['main', 'spare', 'both']);
      expect(msg.modelPolicies.single.id, 'fast');
      final saved = ServerMessage.decode('{"type":"settingsSaved","requestId":2,"settings":{"agents":[]}}') as SettingsSavedMessage;
      expect(saved.modelIds, isEmpty);
    });

    test('an organization edit is answered with the agents as they are now', () {
      final msg = ServerMessage.decode(
          '{"type":"settingsSaved","requestId":9,"version":"v2","settings":{"agents":[{"id":"a","reportsTo":"b"},{"id":"b"}]}}');
      expect(msg, isA<SettingsSavedMessage>());
      expect((msg as SettingsSavedMessage).agents.map((a) => a.id), ['a', 'b']);
    });

    test('a refused key comes through as authRejected', () {
      final settings = ServerMessage.decode('{"type":"settingsError","requestId":2,"message":"wrong key","conflict":false,"authRejected":true}');
      expect((settings as SettingsErrorMessage).authRejected, isTrue);
      final plain = ServerMessage.decode('{"type":"settingsError","requestId":2,"message":"nope"}');
      expect((plain as SettingsErrorMessage).authRejected, isFalse);
      final task = ServerMessage.decode('{"type":"taskError","requestId":5,"message":"not running here","authRejected":false}');
      expect((task as TaskErrorMessage).authRejected, isFalse);
    });

    test('a device invite and a key rotation go out and come back in the web shapes (P36)', () {
      expect(const CreateDeviceInviteMessage(5, 'k', "Ana's phone", 15).toJson(),
          {'type': 'createDeviceInvite', 'requestId': 5, 'pairingKey': 'k', 'name': "Ana's phone", 'ttlMinutes': 15});
      expect(const RotatePairingKeyMessage(6, 'k').toJson(), {'type': 'rotatePairingKey', 'requestId': 6, 'pairingKey': 'k'});

      final invite = ServerMessage.decode('{"type":"deviceInvite","requestId":5,"name":"Ana\'s phone","code":"abc","expiresMs":9}') as DeviceInviteMessage;
      expect((invite.requestId, invite.name, invite.code, invite.expiresMs), (5, "Ana's phone", 'abc', 9));
      final rotated = ServerMessage.decode('{"type":"pairingKeyRotated","requestId":6,"newKey":"new"}') as PairingKeyRotatedMessage;
      expect((rotated.requestId, rotated.newKey), (6, 'new'));
      final refused = ServerMessage.decode('{"type":"deviceError","requestId":7,"message":"wrong key","authRejected":true}') as DeviceErrorMessage;
      expect(refused.authRejected, isTrue);
      final plain = ServerMessage.decode('{"type":"deviceError","requestId":7,"message":"nope"}') as DeviceErrorMessage;
      expect(plain.authRejected, isFalse);
    });

    test('a usage report keeps what each agent spent, and a hub that sends no recent spending decodes as none (P123)', () {
      final msg = ServerMessage.decode(jsonEncode({
        'type': 'usageReport',
        'requestId': 5,
        'report': {
          'recent': {
            'windowHours': 24,
            'byAgent': [
              {'key': 'dev', 'calls': 3, 'tokens': 90, 'costUsd': 0.25, 'unpricedCalls': 2},
            ],
          },
        },
      })) as UsageReportMessage;
      expect(msg.requestId, 5);
      expect(msg.recent!.windowHours, 24);
      expect([msg.recent!.byAgent.single.key, msg.recent!.byAgent.single.costUsd, msg.recent!.byAgent.single.unpricedCalls], ['dev', 0.25, 2]);
      expect((ServerMessage.decode('{"type":"usageReport","requestId":6,"report":{}}') as UsageReportMessage).recent, isNull);
      expect(const RequestUsageMessage(7, -180).toJson(), {'type': 'requestUsage', 'requestId': 7, 'tzOffsetMinutes': -180});
    });

    test('a usage report keeps the totals, the days with their dollars and the limits for the usage screen (P99)', () {
      final msg = ServerMessage.decode(jsonEncode({
        'type': 'usageReport',
        'requestId': 8,
        'report': {
          'total': {'totalTokens': 1500},
          'messageCount': 4,
          'daily': [
            {'date': '2026-10-11', 'calls': 1, 'tokens': 500},
            {'date': '2026-10-12', 'calls': 3, 'tokens': 1000},
          ],
          'dailyCost': [
            {'date': '2026-10-12', 'calls': 3, 'costUsd': 0.5, 'unpricedCalls': 0},
          ],
          'limitsEnabled': true,
          'limits': [
            {
              'id': 'ana-day',
              'windowHours': 24,
              'usedTokens': 200,
              'maxTokens': 150,
              'usedCostUsd': 0.0,
              'maxCostUsd': null,
              'fraction': 1.33,
              'warn': true,
              'exceeded': true,
            },
          ],
          'recent': {'windowHours': 24, 'byAgent': []},
        },
      })) as UsageReportMessage;
      final summary = msg.summary!;
      expect([summary.totalTokens, summary.calls, summary.windowHours, summary.limitsEnabled], [1500, 4, 24, true]);
      expect(summary.days.map((d) => [d.date, d.calls, d.tokens, d.costUsd]), [
        ['2026-10-11', 1, 500, null],
        ['2026-10-12', 3, 1000, 0.5],
      ]);
      expect(summary.costInWindowUsd, 0.5);
      final limit = summary.limits.single;
      expect([limit.id, limit.maxTokens, limit.maxCostUsd, limit.exceeded], ['ana-day', 150, null, true]);
      // A report with nothing in it still decodes, and a refusal has its own message.
      final empty = (ServerMessage.decode('{"type":"usageReport","requestId":9,"report":{}}') as UsageReportMessage).summary!;
      expect([empty.totalTokens, empty.days, empty.limits, empty.limitsEnabled], [0, isEmpty, isEmpty, false]);
      final error = ServerMessage.decode('{"type":"usageError","requestId":10,"message":"your data is locked"}') as UsageErrorMessage;
      expect([error.requestId, error.message], [10, 'your data is locked']);
    });

    test('a task list keeps what the hub sent', () {
      final msg = ServerMessage.decode(jsonEncode({
        'type': 'agentTaskList',
        'requestId': 4,
        'tasks': [
          {
            'id': 'at-1',
            'group': 'g',
            'owner': 'chief',
            'assignee': 'dev',
            'parentId': 'at-0',
            'objective': 'x',
            'model': 'fast',
            'channel': 'cli',
            'state': 'running',
            'totalTokens': 120,
            'createdAtMs': 5,
            'startedAtMs': 6,
            'controllable': true,
            'pausable': true,
          },
        ],
      })) as AgentTaskListMessage;
      final task = msg.tasks.single;
      expect(task.assignee, 'dev');
      expect(task.owner, 'chief');
      expect(task.parentId, 'at-0');
      expect(task.state, 'running');
      expect(task.totalTokens, 120);
      expect(task.controllable, isTrue);
      expect(task.pausable, isTrue);
      expect(task.finishedAtMs, isNull);
    });

    test('the "always" permissions are asked for, revoked and listed', () {
      expect(const ListGrantsMessage(1).toJson(), {'type': 'listGrants', 'requestId': 1});
      expect(
        const RevokeGrantMessage(2, Grant(conversationId: 'c', covers: 'shell')).toJson(),
        {'type': 'revokeGrant', 'requestId': 2, 'conversationId': 'c', 'covers': 'shell'},
        reason: 'no agent id is left out, not sent as null',
      );
      expect(
        const RevokeGrantMessage(3, Grant(conversationId: 'c', agentId: 'chief', covers: 'delete_data')).toJson(),
        {'type': 'revokeGrant', 'requestId': 3, 'conversationId': 'c', 'agentId': 'chief', 'covers': 'delete_data'},
      );
      final msg = ServerMessage.decode('{"type":"grantList","requestId":3,"grants":[{"conversationId":"c","agentId":"chief","covers":"delete_data"},{"conversationId":"c","covers":"shell"}]}')
          as GrantListMessage;
      expect(msg.grants, [
        const Grant(conversationId: 'c', agentId: 'chief', covers: 'delete_data'),
        const Grant(conversationId: 'c', covers: 'shell'),
      ]);
      expect((ServerMessage.decode('{"type":"grantList","requestId":4}') as GrantListMessage).grants, isEmpty);
      expect(msg.ttlDays, isNull, reason: 'a hub that keeps none in a file, or predates the field, tells no term');
      expect((ServerMessage.decode('{"type":"grantList","requestId":4,"grants":[],"ttlDays":7}') as GrantListMessage).ttlDays, 7);

      final withUse = ServerMessage.decode('{"type":"grantList","requestId":5,"grants":[{"conversationId":"c","covers":"shell","lastUsedMs":1700},{"conversationId":"d","covers":"shell"}]}')
          as GrantListMessage;
      expect(withUse.grants.map((g) => g.lastUsedMs), [1700, null], reason: 'a hub that predates the field sends none');
      expect(withUse.grants.first, const Grant(conversationId: 'c', covers: 'shell'), reason: 'the last use is not part of what a grant is');
    });

    test('the feed of activity keeps its events and leaves the optional fields null', () {
      final msg = ServerMessage.decode(jsonEncode({
        'type': 'activityList',
        'requestId': 9,
        'events': [
          {'id': 'at-1-delegated', 'atMs': 5, 'kind': 'delegated', 'actor': 'chief', 'target': 'dev', 'text': 'build it', 'taskId': 'at-1'},
          {'id': 'c-m1', 'atMs': 9, 'kind': 'messaged_user', 'actor': 'pirate', 'text': 'disk full', 'conversationId': 'channel-1'},
        ],
      })) as ActivityListMessage;
      expect(msg.requestId, 9);
      expect(msg.events.map((e) => e.kind), ['delegated', 'messaged_user']);
      final first = msg.events.first;
      expect((first.actor, first.target, first.taskId, first.conversationId), ('chief', 'dev', 'at-1', null));
      final second = msg.events.last;
      expect((second.target, second.taskId, second.conversationId), (null, null, 'channel-1'));
    });

    test('a feed with no events key is an empty one', () {
      final msg = ServerMessage.decode(jsonEncode({'type': 'activityList', 'requestId': 1})) as ActivityListMessage;
      expect(msg.events, isEmpty);
    });
  });

  // P120 — a member's access to the organization: what they ask, what the hub answers and what they were told at sign-in.
  group('a member and the organization', () {
    test('listing the tree, and an edit by the member\'s own session that carries no pairing key', () {
      expect(const ListAgentOrgMessage(3).toJson(), {'type': 'listAgentOrg', 'requestId': 3});
      expect(
        jsonDecode(const EditAgentOrgMessage.asMember(4, RemoveAgentEdit('dev')).encode()),
        {
          'type': 'editAgentOrg',
          'requestId': 4,
          'edit': {'kind': 'remove', 'id': 'dev'},
        },
      );
      expect(const EditAgentOrgMessage(5, 'k', RemoveAgentEdit('dev')).toJson()['pairingKey'], 'k', reason: 'the owner still sends it');
    });

    test('the tree a member gets keeps the id, the role and the superior, and the access', () {
      final msg = ServerMessage.decode(jsonEncode({
        'type': 'agentOrgList',
        'requestId': 3,
        'agents': [
          {'id': 'chief', 'role': 'CTO'},
          {'id': 'dev', 'reportsTo': 'chief'},
        ],
        'access': 'edit',
      })) as AgentOrgListMessage;
      expect((msg.requestId, msg.access), (3, 'edit'));
      expect(msg.agents.map((a) => (a.id, a.role, a.reportsTo)), [('chief', 'CTO', null), ('dev', null, 'chief')]);
      expect(msg.agents.every((a) => !a.canDelegateToAgents && a.autonomy == 4), isTrue, reason: 'no powers are sent, so none are shown');
    });

    test('the hub pushes the new level with no request id', () {
      final msg = ServerMessage.decode(jsonEncode({'type': 'orgAccessChanged', 'access': 'view'})) as OrgAccessChangedMessage;
      expect(msg.access, 'view');
    });

    test('the access comes with the user at sign-in and survives what changes in the user afterwards', () {
      final user = UserInfo.fromJson({'id': 'ana', 'name': 'Ana', 'mustChangePassword': false, 'orgAccess': 'view'});
      expect(user.orgAccess, 'view');
      expect(user.withOwnPassword().orgAccess, 'view');
      expect(user.withTruthId('ana.truth').orgAccess, 'view');
      expect(UserInfo.fromJson({'id': 'bo', 'name': 'Bo'}).orgAccess, '', reason: 'absent is none');
    });
  });

  test('the permissions a person took away are read, and a hub that does not send them reads as on (P122)', () {
    final off = AgentInfo.fromJson({'id': 'a', 'canStartTasks': false, 'canMessageUser': false});
    expect([off.canStartTasks, off.canCreateWorkers, off.canMessageUser, off.canChooseModels], [false, true, false, true]);
    final old = AgentInfo.fromJson({'id': 'b'});
    expect([old.canStartTasks, old.canCreateWorkers, old.canMessageUser, old.canChooseModels], [true, true, true, true]);
  });
}
