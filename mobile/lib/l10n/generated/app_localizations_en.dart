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
  String get themeToLight => 'Switch to the light theme';

  @override
  String get themeToDark => 'Switch to the dark theme';

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
  String get chatTopUp => 'See balance';

  @override
  String get chatTopUpLink => 'Top up credit';

  @override
  String chatCredit(String amount) {
    return 'Credit: $amount';
  }

  @override
  String get chatCreditEmpty => 'Credit: none left';

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
  String get usageTitle => 'Usage';

  @override
  String usageLoadFailed(String error) {
    return 'Couldn\'t load the usage: $error';
  }

  @override
  String get usageCredit => 'Credits';

  @override
  String usageCreditAvailable(String amount) {
    return '$amount available after the calls in progress';
  }

  @override
  String get usageCreditEmpty =>
      'Out of credits. Top up your account to keep using paid models.';

  @override
  String get usageCreditTopUp => 'Top up credit';

  @override
  String get usageTokens => 'Tokens';

  @override
  String get usageCalls => 'Model calls';

  @override
  String usageSpent(String window) {
    return 'Spent, last $window';
  }

  @override
  String get usageWindowDay => '24 h';

  @override
  String usageWindowDays(int count) {
    return '$count days';
  }

  @override
  String usageWindowHours(int count) {
    return '$count h';
  }

  @override
  String get usageLimits => 'Limits';

  @override
  String get usageLimitsOff => 'Spending limits are switched off on this hub.';

  @override
  String get usageLimitsNone => 'No limit is set for you.';

  @override
  String get usageLimitsNote =>
      'Only the limits written for you show here. To allow more, ask whoever runs this Warden.';

  @override
  String get usageByDay => 'By day';

  @override
  String get usageNoConversation => 'No conversation yet.';

  @override
  String usageCallsCount(int count) {
    String _temp0 = intl.Intl.pluralLogic(
      count,
      locale: localeName,
      other: '$count calls',
      one: '1 call',
    );
    return '$_temp0';
  }

  @override
  String usageTokensCount(String tokens) {
    return '$tokens tokens';
  }

  @override
  String usageTokensCost(String tokens, String cost) {
    return '$tokens tokens · $cost';
  }

  @override
  String get usageByAgent => 'By agent';

  @override
  String get usageNoAgent => 'No agent';

  @override
  String get usageByProvider => 'By provider';

  @override
  String get usageDefaultProvider => 'Default provider';

  @override
  String get usageByCombo => 'By combo';

  @override
  String get usageNoCombo => 'No combo';

  @override
  String usageSpentByCombo(String window) {
    return 'Spent by combo, last $window';
  }

  @override
  String usageUnpriced(String cost, int count) {
    return '$cost (+$count unpriced)';
  }

  @override
  String usageBucketLine(String tokens, String calls) {
    return '$tokens tokens · $calls';
  }

  @override
  String usageLimitTokens(String used, String max) {
    return '$used of $max tokens';
  }

  @override
  String usageLimitCost(String used, String max) {
    return '$used of $max';
  }

  @override
  String get usageStateOver => 'Used up';

  @override
  String get usageStateWarn => 'Close to the limit';

  @override
  String get usageStateOk => 'Plenty left';

  @override
  String usageLimitTitle(String id, String window) {
    return '$id · last $window';
  }

  @override
  String usageLimitState(String state, int percent, String parts) {
    return '$state · $percent% · $parts';
  }

  @override
  String get invCancelTitle => 'Cancel invite';

  @override
  String invCancelMessage(String name) {
    return 'Cancels the invite for $name; its code stops working.';
  }

  @override
  String get invFill =>
      'Fill in who it is for and a validity of at least 1 minute.';

  @override
  String get invCreateTitle => 'Create invite';

  @override
  String invCreateMessage(int minutes) {
    return 'The code works once, for $minutes minutes.';
  }

  @override
  String get invRotateTitle => 'Change the pairing key';

  @override
  String get invRotateMessage =>
      'The old key stops opening the hub\'s administration. Devices already paired keep working.';

  @override
  String get invTitle => 'Devices';

  @override
  String get invHint =>
      'The pairing key pairs only the hub\'s first device. For each other one, make an invite and give the person the code: they paste it where the key would go. It works once.';

  @override
  String get invWho => 'Who is it for';

  @override
  String get invWhoHint => 'Ana\'s phone';

  @override
  String get invMinutes => 'Valid for (minutes)';

  @override
  String get invCreate => 'Create invite';

  @override
  String invCancelled(String name) {
    return 'Invite for $name cancelled.';
  }

  @override
  String get invWaiting => 'Invites waiting';

  @override
  String invValidUntil(String when) {
    return 'valid until $when';
  }

  @override
  String invMade(String name, String when) {
    return 'Invite for $name, valid until $when. The code shows only now:';
  }

  @override
  String get invNewKey => 'New pairing key, shown only now:';

  @override
  String get memberOrgNone =>
      'The workspace\'s owner hasn\'t given you access to the organization of the agents.';

  @override
  String get memberOrgEdit =>
      'Who reports to whom among the workspace\'s agents. The owner let you change the hierarchy: the role and the superior of an agent, a new report, a removal. What the agents may do is the owner\'s.';

  @override
  String get memberOrgView =>
      'Who reports to whom among the workspace\'s agents. You can look at the tree; changing it is the owner\'s.';

  @override
  String get skillsTitle => 'Skills';

  @override
  String skillsDeleteTitle(String name) {
    return 'Delete \'$name\'?';
  }

  @override
  String skillsRejectTitle(String name) {
    return 'Reject \'$name\'?';
  }

  @override
  String get skillsDeleteBody =>
      'This removes the skill file and can\'t be undone.';

  @override
  String get skillsReject => 'Reject';

  @override
  String get skillsAccept => 'Accept';

  @override
  String skillsSeeChanges(String name) {
    return 'See what changes in $name';
  }

  @override
  String get skillsNoChange => 'No change in the instruction text.';

  @override
  String get skillsSuggested => 'Suggested';

  @override
  String skillsChangeTo(String name) {
    return 'Change to $name';
  }

  @override
  String skillsFrom(String origin) {
    return 'From: $origin';
  }

  @override
  String get skillsNew => 'New skill';

  @override
  String get skillsEdit => 'Edit skill';

  @override
  String get skillsNone =>
      'No skills yet. Create one here, or ask the AI to write one in chat — they sync with the rest of the vault.';

  @override
  String get skillsSuggestedTitle => 'Suggested by the AI';

  @override
  String get skillsSuggestedNote =>
      'Not active yet — the AI only uses a suggestion after you accept it.';

  @override
  String get skillsYours => 'Your skills';

  @override
  String get skillsNoDescription => '(no description)';

  @override
  String skillsDeleteTooltip(String name) {
    return 'Delete $name';
  }

  @override
  String get skillsName => 'Name';

  @override
  String get skillsNameHelper =>
      'Lowercase letters, digits and hyphens — it becomes the file name.';

  @override
  String get skillsDescription => 'Description';

  @override
  String get skillsDescriptionHelper =>
      'When to use it — the AI reads this every turn.';

  @override
  String get skillsInstructions => 'Instructions';

  @override
  String get skillsAcceptThis => 'Accept this skill';

  @override
  String get skillsAcceptThisHint =>
      'Off: it stays a suggestion and the AI still ignores it.';

  @override
  String get skillsSaving => 'Saving…';

  @override
  String get skillsGallery => 'Gallery';

  @override
  String get skillsGalleryIntro =>
      'Ready-made skills that come with Warden. Installing copies the skill into your vault, where you can edit it like any other. Nothing is downloaded.';

  @override
  String get skillsGalleryInstalled => 'Installed';

  @override
  String get skillsInstall => 'Install';

  @override
  String get skillsInstalling => 'Installing…';

  @override
  String get skillsUpdate => 'Update';

  @override
  String get skillsUpdating => 'Updating…';

  @override
  String get skillsUpdateAvailable => 'A newer version is available; yours is untouched';

  @override
  String get skillsReplaceMine => 'Replace mine';

  @override
  String get skillsKeepMine => 'Keep mine';

  @override
  String get skillsOtherSameName => 'You have another skill with this name';

  @override
  String get skillsUseGallery => 'Use the gallery\'s';

  @override
  String get skillsViewInstructions => 'View the instructions';

  @override
  String skillsInstallFailed(String error) {
    return 'Failed to install the skill: $error';
  }

  @override
  String get diffGapOne => '… 1 identical line';

  @override
  String diffGapMany(int count) {
    return '… $count identical lines';
  }

  @override
  String get syncTitle => 'Warden — Sync';

  @override
  String get syncNothingToSend =>
      'Nothing to send — vault and config already match the last Send.';

  @override
  String get syncShowCode => 'Show pairing code';

  @override
  String get syncWaitingDevice => 'Waiting for another device…';

  @override
  String get syncPaired => 'Paired successfully!';

  @override
  String get syncIntro =>
      'Syncs the vault and config.toml across your devices via Arweave, paid by the TruthID app. Everything is encrypted on this device before it leaves — neither TruthID nor Arweave ever see the content in plain text.';

  @override
  String get syncSetUp => 'Set up';

  @override
  String get syncSetUpHint =>
      'First device? Initialize a fresh key. Already have another device with sync set up? Ask it to show a pairing code (below) instead.';

  @override
  String get syncInit => 'Initialize sync on this device';

  @override
  String get syncSendPull => 'Send / Pull';

  @override
  String get syncSend => 'Send';

  @override
  String get syncPull => 'Pull';

  @override
  String syncSent(String tx, int files) {
    return 'Sent — tx $tx ($files file(s)).';
  }

  @override
  String syncPulled(int written, int deleted, String config) {
    return 'Pull complete — $written written, $deleted deleted$config.';
  }

  @override
  String get syncConfigUpdated => ', config.toml updated';

  @override
  String get syncPairDevice => 'Pair a device';

  @override
  String get syncShowThisCode =>
      'Show this code on the device you want to pair.';

  @override
  String get syncHaveCode =>
      'Have a code shown on another device? Enter it here.';

  @override
  String get syncCodeLabel => 'Pairing code';

  @override
  String get syncHostLabel => 'Other device\'s IP (optional)';

  @override
  String get syncHostHint =>
      'e.g. a Tailscale IP for a hub on a VPS — without it, only the local network is searched';

  @override
  String get syncPair => 'Pair';

  @override
  String get syncNever => 'never';

  @override
  String get syncStatus => 'Status';

  @override
  String get syncStatusPaired => 'Paired';

  @override
  String get syncYes => 'Yes';

  @override
  String get syncNo => 'No';

  @override
  String get syncOwner => 'Owner address (Arweave)';

  @override
  String get syncLast => 'Last synced';

  @override
  String get syncPending => 'Pending';

  @override
  String syncPendingValue(int files, String config) {
    return '$files file(s)$config';
  }

  @override
  String get syncPlusConfig => ' + config.toml';

  @override
  String syncScan(int files, String config) {
    return 'Scan with the TruthID app to approve and publish ($files file(s)$config).';
  }

  @override
  String get syncWaitingApproval => 'Waiting for approval…';

  @override
  String get acctCodeNewTitle => 'Your new recovery code';

  @override
  String get acctCodeTitle => 'Your data is now encrypted';

  @override
  String get acctCodeBodyNew =>
      'The old code stopped working. Keep this one: it is the only way to recover everything if you forget your password, or if whoever runs the hub resets it.';

  @override
  String get acctCodeBody =>
      'Keep this code: it is the only way to recover everything if you forget your password, or if whoever runs the hub resets it.';

  @override
  String get acctCodeOnce =>
      'It is shown only this once, and nobody on the hub can see it afterwards — not even whoever runs it. Write it down somewhere safe, away from this phone.';

  @override
  String get acctCodeSaved => 'I saved the code somewhere safe';

  @override
  String get acctContinue => 'Continue';

  @override
  String acctPasswordShort(int min) {
    return 'The new password needs at least $min characters.';
  }

  @override
  String get acctPasswordMismatch => 'The two new passwords don\'t match.';

  @override
  String get acctNeedCode => 'Give your recovery code to keep your data.';

  @override
  String get acctWrongProvisional => 'The provisional password is wrong.';

  @override
  String get acctWrongCurrent => 'The current password is wrong.';

  @override
  String get acctChooseTitle => 'Choose your password';

  @override
  String get acctChangeTitle => 'Change your password';

  @override
  String acctHi(String name) {
    return 'Hi $name. Before you start, replace the provisional password with one of your own.';
  }

  @override
  String get acctProvisional => 'Provisional password';

  @override
  String get acctCurrent => 'Current password';

  @override
  String get acctRecoveryCode => 'Recovery code';

  @override
  String get acctRecoveryHelper =>
      'Whoever runs the hub reset your password. Your data only comes back with the code you wrote down.';

  @override
  String get acctNewPassword => 'New password';

  @override
  String get acctNewPasswordAgain => 'New password again';

  @override
  String get acctWrongThat => 'That password is wrong.';

  @override
  String get acctPolicyTitle => 'Who can help recover your data changed';

  @override
  String get acctPolicyCompany =>
      'Whoever runs the hub can recover your data alone, with their recovery key. Every recovery is recorded and you are told.';

  @override
  String get acctPolicyConsent =>
      'Whoever runs the hub can recover your data only together with your recovery code: neither opens it alone.';

  @override
  String get acctPolicyToday =>
      'Today your data follows the previous rule; it only follows this one if you accept.';

  @override
  String get acctPolicyKeep =>
      'Keep in mind: whoever controls the machine the hub runs on can always, technically, see what the agent sees while it works. The record and the notice do not protect against someone editing the hub\'s files.';

  @override
  String get acctPolicyPassword => 'Your password, to accept';

  @override
  String get acctNotNow => 'Not now';

  @override
  String get acctAccept => 'Accept';

  @override
  String get acctRecoveredTitle => 'Your data was recovered';

  @override
  String get acctRecoveredBody =>
      'Whoever runs the hub recovered your data, with their recovery key, and you signed in just now with the provisional password they gave you.';

  @override
  String get acctRecoveredCompany => 'company recovery';

  @override
  String get acctRecoveredConsent => 'recovery with your consent';

  @override
  String get acctRecoveredNotYou =>
      'If it wasn\'t you who asked, tell whoever runs the hub and change your password.';

  @override
  String get acctGotIt => 'Got it';

  @override
  String get acctNewCodeTitle => 'New recovery code';

  @override
  String get acctNewCodeHint =>
      'The old code stops working. Type your password to get a new one.';

  @override
  String get acctTitle => 'Account';

  @override
  String get acctEncrypted => 'Your data is encrypted on the hub';

  @override
  String get acctNotEncrypted =>
      'Your data is not encrypted yet — it is once you sign in with your password';

  @override
  String get acctChangePassword => 'Change password';

  @override
  String get acctGetNewCode => 'Get a new recovery code';

  @override
  String get acctOldStops => 'The old one stops working';

  @override
  String get acctLinkTruthid => 'Link my TruthID';

  @override
  String acctTruthidIs(String name) {
    return 'TruthID: @$name';
  }

  @override
  String get acctLinkHint =>
      'With the invite code whoever runs the hub gave you';

  @override
  String get acctLinkedHint =>
      'To change it, ask whoever runs the hub for a new invite';

  @override
  String get acctPassword => 'Password';

  @override
  String get acctInviteCode => 'Invite code';

  @override
  String get acctUsername => 'My TruthID username';

  @override
  String get acctLink => 'Link';

  @override
  String get notifChannel => 'Chat messages';

  @override
  String get notifChannelDesc =>
      'Notifies you when Warden replies while the app isn\'t in the foreground';

  @override
  String get notifAgentChannel => 'Agent messages';

  @override
  String get notifAgentChannelDesc =>
      'Notifies you when an agent writes to you in its channel';

  @override
  String notifError(String server) {
    return '$server — Error';
  }

  @override
  String notifApproval(String server) {
    return '$server — Approval needed';
  }

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
