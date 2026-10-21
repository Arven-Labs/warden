// ignore: unused_import
import 'package:intl/intl.dart' as intl;

import 'app_localizations.dart';

// ignore_for_file: type=lint

/// The translations for English (`en`).
class AppLocalizationsEn extends AppLocalizations {
  AppLocalizationsEn([String locale = 'en']) : super(locale);

  @override
  String get languageLabel => 'Language';

  @override
  String get languageEnglish => 'English';

  @override
  String get languagePortuguese => 'Português (Brasil)';

  @override
  String get languageSystem => 'Follow the system';

  @override
  String get commonCancel => 'Cancel';

  @override
  String get commonSave => 'Save';

  @override
  String get commonEdit => 'Edit';

  @override
  String get commonRemove => 'Remove';

  @override
  String get commonDelete => 'Delete';

  @override
  String get commonKeep => 'Keep';

  @override
  String get commonClose => 'Close';

  @override
  String get commonRefresh => 'Refresh';

  @override
  String get commonLoading => 'Loading…';

  @override
  String get commonCopy => 'Copy';

  @override
  String get commonDone => 'Done';

  @override
  String get commonTryAgain => 'Try again';

  @override
  String get folderTitle => 'Working folder';

  @override
  String get folderHint =>
      'The AI reads and writes in this folder, and the shell starts there (each command asks for your yes). It applies to the whole conversation and does not change afterwards.';

  @override
  String get folderThisHub => 'This hub';

  @override
  String get folderYours => 'Your folders';

  @override
  String get folderUp => 'Up';

  @override
  String get folderEmpty => 'No subfolders.';

  @override
  String get folderUse => 'Use this folder';

  @override
  String get connTitle => 'Warden — Server Connection';

  @override
  String get connDiscoverTooltip => 'Discover hubs on the local network';

  @override
  String get connScanTooltip => 'Scan QR to fill in connection';

  @override
  String get connAccountTooltip => 'My account';

  @override
  String get connSyncTooltip => 'Sync';

  @override
  String get connSkillsTooltip => 'Skills';

  @override
  String get connHost => 'Server host';

  @override
  String get connPort => 'Port';

  @override
  String get connUseTls => 'Use TLS (wss://)';

  @override
  String get connUseTlsHint =>
      'For a hub serving HTTPS — use the name its certificate covers, e.g. hub.tailXXXX.ts.net';

  @override
  String get connPairingKey => 'Pairing key';

  @override
  String get connUsername => 'Username';

  @override
  String get connPassword => 'Password';

  @override
  String get connPasswordHint => 'Only needed the first time on this phone';

  @override
  String get connAuthKey => 'Auth key';

  @override
  String get connAuthKeyHint => 'Pairing key, or an invite code';

  @override
  String get connDeviceName => 'Device name';

  @override
  String get connResume => 'Resume chat';

  @override
  String get connDisconnect => 'Disconnect';

  @override
  String get connConnect => 'Connect';

  @override
  String get connFillAccount =>
      'Fill in host, port, username, password and device name';

  @override
  String get connFillKey => 'Fill in host, port, auth key and device name';

  @override
  String get connLocked =>
      'Your data is locked because the hub restarted. Sign in with your username and password once to unlock it.';

  @override
  String get connChoosePassword => 'Choose your own password to continue';

  @override
  String connDiscoveryFailed(String error) {
    return 'Discovery failed: $error';
  }

  @override
  String get connNoHub => 'No hub answered on the local network.';

  @override
  String get connHubsFound => 'Hubs found';

  @override
  String get connStatusDisconnected => 'Disconnected';

  @override
  String connStatusDisconnectedWhy(String reason) {
    return 'Disconnected: $reason';
  }

  @override
  String get connStatusConnecting => 'Connecting…';

  @override
  String connStatusConnected(String server) {
    return 'Connected to $server';
  }

  @override
  String connStatusError(String message) {
    return 'Error: $message';
  }

  @override
  String get qrTitle => 'Scan the hub\'s QR';

  @override
  String get qrNotRecognized =>
      'QR code not recognized — scan the QR of the desktop\'s Workspace.';

  @override
  String get categoryDeleteData => 'Delete data';

  @override
  String get categorySpendMoney => 'Spend money';

  @override
  String get categoryCriticalInfra =>
      'Touch critical infrastructure (shell, SSH, nodes)';

  @override
  String get categoryExternalMessage =>
      'Send an external message (browser, outside services)';

  @override
  String get categoryPublishCode => 'Publish code';

  @override
  String get categoryImportantConfig =>
      'Change important configuration (scheduled tasks)';

  @override
  String get categoryElevatedAgent => 'Create or change agents';

  @override
  String get autonomy1 => '1 — only answers, no tools';

  @override
  String get autonomy2 =>
      '2 — suggests: reads, refuses the rest with a suggestion';

  @override
  String get autonomy3 => '3 — asks for your yes before any change';

  @override
  String get autonomy4 => '4 — acts on its own (the checked kinds still ask)';

  @override
  String get autonomy5 => '5 — also manages its reports on its own';

  @override
  String sideTo(String name) {
    return 'to $name';
  }

  @override
  String sideFrom(String name) {
    return 'from $name';
  }

  @override
  String get sideScheduledTask => 'scheduled task';

  @override
  String get sideWebhook => 'webhook';

  @override
  String get sideButton => 'Notes and runs';

  @override
  String sideButtonCount(int count) {
    return 'Notes and runs ($count)';
  }

  @override
  String get commonWait => 'Wait…';

  @override
  String get commonConfirm => 'Confirm';

  @override
  String get agentsTitle => 'Agents';

  @override
  String get agentsTabOrganization => 'Organization';

  @override
  String get agentsTabTasks => 'Tasks';

  @override
  String get agentsTabActivity => 'Activity';

  @override
  String get pairingWrong => 'Wrong pairing key.';

  @override
  String get pairingLabel => 'Hub\'s pairing key';

  @override
  String get pairingHelper => 'Asked on every change; never kept.';

  @override
  String positionTitle(String agent) {
    return 'Position of $agent';
  }

  @override
  String get positionRole => 'Role';

  @override
  String get positionRoleHint => 'e.g. backend lead';

  @override
  String get positionReportsTo => 'Reports to';

  @override
  String positionReportsHelper(String agent) {
    return 'Whoever reports to $agent goes with it.';
  }

  @override
  String get positionNobody => 'Nobody (top of the tree)';

  @override
  String powersTitle(String agent) {
    return 'Powers of $agent';
  }

  @override
  String get powersAutonomy => 'Autonomy';

  @override
  String powersLevel5(String agent) {
    return 'Level 5 only counts for an agent that may manage agents, and $agent may not: that is turned on in the settings.';
  }

  @override
  String get powersStartTasks => 'Can start background work';

  @override
  String get powersCreateWorkers => 'Can create temporary agents';

  @override
  String get powersMessageUser => 'Can start a conversation with you';

  @override
  String get powersChooseModels =>
      'Can pick the model of the tasks it delegates';

  @override
  String get powersAsks =>
      'Asks for your yes before (even when acting on its own):';

  @override
  String limitTitle(String agent) {
    return 'Models of $agent';
  }

  @override
  String get limitHint =>
      'The models it can pick for the tasks it delegates. None checked leaves the choice open; with one, it dictates the model.';

  @override
  String get limitNone => 'The hub has no providers or policies to pick from.';

  @override
  String limitPolicy(String id) {
    return '$id (policy)';
  }

  @override
  String get limitDefault => 'Default';

  @override
  String limitDefaultHelper(String agent) {
    return 'What a task gets when $agent does not pick.';
  }

  @override
  String get policyNew => 'New policy';

  @override
  String policyTitle(String id) {
    return 'Policy $id';
  }

  @override
  String get policyName => 'Name';

  @override
  String get policyAnsweredBy => 'Answered by';

  @override
  String get policyWhen => 'When to pick it (one line)';

  @override
  String get policyWhenHint => 'e.g. simple, cheap work';

  @override
  String get policySave => 'Save policy';

  @override
  String get newAgentTop => 'New agent at the top';

  @override
  String newAgentUnder(String parent) {
    return 'New agent under $parent';
  }

  @override
  String get newAgentName => 'Name';

  @override
  String get newAgentNameHint => 'e.g. reviewer';

  @override
  String get newAgentRole => 'Role (optional)';

  @override
  String get newAgentDoes => 'What it does';

  @override
  String get newAgentHelper =>
      'Starts careful: read-only tools, asks before any change, and cannot delegate or manage agents until a person turns that on.';

  @override
  String get newAgentAdd => 'Add agent';

  @override
  String get orgRestarts => 'The hub restarts with the change.';

  @override
  String orgChange(String agent) {
    return 'Change $agent';
  }

  @override
  String orgAdd(String agent) {
    return 'Add $agent';
  }

  @override
  String orgSavePolicy(String id) {
    return 'Save the policy $id';
  }

  @override
  String orgRemovePolicy(String id) {
    return 'Remove the policy $id';
  }

  @override
  String get delegationOpen =>
      'Open: the agent picks any model for each task it delegates.';

  @override
  String delegationDictated(String model) {
    return 'Dictated: every task it delegates runs on $model.';
  }

  @override
  String delegationLimited(int count, String model) {
    return 'Limited to $count models; $model is what a task gets when the agent does not pick.';
  }

  @override
  String get taskPending => 'Pending';

  @override
  String get taskRunning => 'Running';

  @override
  String get taskWaiting => 'Waiting for an agent';

  @override
  String get taskPaused => 'Paused';

  @override
  String get taskDone => 'Done';

  @override
  String get taskFailed => 'Failed';

  @override
  String get taskCancelled => 'Stopped';

  @override
  String get taskActionPause => 'Pause';

  @override
  String get taskActionResume => 'Resume';

  @override
  String get taskActionCancel => 'Stop';

  @override
  String get grantDeleteData => 'Delete data';

  @override
  String get grantSpendMoney => 'Spend money';

  @override
  String get grantCriticalInfra => 'Critical infrastructure';

  @override
  String get grantExternalMessage => 'External message';

  @override
  String get grantPublishCode => 'Publish code';

  @override
  String get grantImportantConfig => 'Important configuration';

  @override
  String get grantElevatedAgent => 'Agent with elevated permission';

  @override
  String spendWindow(int hours) {
    return 'in the last $hours h';
  }

  @override
  String spendUnpricedOne(int count) {
    return '$count unpriced call';
  }

  @override
  String spendUnpricedMany(int count) {
    return '$count unpriced calls';
  }

  @override
  String spendUnder(String amount) {
    return 'under $amount';
  }

  @override
  String spendWithUnpriced(String money, String window, String unpriced) {
    return '$money $window (and $unpriced)';
  }

  @override
  String get agoNow => 'just now';

  @override
  String agoMinutes(int count) {
    return '$count min ago';
  }

  @override
  String agoHours(int count) {
    return '$count h ago';
  }

  @override
  String agoDays(int count) {
    return '$count d ago';
  }

  @override
  String tallyDone(int count) {
    return '$count done';
  }

  @override
  String tallyFailed(int count) {
    return '$count failed';
  }

  @override
  String tallyStopped(int count) {
    return '$count stopped';
  }

  @override
  String tallyRunning(int count) {
    return '$count running';
  }

  @override
  String tallyWaiting(int count) {
    return '$count waiting for an agent';
  }

  @override
  String tallyPaused(int count) {
    return '$count paused';
  }

  @override
  String tallyPending(int count) {
    return '$count pending';
  }

  @override
  String tallyTokens(String tokens) {
    return '$tokens tokens';
  }

  @override
  String tallyDelegated(int count) {
    return 'delegated $count';
  }

  @override
  String tallyActiveAgo(String ago) {
    return 'active $ago';
  }

  @override
  String get activityNoAgent => 'The assistant';

  @override
  String activityCreatedAgent(String actor, String target) {
    return '$actor created the agent $target';
  }

  @override
  String activityRemovedAgent(String actor, String target) {
    return '$actor removed the agent $target';
  }

  @override
  String activityUpdatedAgent(String actor, String target) {
    return '$actor changed the agent $target';
  }

  @override
  String activityScheduledRan(String actor, String target) {
    return '$actor ran the scheduled task $target';
  }

  @override
  String activityScheduledFailed(String target) {
    return 'The scheduled task $target failed';
  }

  @override
  String activityWebhookRan(String actor, String target) {
    return '$actor answered the webhook $target';
  }

  @override
  String activityWebhookFailed(String target) {
    return 'The webhook $target failed';
  }

  @override
  String activityDelegated(String actor, String target) {
    return '$actor delegated a task to $target';
  }

  @override
  String activityStarted(String actor) {
    return '$actor started the task';
  }

  @override
  String activityDone(String actor) {
    return '$actor finished the task';
  }

  @override
  String activityFailed(String actor) {
    return '$actor could not finish the task';
  }

  @override
  String activityCancelled(String actor) {
    return '$actor\'s task was cancelled';
  }

  @override
  String activityNote(String actor, String target) {
    return '$actor left a note for $target';
  }

  @override
  String activityReply(String actor, String target) {
    return '$actor answered $target';
  }

  @override
  String activityMessagedUser(String actor) {
    return '$actor wrote to you';
  }

  @override
  String get activityToday => 'Today';

  @override
  String get activityYesterday => 'Yesterday';

  @override
  String activityDate(int day, String month, int year) {
    String _temp0 = intl.Intl.selectLogic(month, {
      '1': 'January',
      '2': 'February',
      '3': 'March',
      '4': 'April',
      '5': 'May',
      '6': 'June',
      '7': 'July',
      '8': 'August',
      '9': 'September',
      '10': 'October',
      '11': 'November',
      'other': 'December',
    });
    return '$day $_temp0 $year';
  }

  @override
  String orgRemoveTitle(String agent) {
    return 'Remove $agent?';
  }

  @override
  String orgRemoveBody(int count, String goesTo) {
    String _temp0 = intl.Intl.pluralLogic(
      count,
      locale: localeName,
      other: 'Its $count reports go to report to $goesTo. ',
      one: 'Its report goes to report to $goesTo. ',
      zero: '',
    );
    return '${_temp0}The conversations stay; this can\'t be undone.';
  }

  @override
  String orgRemove(String agent) {
    return 'Remove $agent';
  }

  @override
  String get orgNobodyTop => 'nobody (the top of the tree)';

  @override
  String get badgeDelegates => 'delegates';

  @override
  String get badgeManages => 'manages agents';

  @override
  String get badgeNotes => 'leaves notes';

  @override
  String get badgeTasks => 'schedules tasks';

  @override
  String get badgeNoBackground => 'no background work';

  @override
  String get badgeNoTemp => 'no temporary agents';

  @override
  String get badgeNoChat => 'cannot start a chat';

  @override
  String get badgeNoModels => 'cannot pick models';

  @override
  String badgeAutonomy(int level, String label) {
    return 'autonomy $level: $label';
  }

  @override
  String get autonomyShort1 => 'only answers';

  @override
  String get autonomyShort2 => 'suggests';

  @override
  String get autonomyShort3 => 'asks first';

  @override
  String get autonomyShort4 => 'acts alone';

  @override
  String get autonomyShort5 => 'manages alone';

  @override
  String badgeAsksFirst(int count) {
    String _temp0 = intl.Intl.pluralLogic(
      count,
      locale: localeName,
      other: 'asks first: $count kinds of action',
      one: 'asks first: 1 kind of action',
    );
    return '$_temp0';
  }

  @override
  String orgReports(int count) {
    String _temp0 = intl.Intl.pluralLogic(
      count,
      locale: localeName,
      other: '$count reports',
      one: '1 report',
    );
    return '$_temp0';
  }

  @override
  String get menuChat => 'Chat';

  @override
  String get menuTasks => 'Tasks';

  @override
  String get menuEditPosition => 'Edit position';

  @override
  String get menuPowers => 'Powers';

  @override
  String get menuModels => 'Models it can pick';

  @override
  String get menuAddReport => 'Add a report';

  @override
  String get orgHint =>
      'Who reports to whom. An agent that manages or delegates to others reaches only the ones below it. Anything an agent changes still waits for your yes.';

  @override
  String get orgNone => 'No agents yet.';

  @override
  String get orgNobodyReports =>
      'Nobody reports to anybody yet: use \"Edit position\" on an agent to pick its superior.';

  @override
  String get orgAddTop => 'Add an agent at the top';

  @override
  String get policiesTitle => 'Model policies';

  @override
  String get policiesNone =>
      'None: an agent that delegates sees only the model ids.';

  @override
  String get grantsTitle => 'Permissions given with \"always\"';

  @override
  String grantsHintKept(int ttl) {
    String _temp0 = intl.Intl.pluralLogic(
      ttl,
      locale: localeName,
      other:
          'Kept in a file, they survive a restart and lapse after $ttl days unused (each use renews them).',
      one: 'Kept in a file, they survive a restart and lapse after 1 day unused (each use renews them).',
    );
    return 'They hold per conversation and agent. $_temp0 Taking one back makes the agent ask again.';
  }

  @override
  String get grantsHintSession =>
      'They hold per conversation and agent. They end when the connection closes. Taking one back makes the agent ask again.';

  @override
  String get grantsNone => 'None yet.';

  @override
  String get grantDefaultAgent => 'default agent';

  @override
  String grantConversation(String id) {
    return 'conversation $id';
  }

  @override
  String grantUsed(String ago) {
    return 'used $ago';
  }

  @override
  String get grantTakeBack => 'Take back';

  @override
  String taskControlTitle(String action, String agent) {
    return '$action the task of $agent';
  }

  @override
  String get taskControlStops => 'Its subtasks stop too.';

  @override
  String get taskControlOnly =>
      'Only a task running on this hub can be controlled.';

  @override
  String get taskAnAgent => 'An agent';

  @override
  String get taskWorking => 'working';

  @override
  String taskProgress(int finished, int total, int percent, String parts) {
    return '$finished of $total finished ($percent%) · $parts';
  }

  @override
  String taskTok(String tokens) {
    return '$tokens tok';
  }

  @override
  String get taskDetailTask => 'Task';

  @override
  String get taskDetailResult => 'Result';

  @override
  String get taskDetailWhy => 'Why it stopped';

  @override
  String taskNoResult(String state) {
    return '$state: no result yet.';
  }

  @override
  String get tasksHint =>
      'Tasks the agents handed each other in the background on this hub, where each one is and what it cost.';

  @override
  String tasksOnly(String agent) {
    return 'Only the tasks of $agent: the ones it received and the ones it delegated.';
  }

  @override
  String get tasksShowAll => 'Show all';

  @override
  String tasksNoneFor(String agent) {
    return 'No task involves $agent yet.';
  }

  @override
  String get tasksNone =>
      'Nothing yet. When an agent delegates a task with \"background\", it shows up here.';

  @override
  String get activityHint =>
      'What happened among the agents on this hub: who delegated, started and finished what, the notes they left each other and the messages an agent wrote first to you.';

  @override
  String get activityAll => 'All agents';

  @override
  String activityNothingFor(String agent) {
    return 'Nothing involves $agent yet.';
  }

  @override
  String get activityNothing =>
      'Nothing yet. When agents delegate tasks, leave each other notes or write to you, it shows up here.';

  @override
  String get chatApprovalTitle => 'Approval needed';

  @override
  String chatApprovalCategory(String category) {
    return 'Category: $category (this agent asks for approval on this)';
  }

  @override
  String get chatDeny => 'Deny';

  @override
  String get chatApprove => 'Approve';

  @override
  String chatSyncSnack(String message) {
    return 'Sync: $message';
  }

  @override
  String get chatEditTitle => 'Edit message';

  @override
  String get chatSend => 'Send';

  @override
  String chatEditWarning(int count) {
    String _temp0 = intl.Intl.pluralLogic(
      count,
      locale: localeName,
      other:
          'Sending this deletes the $count messages after it, and the threads on them.',
      one: 'Sending this deletes the message after it, and the threads on it.',
    );
    return '$_temp0';
  }

  @override
  String chatChannelFailed(String error) {
    return 'Couldn\'t open the channel: $error';
  }

  @override
  String get chatFilesTitle => 'Files';

  @override
  String get chatFilesNone => 'No folder configured yet.';

  @override
  String chatFilesConfigured(String uri) {
    return 'Configured folder:\n$uri';
  }

  @override
  String get chatFilesHint =>
      'Lets the model list and read text files under this folder. Takes effect the next time you connect.';

  @override
  String get chatFilesClear => 'Clear';

  @override
  String get chatFilesChoose => 'Choose folder';

  @override
  String get chatNewConversation => 'New conversation';

  @override
  String chatAs(String agent) {
    return 'as $agent';
  }

  @override
  String get chatChannelsTooltip => 'Agent channels';

  @override
  String chatChannelsTooltipNew(int count) {
    return 'Agent channels ($count new)';
  }

  @override
  String get chatAgentsTooltip => 'Agents';

  @override
  String get chatUsageTooltip => 'Usage';

  @override
  String get chatDevicesTooltip => 'Devices';

  @override
  String get chatConversationsTooltip => 'Conversations';

  @override
  String get chatFilesTooltip => 'Files';

  @override
  String get chatDisconnect => 'Disconnect';

  @override
  String get chatThreadBack => 'Back to the conversation';

  @override
  String get chatChannelBack => 'Back to the conversations';

  @override
  String get chatWorkBack => 'Back to notes and runs';

  @override
  String get chatRegenerate => 'Regenerate';

  @override
  String get folderBarNone => 'Working folder: none';

  @override
  String get folderBarTooltip =>
      'Pick a folder of the hub or of a node for the AI to work in';

  @override
  String get folderBarClear => 'Remove the folder';

  @override
  String get channelsNone => 'No agents configured yet.';

  @override
  String get channelsAnswering => 'answering…';

  @override
  String get channelsNew => 'new';

  @override
  String workNone(String agent) {
    return '$agent has not traded notes with other agents or run on its own yet.';
  }

  @override
  String get agentMenuTooltip => 'Agent';

  @override
  String get agentMenuNone => 'No agent';

  @override
  String agentMenuRemoved(String agent) {
    return '$agent (removed)';
  }

  @override
  String attachmentUnsupported(String mime) {
    return '📎 $mime attached (playback not supported here yet)';
  }

  @override
  String attachmentDecodeFailed(String mime) {
    return 'Could not decode $mime attachment';
  }

  @override
  String attachmentPlayFailed(String mime) {
    return 'Could not play $mime attachment';
  }

  @override
  String get inputHint => 'Message';

  @override
  String get bucketLast7 => 'Previous 7 days';

  @override
  String get bucketLast30 => 'Previous 30 days';

  @override
  String get bucketOlder => 'Older';

  @override
  String get drawerRenameTitle => 'Rename conversation';

  @override
  String get drawerDeleteTitle => 'Delete conversation?';

  @override
  String drawerDeleteBody(String title) {
    return '\"$title\" will be deleted from the hub. This can\'t be undone.';
  }

  @override
  String get drawerUntitled => 'Untitled';

  @override
  String get drawerAnswering => 'Answering…';

  @override
  String get drawerActions => 'Conversation actions';

  @override
  String get drawerUnpin => 'Unpin';

  @override
  String get drawerPin => 'Pin to the top';

  @override
  String get drawerUnarchive => 'Unarchive';

  @override
  String get drawerArchive => 'Archive';

  @override
  String get drawerRename => 'Rename';

  @override
  String get drawerNew => 'New conversation';

  @override
  String get drawerNone => 'No conversations yet.';

  @override
  String get drawerPinned => 'Pinned';

  @override
  String drawerArchived(int count) {
    return 'Archived ($count)';
  }

  @override
  String get bannerDisconnected => 'Disconnected';

  @override
  String bannerDisconnectedWhy(String reason) {
    return 'Disconnected: $reason';
  }

  @override
  String get bannerConnecting => 'Connecting…';

  @override
  String bannerError(String message) {
    return 'Connection error: $message';
  }

  @override
  String get liveStop => 'Stop';

  @override
  String get thinking => 'Thinking…';

  @override
  String get threadReply => 'Reply in thread';

  @override
  String threadReplies(int count) {
    String _temp0 = intl.Intl.pluralLogic(
      count,
      locale: localeName,
      other: '$count replies',
      one: '1 reply',
    );
    return '$_temp0';
  }
}
