import 'dart:async';

import 'package:flutter/foundation.dart';
import 'package:flutter/widgets.dart';
import 'package:flutter_localizations/flutter_localizations.dart';
import 'package:intl/intl.dart' as intl;

import 'app_localizations_en.dart';
import 'app_localizations_pt.dart';

// ignore_for_file: type=lint

/// Callers can lookup localized strings with an instance of AppLocalizations
/// returned by `AppLocalizations.of(context)`.
///
/// Applications need to include `AppLocalizations.delegate()` in their app's
/// `localizationDelegates` list, and the locales they support in the app's
/// `supportedLocales` list. For example:
///
/// ```dart
/// import 'generated/app_localizations.dart';
///
/// return MaterialApp(
///   localizationsDelegates: AppLocalizations.localizationsDelegates,
///   supportedLocales: AppLocalizations.supportedLocales,
///   home: MyApplicationHome(),
/// );
/// ```
///
/// ## Update pubspec.yaml
///
/// Please make sure to update your pubspec.yaml to include the following
/// packages:
///
/// ```yaml
/// dependencies:
///   # Internationalization support.
///   flutter_localizations:
///     sdk: flutter
///   intl: any # Use the pinned version from flutter_localizations
///
///   # Rest of dependencies
/// ```
///
/// ## iOS Applications
///
/// iOS applications define key application metadata, including supported
/// locales, in an Info.plist file that is built into the application bundle.
/// To configure the locales supported by your app, you’ll need to edit this
/// file.
///
/// First, open your project’s ios/Runner.xcworkspace Xcode workspace file.
/// Then, in the Project Navigator, open the Info.plist file under the Runner
/// project’s Runner folder.
///
/// Next, select the Information Property List item, select Add Item from the
/// Editor menu, then select Localizations from the pop-up menu.
///
/// Select and expand the newly-created Localizations item then, for each
/// locale your application supports, add a new item and select the locale
/// you wish to add from the pop-up menu in the Value field. This list should
/// be consistent with the languages listed in the AppLocalizations.supportedLocales
/// property.
abstract class AppLocalizations {
  AppLocalizations(String locale)
    : localeName = intl.Intl.canonicalizedLocale(locale.toString());

  final String localeName;

  static AppLocalizations of(BuildContext context) {
    return Localizations.of<AppLocalizations>(context, AppLocalizations)!;
  }

  static const LocalizationsDelegate<AppLocalizations> delegate =
      _AppLocalizationsDelegate();

  /// A list of this localizations delegate along with the default localizations
  /// delegates.
  ///
  /// Returns a list of localizations delegates containing this delegate along with
  /// GlobalMaterialLocalizations.delegate, GlobalCupertinoLocalizations.delegate,
  /// and GlobalWidgetsLocalizations.delegate.
  ///
  /// Additional delegates can be added by appending to this list in
  /// MaterialApp. This list does not have to be used at all if a custom list
  /// of delegates is preferred or required.
  static const List<LocalizationsDelegate<dynamic>> localizationsDelegates =
      <LocalizationsDelegate<dynamic>>[
        delegate,
        GlobalMaterialLocalizations.delegate,
        GlobalCupertinoLocalizations.delegate,
        GlobalWidgetsLocalizations.delegate,
      ];

  /// A list of this localizations delegate's supported locales.
  static const List<Locale> supportedLocales = <Locale>[
    Locale('en'),
    Locale('pt'),
  ];

  /// No description provided for @languageLabel.
  ///
  /// In en, this message translates to:
  /// **'Language'**
  String get languageLabel;

  /// No description provided for @languageEnglish.
  ///
  /// In en, this message translates to:
  /// **'English'**
  String get languageEnglish;

  /// No description provided for @languagePortuguese.
  ///
  /// In en, this message translates to:
  /// **'Português (Brasil)'**
  String get languagePortuguese;

  /// No description provided for @languageSystem.
  ///
  /// In en, this message translates to:
  /// **'Follow the system'**
  String get languageSystem;

  /// No description provided for @themeToLight.
  ///
  /// In en, this message translates to:
  /// **'Switch to the light theme'**
  String get themeToLight;

  /// No description provided for @themeToDark.
  ///
  /// In en, this message translates to:
  /// **'Switch to the dark theme'**
  String get themeToDark;

  /// No description provided for @commonCancel.
  ///
  /// In en, this message translates to:
  /// **'Cancel'**
  String get commonCancel;

  /// No description provided for @commonSave.
  ///
  /// In en, this message translates to:
  /// **'Save'**
  String get commonSave;

  /// No description provided for @commonEdit.
  ///
  /// In en, this message translates to:
  /// **'Edit'**
  String get commonEdit;

  /// No description provided for @commonRemove.
  ///
  /// In en, this message translates to:
  /// **'Remove'**
  String get commonRemove;

  /// No description provided for @commonDelete.
  ///
  /// In en, this message translates to:
  /// **'Delete'**
  String get commonDelete;

  /// No description provided for @commonKeep.
  ///
  /// In en, this message translates to:
  /// **'Keep'**
  String get commonKeep;

  /// No description provided for @commonClose.
  ///
  /// In en, this message translates to:
  /// **'Close'**
  String get commonClose;

  /// No description provided for @commonRefresh.
  ///
  /// In en, this message translates to:
  /// **'Refresh'**
  String get commonRefresh;

  /// No description provided for @commonLoading.
  ///
  /// In en, this message translates to:
  /// **'Loading…'**
  String get commonLoading;

  /// No description provided for @commonCopy.
  ///
  /// In en, this message translates to:
  /// **'Copy'**
  String get commonCopy;

  /// No description provided for @commonDone.
  ///
  /// In en, this message translates to:
  /// **'Done'**
  String get commonDone;

  /// No description provided for @commonTryAgain.
  ///
  /// In en, this message translates to:
  /// **'Try again'**
  String get commonTryAgain;

  /// No description provided for @folderTitle.
  ///
  /// In en, this message translates to:
  /// **'Working folder'**
  String get folderTitle;

  /// No description provided for @folderHint.
  ///
  /// In en, this message translates to:
  /// **'The AI reads and writes in this folder, and the shell starts there (each command asks for your yes). It applies to the whole conversation and does not change afterwards.'**
  String get folderHint;

  /// No description provided for @folderThisHub.
  ///
  /// In en, this message translates to:
  /// **'This hub'**
  String get folderThisHub;

  /// No description provided for @folderYours.
  ///
  /// In en, this message translates to:
  /// **'Your folders'**
  String get folderYours;

  /// No description provided for @folderUp.
  ///
  /// In en, this message translates to:
  /// **'Up'**
  String get folderUp;

  /// No description provided for @folderEmpty.
  ///
  /// In en, this message translates to:
  /// **'No subfolders.'**
  String get folderEmpty;

  /// No description provided for @folderUse.
  ///
  /// In en, this message translates to:
  /// **'Use this folder'**
  String get folderUse;

  /// No description provided for @connTitle.
  ///
  /// In en, this message translates to:
  /// **'Warden — Server Connection'**
  String get connTitle;

  /// No description provided for @connDiscoverTooltip.
  ///
  /// In en, this message translates to:
  /// **'Discover hubs on the local network'**
  String get connDiscoverTooltip;

  /// No description provided for @connScanTooltip.
  ///
  /// In en, this message translates to:
  /// **'Scan QR to fill in connection'**
  String get connScanTooltip;

  /// No description provided for @connAccountTooltip.
  ///
  /// In en, this message translates to:
  /// **'My account'**
  String get connAccountTooltip;

  /// No description provided for @connSyncTooltip.
  ///
  /// In en, this message translates to:
  /// **'Sync'**
  String get connSyncTooltip;

  /// No description provided for @connSkillsTooltip.
  ///
  /// In en, this message translates to:
  /// **'Skills'**
  String get connSkillsTooltip;

  /// No description provided for @connHost.
  ///
  /// In en, this message translates to:
  /// **'Server host'**
  String get connHost;

  /// No description provided for @connPort.
  ///
  /// In en, this message translates to:
  /// **'Port'**
  String get connPort;

  /// No description provided for @connUseTls.
  ///
  /// In en, this message translates to:
  /// **'Use TLS (wss://)'**
  String get connUseTls;

  /// No description provided for @connUseTlsHint.
  ///
  /// In en, this message translates to:
  /// **'For a hub serving HTTPS — use the name its certificate covers, e.g. hub.tailXXXX.ts.net'**
  String get connUseTlsHint;

  /// No description provided for @connPairingKey.
  ///
  /// In en, this message translates to:
  /// **'Pairing key'**
  String get connPairingKey;

  /// No description provided for @connUsername.
  ///
  /// In en, this message translates to:
  /// **'Username'**
  String get connUsername;

  /// No description provided for @connPassword.
  ///
  /// In en, this message translates to:
  /// **'Password'**
  String get connPassword;

  /// No description provided for @connPasswordHint.
  ///
  /// In en, this message translates to:
  /// **'Only needed the first time on this phone'**
  String get connPasswordHint;

  /// No description provided for @connAuthKey.
  ///
  /// In en, this message translates to:
  /// **'Auth key'**
  String get connAuthKey;

  /// No description provided for @connAuthKeyHint.
  ///
  /// In en, this message translates to:
  /// **'Pairing key, or an invite code'**
  String get connAuthKeyHint;

  /// No description provided for @connDeviceName.
  ///
  /// In en, this message translates to:
  /// **'Device name'**
  String get connDeviceName;

  /// No description provided for @connResume.
  ///
  /// In en, this message translates to:
  /// **'Resume chat'**
  String get connResume;

  /// No description provided for @connDisconnect.
  ///
  /// In en, this message translates to:
  /// **'Disconnect'**
  String get connDisconnect;

  /// No description provided for @connConnect.
  ///
  /// In en, this message translates to:
  /// **'Connect'**
  String get connConnect;

  /// No description provided for @connFillAccount.
  ///
  /// In en, this message translates to:
  /// **'Fill in host, port, username, password and device name'**
  String get connFillAccount;

  /// No description provided for @connFillKey.
  ///
  /// In en, this message translates to:
  /// **'Fill in host, port, auth key and device name'**
  String get connFillKey;

  /// No description provided for @connLocked.
  ///
  /// In en, this message translates to:
  /// **'Your data is locked because the hub restarted. Sign in with your username and password once to unlock it.'**
  String get connLocked;

  /// No description provided for @connChoosePassword.
  ///
  /// In en, this message translates to:
  /// **'Choose your own password to continue'**
  String get connChoosePassword;

  /// No description provided for @connDiscoveryFailed.
  ///
  /// In en, this message translates to:
  /// **'Discovery failed: {error}'**
  String connDiscoveryFailed(String error);

  /// No description provided for @connNoHub.
  ///
  /// In en, this message translates to:
  /// **'No hub answered on the local network.'**
  String get connNoHub;

  /// No description provided for @connHubsFound.
  ///
  /// In en, this message translates to:
  /// **'Hubs found'**
  String get connHubsFound;

  /// No description provided for @connStatusDisconnected.
  ///
  /// In en, this message translates to:
  /// **'Disconnected'**
  String get connStatusDisconnected;

  /// No description provided for @connStatusDisconnectedWhy.
  ///
  /// In en, this message translates to:
  /// **'Disconnected: {reason}'**
  String connStatusDisconnectedWhy(String reason);

  /// No description provided for @connStatusConnecting.
  ///
  /// In en, this message translates to:
  /// **'Connecting…'**
  String get connStatusConnecting;

  /// No description provided for @connStatusConnected.
  ///
  /// In en, this message translates to:
  /// **'Connected to {server}'**
  String connStatusConnected(String server);

  /// No description provided for @connStatusError.
  ///
  /// In en, this message translates to:
  /// **'Error: {message}'**
  String connStatusError(String message);

  /// No description provided for @qrTitle.
  ///
  /// In en, this message translates to:
  /// **'Scan the hub\'s QR'**
  String get qrTitle;

  /// No description provided for @qrNotRecognized.
  ///
  /// In en, this message translates to:
  /// **'QR code not recognized — scan the QR of the desktop\'s Workspace.'**
  String get qrNotRecognized;

  /// No description provided for @categoryDeleteData.
  ///
  /// In en, this message translates to:
  /// **'Delete data'**
  String get categoryDeleteData;

  /// No description provided for @categorySpendMoney.
  ///
  /// In en, this message translates to:
  /// **'Spend money'**
  String get categorySpendMoney;

  /// No description provided for @categoryCriticalInfra.
  ///
  /// In en, this message translates to:
  /// **'Touch critical infrastructure (shell, SSH, nodes)'**
  String get categoryCriticalInfra;

  /// No description provided for @categoryExternalMessage.
  ///
  /// In en, this message translates to:
  /// **'Send an external message (browser, outside services)'**
  String get categoryExternalMessage;

  /// No description provided for @categoryPublishCode.
  ///
  /// In en, this message translates to:
  /// **'Publish code'**
  String get categoryPublishCode;

  /// No description provided for @categoryImportantConfig.
  ///
  /// In en, this message translates to:
  /// **'Change important configuration (scheduled tasks)'**
  String get categoryImportantConfig;

  /// No description provided for @categoryElevatedAgent.
  ///
  /// In en, this message translates to:
  /// **'Create or change agents'**
  String get categoryElevatedAgent;

  /// No description provided for @autonomy1.
  ///
  /// In en, this message translates to:
  /// **'1 — only answers, no tools'**
  String get autonomy1;

  /// No description provided for @autonomy2.
  ///
  /// In en, this message translates to:
  /// **'2 — suggests: reads, refuses the rest with a suggestion'**
  String get autonomy2;

  /// No description provided for @autonomy3.
  ///
  /// In en, this message translates to:
  /// **'3 — asks for your yes before any change'**
  String get autonomy3;

  /// No description provided for @autonomy4.
  ///
  /// In en, this message translates to:
  /// **'4 — acts on its own (the checked kinds still ask)'**
  String get autonomy4;

  /// No description provided for @autonomy5.
  ///
  /// In en, this message translates to:
  /// **'5 — also manages its reports on its own'**
  String get autonomy5;

  /// No description provided for @sideTo.
  ///
  /// In en, this message translates to:
  /// **'to {name}'**
  String sideTo(String name);

  /// No description provided for @sideFrom.
  ///
  /// In en, this message translates to:
  /// **'from {name}'**
  String sideFrom(String name);

  /// No description provided for @sideScheduledTask.
  ///
  /// In en, this message translates to:
  /// **'scheduled task'**
  String get sideScheduledTask;

  /// No description provided for @sideWebhook.
  ///
  /// In en, this message translates to:
  /// **'webhook'**
  String get sideWebhook;

  /// No description provided for @sideButton.
  ///
  /// In en, this message translates to:
  /// **'Notes and runs'**
  String get sideButton;

  /// No description provided for @sideButtonCount.
  ///
  /// In en, this message translates to:
  /// **'Notes and runs ({count})'**
  String sideButtonCount(int count);

  /// No description provided for @commonWait.
  ///
  /// In en, this message translates to:
  /// **'Wait…'**
  String get commonWait;

  /// No description provided for @commonConfirm.
  ///
  /// In en, this message translates to:
  /// **'Confirm'**
  String get commonConfirm;

  /// No description provided for @agentsTitle.
  ///
  /// In en, this message translates to:
  /// **'Agents'**
  String get agentsTitle;

  /// No description provided for @agentsTabOrganization.
  ///
  /// In en, this message translates to:
  /// **'Organization'**
  String get agentsTabOrganization;

  /// No description provided for @agentsTabTasks.
  ///
  /// In en, this message translates to:
  /// **'Tasks'**
  String get agentsTabTasks;

  /// No description provided for @agentsTabActivity.
  ///
  /// In en, this message translates to:
  /// **'Activity'**
  String get agentsTabActivity;

  /// No description provided for @pairingWrong.
  ///
  /// In en, this message translates to:
  /// **'Wrong pairing key.'**
  String get pairingWrong;

  /// No description provided for @pairingLabel.
  ///
  /// In en, this message translates to:
  /// **'Hub\'s pairing key'**
  String get pairingLabel;

  /// No description provided for @pairingHelper.
  ///
  /// In en, this message translates to:
  /// **'Asked on every change; never kept.'**
  String get pairingHelper;

  /// No description provided for @positionTitle.
  ///
  /// In en, this message translates to:
  /// **'Position of {agent}'**
  String positionTitle(String agent);

  /// No description provided for @positionRole.
  ///
  /// In en, this message translates to:
  /// **'Role'**
  String get positionRole;

  /// No description provided for @positionRoleHint.
  ///
  /// In en, this message translates to:
  /// **'e.g. backend lead'**
  String get positionRoleHint;

  /// No description provided for @positionReportsTo.
  ///
  /// In en, this message translates to:
  /// **'Reports to'**
  String get positionReportsTo;

  /// No description provided for @positionReportsHelper.
  ///
  /// In en, this message translates to:
  /// **'Whoever reports to {agent} goes with it.'**
  String positionReportsHelper(String agent);

  /// No description provided for @positionNobody.
  ///
  /// In en, this message translates to:
  /// **'Nobody (top of the tree)'**
  String get positionNobody;

  /// No description provided for @powersTitle.
  ///
  /// In en, this message translates to:
  /// **'Powers of {agent}'**
  String powersTitle(String agent);

  /// No description provided for @powersAutonomy.
  ///
  /// In en, this message translates to:
  /// **'Autonomy'**
  String get powersAutonomy;

  /// No description provided for @powersLevel5.
  ///
  /// In en, this message translates to:
  /// **'Level 5 only counts for an agent that may manage agents, and {agent} may not: that is turned on in the settings.'**
  String powersLevel5(String agent);

  /// No description provided for @powersStartTasks.
  ///
  /// In en, this message translates to:
  /// **'Can start background work'**
  String get powersStartTasks;

  /// No description provided for @powersCreateWorkers.
  ///
  /// In en, this message translates to:
  /// **'Can create temporary agents'**
  String get powersCreateWorkers;

  /// No description provided for @powersMessageUser.
  ///
  /// In en, this message translates to:
  /// **'Can start a conversation with you'**
  String get powersMessageUser;

  /// No description provided for @powersChooseModels.
  ///
  /// In en, this message translates to:
  /// **'Can pick the model of the tasks it delegates'**
  String get powersChooseModels;

  /// No description provided for @powersAsks.
  ///
  /// In en, this message translates to:
  /// **'Asks for your yes before (even when acting on its own):'**
  String get powersAsks;

  /// No description provided for @limitTitle.
  ///
  /// In en, this message translates to:
  /// **'Models of {agent}'**
  String limitTitle(String agent);

  /// No description provided for @limitHint.
  ///
  /// In en, this message translates to:
  /// **'The models it can pick for the tasks it delegates. None checked leaves the choice open; with one, it dictates the model.'**
  String get limitHint;

  /// No description provided for @limitNone.
  ///
  /// In en, this message translates to:
  /// **'The hub has no providers or policies to pick from.'**
  String get limitNone;

  /// No description provided for @limitPolicy.
  ///
  /// In en, this message translates to:
  /// **'{id} (policy)'**
  String limitPolicy(String id);

  /// No description provided for @limitDefault.
  ///
  /// In en, this message translates to:
  /// **'Default'**
  String get limitDefault;

  /// No description provided for @limitDefaultHelper.
  ///
  /// In en, this message translates to:
  /// **'What a task gets when {agent} does not pick.'**
  String limitDefaultHelper(String agent);

  /// No description provided for @policyNew.
  ///
  /// In en, this message translates to:
  /// **'New policy'**
  String get policyNew;

  /// No description provided for @policyTitle.
  ///
  /// In en, this message translates to:
  /// **'Policy {id}'**
  String policyTitle(String id);

  /// No description provided for @policyName.
  ///
  /// In en, this message translates to:
  /// **'Name'**
  String get policyName;

  /// No description provided for @policyAnsweredBy.
  ///
  /// In en, this message translates to:
  /// **'Answered by'**
  String get policyAnsweredBy;

  /// No description provided for @policyWhen.
  ///
  /// In en, this message translates to:
  /// **'When to pick it (one line)'**
  String get policyWhen;

  /// No description provided for @policyWhenHint.
  ///
  /// In en, this message translates to:
  /// **'e.g. simple, cheap work'**
  String get policyWhenHint;

  /// No description provided for @policySave.
  ///
  /// In en, this message translates to:
  /// **'Save policy'**
  String get policySave;

  /// No description provided for @newAgentTop.
  ///
  /// In en, this message translates to:
  /// **'New agent at the top'**
  String get newAgentTop;

  /// No description provided for @newAgentUnder.
  ///
  /// In en, this message translates to:
  /// **'New agent under {parent}'**
  String newAgentUnder(String parent);

  /// No description provided for @newAgentName.
  ///
  /// In en, this message translates to:
  /// **'Name'**
  String get newAgentName;

  /// No description provided for @newAgentNameHint.
  ///
  /// In en, this message translates to:
  /// **'e.g. reviewer'**
  String get newAgentNameHint;

  /// No description provided for @newAgentRole.
  ///
  /// In en, this message translates to:
  /// **'Role (optional)'**
  String get newAgentRole;

  /// No description provided for @newAgentDoes.
  ///
  /// In en, this message translates to:
  /// **'What it does'**
  String get newAgentDoes;

  /// No description provided for @newAgentHelper.
  ///
  /// In en, this message translates to:
  /// **'Starts careful: read-only tools, asks before any change, and cannot delegate or manage agents until a person turns that on.'**
  String get newAgentHelper;

  /// No description provided for @newAgentAdd.
  ///
  /// In en, this message translates to:
  /// **'Add agent'**
  String get newAgentAdd;

  /// No description provided for @orgRestarts.
  ///
  /// In en, this message translates to:
  /// **'The hub restarts with the change.'**
  String get orgRestarts;

  /// No description provided for @orgChange.
  ///
  /// In en, this message translates to:
  /// **'Change {agent}'**
  String orgChange(String agent);

  /// No description provided for @orgAdd.
  ///
  /// In en, this message translates to:
  /// **'Add {agent}'**
  String orgAdd(String agent);

  /// No description provided for @orgSavePolicy.
  ///
  /// In en, this message translates to:
  /// **'Save the policy {id}'**
  String orgSavePolicy(String id);

  /// No description provided for @orgRemovePolicy.
  ///
  /// In en, this message translates to:
  /// **'Remove the policy {id}'**
  String orgRemovePolicy(String id);

  /// No description provided for @delegationOpen.
  ///
  /// In en, this message translates to:
  /// **'Open: the agent picks any model for each task it delegates.'**
  String get delegationOpen;

  /// No description provided for @delegationDictated.
  ///
  /// In en, this message translates to:
  /// **'Dictated: every task it delegates runs on {model}.'**
  String delegationDictated(String model);

  /// No description provided for @delegationLimited.
  ///
  /// In en, this message translates to:
  /// **'Limited to {count} models; {model} is what a task gets when the agent does not pick.'**
  String delegationLimited(int count, String model);

  /// No description provided for @taskPending.
  ///
  /// In en, this message translates to:
  /// **'Pending'**
  String get taskPending;

  /// No description provided for @taskRunning.
  ///
  /// In en, this message translates to:
  /// **'Running'**
  String get taskRunning;

  /// No description provided for @taskWaiting.
  ///
  /// In en, this message translates to:
  /// **'Waiting for an agent'**
  String get taskWaiting;

  /// No description provided for @taskPaused.
  ///
  /// In en, this message translates to:
  /// **'Paused'**
  String get taskPaused;

  /// No description provided for @taskDone.
  ///
  /// In en, this message translates to:
  /// **'Done'**
  String get taskDone;

  /// No description provided for @taskFailed.
  ///
  /// In en, this message translates to:
  /// **'Failed'**
  String get taskFailed;

  /// No description provided for @taskCancelled.
  ///
  /// In en, this message translates to:
  /// **'Stopped'**
  String get taskCancelled;

  /// No description provided for @taskActionPause.
  ///
  /// In en, this message translates to:
  /// **'Pause'**
  String get taskActionPause;

  /// No description provided for @taskActionResume.
  ///
  /// In en, this message translates to:
  /// **'Resume'**
  String get taskActionResume;

  /// No description provided for @taskActionCancel.
  ///
  /// In en, this message translates to:
  /// **'Stop'**
  String get taskActionCancel;

  /// No description provided for @grantDeleteData.
  ///
  /// In en, this message translates to:
  /// **'Delete data'**
  String get grantDeleteData;

  /// No description provided for @grantSpendMoney.
  ///
  /// In en, this message translates to:
  /// **'Spend money'**
  String get grantSpendMoney;

  /// No description provided for @grantCriticalInfra.
  ///
  /// In en, this message translates to:
  /// **'Critical infrastructure'**
  String get grantCriticalInfra;

  /// No description provided for @grantExternalMessage.
  ///
  /// In en, this message translates to:
  /// **'External message'**
  String get grantExternalMessage;

  /// No description provided for @grantPublishCode.
  ///
  /// In en, this message translates to:
  /// **'Publish code'**
  String get grantPublishCode;

  /// No description provided for @grantImportantConfig.
  ///
  /// In en, this message translates to:
  /// **'Important configuration'**
  String get grantImportantConfig;

  /// No description provided for @grantElevatedAgent.
  ///
  /// In en, this message translates to:
  /// **'Agent with elevated permission'**
  String get grantElevatedAgent;

  /// No description provided for @spendWindow.
  ///
  /// In en, this message translates to:
  /// **'in the last {hours} h'**
  String spendWindow(int hours);

  /// No description provided for @spendUnpricedOne.
  ///
  /// In en, this message translates to:
  /// **'{count} unpriced call'**
  String spendUnpricedOne(int count);

  /// No description provided for @spendUnpricedMany.
  ///
  /// In en, this message translates to:
  /// **'{count} unpriced calls'**
  String spendUnpricedMany(int count);

  /// No description provided for @spendUnder.
  ///
  /// In en, this message translates to:
  /// **'under {amount}'**
  String spendUnder(String amount);

  /// No description provided for @spendWithUnpriced.
  ///
  /// In en, this message translates to:
  /// **'{money} {window} (and {unpriced})'**
  String spendWithUnpriced(String money, String window, String unpriced);

  /// No description provided for @agoNow.
  ///
  /// In en, this message translates to:
  /// **'just now'**
  String get agoNow;

  /// No description provided for @agoMinutes.
  ///
  /// In en, this message translates to:
  /// **'{count} min ago'**
  String agoMinutes(int count);

  /// No description provided for @agoHours.
  ///
  /// In en, this message translates to:
  /// **'{count} h ago'**
  String agoHours(int count);

  /// No description provided for @agoDays.
  ///
  /// In en, this message translates to:
  /// **'{count} d ago'**
  String agoDays(int count);

  /// No description provided for @tallyDone.
  ///
  /// In en, this message translates to:
  /// **'{count} done'**
  String tallyDone(int count);

  /// No description provided for @tallyFailed.
  ///
  /// In en, this message translates to:
  /// **'{count} failed'**
  String tallyFailed(int count);

  /// No description provided for @tallyStopped.
  ///
  /// In en, this message translates to:
  /// **'{count} stopped'**
  String tallyStopped(int count);

  /// No description provided for @tallyRunning.
  ///
  /// In en, this message translates to:
  /// **'{count} running'**
  String tallyRunning(int count);

  /// No description provided for @tallyWaiting.
  ///
  /// In en, this message translates to:
  /// **'{count} waiting for an agent'**
  String tallyWaiting(int count);

  /// No description provided for @tallyPaused.
  ///
  /// In en, this message translates to:
  /// **'{count} paused'**
  String tallyPaused(int count);

  /// No description provided for @tallyPending.
  ///
  /// In en, this message translates to:
  /// **'{count} pending'**
  String tallyPending(int count);

  /// No description provided for @tallyTokens.
  ///
  /// In en, this message translates to:
  /// **'{tokens} tokens'**
  String tallyTokens(String tokens);

  /// No description provided for @tallyDelegated.
  ///
  /// In en, this message translates to:
  /// **'delegated {count}'**
  String tallyDelegated(int count);

  /// No description provided for @tallyActiveAgo.
  ///
  /// In en, this message translates to:
  /// **'active {ago}'**
  String tallyActiveAgo(String ago);

  /// No description provided for @activityNoAgent.
  ///
  /// In en, this message translates to:
  /// **'The assistant'**
  String get activityNoAgent;

  /// No description provided for @activityCreatedAgent.
  ///
  /// In en, this message translates to:
  /// **'{actor} created the agent {target}'**
  String activityCreatedAgent(String actor, String target);

  /// No description provided for @activityRemovedAgent.
  ///
  /// In en, this message translates to:
  /// **'{actor} removed the agent {target}'**
  String activityRemovedAgent(String actor, String target);

  /// No description provided for @activityUpdatedAgent.
  ///
  /// In en, this message translates to:
  /// **'{actor} changed the agent {target}'**
  String activityUpdatedAgent(String actor, String target);

  /// No description provided for @activityScheduledRan.
  ///
  /// In en, this message translates to:
  /// **'{actor} ran the scheduled task {target}'**
  String activityScheduledRan(String actor, String target);

  /// No description provided for @activityScheduledFailed.
  ///
  /// In en, this message translates to:
  /// **'The scheduled task {target} failed'**
  String activityScheduledFailed(String target);

  /// No description provided for @activityWebhookRan.
  ///
  /// In en, this message translates to:
  /// **'{actor} answered the webhook {target}'**
  String activityWebhookRan(String actor, String target);

  /// No description provided for @activityWebhookFailed.
  ///
  /// In en, this message translates to:
  /// **'The webhook {target} failed'**
  String activityWebhookFailed(String target);

  /// No description provided for @activityDelegated.
  ///
  /// In en, this message translates to:
  /// **'{actor} delegated a task to {target}'**
  String activityDelegated(String actor, String target);

  /// No description provided for @activityStarted.
  ///
  /// In en, this message translates to:
  /// **'{actor} started the task'**
  String activityStarted(String actor);

  /// No description provided for @activityDone.
  ///
  /// In en, this message translates to:
  /// **'{actor} finished the task'**
  String activityDone(String actor);

  /// No description provided for @activityFailed.
  ///
  /// In en, this message translates to:
  /// **'{actor} could not finish the task'**
  String activityFailed(String actor);

  /// No description provided for @activityCancelled.
  ///
  /// In en, this message translates to:
  /// **'{actor}\'s task was cancelled'**
  String activityCancelled(String actor);

  /// No description provided for @activityNote.
  ///
  /// In en, this message translates to:
  /// **'{actor} left a note for {target}'**
  String activityNote(String actor, String target);

  /// No description provided for @activityReply.
  ///
  /// In en, this message translates to:
  /// **'{actor} answered {target}'**
  String activityReply(String actor, String target);

  /// No description provided for @activityMessagedUser.
  ///
  /// In en, this message translates to:
  /// **'{actor} wrote to you'**
  String activityMessagedUser(String actor);

  /// No description provided for @activityToday.
  ///
  /// In en, this message translates to:
  /// **'Today'**
  String get activityToday;

  /// No description provided for @activityYesterday.
  ///
  /// In en, this message translates to:
  /// **'Yesterday'**
  String get activityYesterday;

  /// No description provided for @activityDate.
  ///
  /// In en, this message translates to:
  /// **'{day} {month, select, 1{January} 2{February} 3{March} 4{April} 5{May} 6{June} 7{July} 8{August} 9{September} 10{October} 11{November} other{December}} {year}'**
  String activityDate(int day, String month, int year);

  /// No description provided for @orgRemoveTitle.
  ///
  /// In en, this message translates to:
  /// **'Remove {agent}?'**
  String orgRemoveTitle(String agent);

  /// No description provided for @orgRemoveBody.
  ///
  /// In en, this message translates to:
  /// **'{count, plural, =0{} one{Its report goes to report to {goesTo}. } other{Its {count} reports go to report to {goesTo}. }}The conversations stay; this can\'t be undone.'**
  String orgRemoveBody(int count, String goesTo);

  /// No description provided for @orgRemove.
  ///
  /// In en, this message translates to:
  /// **'Remove {agent}'**
  String orgRemove(String agent);

  /// No description provided for @orgNobodyTop.
  ///
  /// In en, this message translates to:
  /// **'nobody (the top of the tree)'**
  String get orgNobodyTop;

  /// No description provided for @badgeDelegates.
  ///
  /// In en, this message translates to:
  /// **'delegates'**
  String get badgeDelegates;

  /// No description provided for @badgeManages.
  ///
  /// In en, this message translates to:
  /// **'manages agents'**
  String get badgeManages;

  /// No description provided for @badgeNotes.
  ///
  /// In en, this message translates to:
  /// **'leaves notes'**
  String get badgeNotes;

  /// No description provided for @badgeTasks.
  ///
  /// In en, this message translates to:
  /// **'schedules tasks'**
  String get badgeTasks;

  /// No description provided for @badgeNoBackground.
  ///
  /// In en, this message translates to:
  /// **'no background work'**
  String get badgeNoBackground;

  /// No description provided for @badgeNoTemp.
  ///
  /// In en, this message translates to:
  /// **'no temporary agents'**
  String get badgeNoTemp;

  /// No description provided for @badgeNoChat.
  ///
  /// In en, this message translates to:
  /// **'cannot start a chat'**
  String get badgeNoChat;

  /// No description provided for @badgeNoModels.
  ///
  /// In en, this message translates to:
  /// **'cannot pick models'**
  String get badgeNoModels;

  /// No description provided for @badgeAutonomy.
  ///
  /// In en, this message translates to:
  /// **'autonomy {level}: {label}'**
  String badgeAutonomy(int level, String label);

  /// No description provided for @autonomyShort1.
  ///
  /// In en, this message translates to:
  /// **'only answers'**
  String get autonomyShort1;

  /// No description provided for @autonomyShort2.
  ///
  /// In en, this message translates to:
  /// **'suggests'**
  String get autonomyShort2;

  /// No description provided for @autonomyShort3.
  ///
  /// In en, this message translates to:
  /// **'asks first'**
  String get autonomyShort3;

  /// No description provided for @autonomyShort4.
  ///
  /// In en, this message translates to:
  /// **'acts alone'**
  String get autonomyShort4;

  /// No description provided for @autonomyShort5.
  ///
  /// In en, this message translates to:
  /// **'manages alone'**
  String get autonomyShort5;

  /// No description provided for @badgeAsksFirst.
  ///
  /// In en, this message translates to:
  /// **'{count, plural, one{asks first: 1 kind of action} other{asks first: {count} kinds of action}}'**
  String badgeAsksFirst(int count);

  /// No description provided for @orgReports.
  ///
  /// In en, this message translates to:
  /// **'{count, plural, one{1 report} other{{count} reports}}'**
  String orgReports(int count);

  /// No description provided for @menuChat.
  ///
  /// In en, this message translates to:
  /// **'Chat'**
  String get menuChat;

  /// No description provided for @menuTasks.
  ///
  /// In en, this message translates to:
  /// **'Tasks'**
  String get menuTasks;

  /// No description provided for @menuEditPosition.
  ///
  /// In en, this message translates to:
  /// **'Edit position'**
  String get menuEditPosition;

  /// No description provided for @menuPowers.
  ///
  /// In en, this message translates to:
  /// **'Powers'**
  String get menuPowers;

  /// No description provided for @menuModels.
  ///
  /// In en, this message translates to:
  /// **'Models it can pick'**
  String get menuModels;

  /// No description provided for @menuAddReport.
  ///
  /// In en, this message translates to:
  /// **'Add a report'**
  String get menuAddReport;

  /// No description provided for @orgHint.
  ///
  /// In en, this message translates to:
  /// **'Who reports to whom. An agent that manages or delegates to others reaches only the ones below it. Anything an agent changes still waits for your yes.'**
  String get orgHint;

  /// No description provided for @orgNone.
  ///
  /// In en, this message translates to:
  /// **'No agents yet.'**
  String get orgNone;

  /// No description provided for @orgNobodyReports.
  ///
  /// In en, this message translates to:
  /// **'Nobody reports to anybody yet: use \"Edit position\" on an agent to pick its superior.'**
  String get orgNobodyReports;

  /// No description provided for @orgAddTop.
  ///
  /// In en, this message translates to:
  /// **'Add an agent at the top'**
  String get orgAddTop;

  /// No description provided for @policiesTitle.
  ///
  /// In en, this message translates to:
  /// **'Model policies'**
  String get policiesTitle;

  /// No description provided for @policiesNone.
  ///
  /// In en, this message translates to:
  /// **'None: an agent that delegates sees only the model ids.'**
  String get policiesNone;

  /// No description provided for @grantsTitle.
  ///
  /// In en, this message translates to:
  /// **'Permissions given with \"always\"'**
  String get grantsTitle;

  /// No description provided for @grantsHintKept.
  ///
  /// In en, this message translates to:
  /// **'They hold per conversation and agent. {ttl, plural, one{Kept in a file, they survive a restart and lapse after 1 day unused (each use renews them).} other{Kept in a file, they survive a restart and lapse after {ttl} days unused (each use renews them).}} Taking one back makes the agent ask again.'**
  String grantsHintKept(int ttl);

  /// No description provided for @grantsHintSession.
  ///
  /// In en, this message translates to:
  /// **'They hold per conversation and agent. They end when the connection closes. Taking one back makes the agent ask again.'**
  String get grantsHintSession;

  /// No description provided for @grantsNone.
  ///
  /// In en, this message translates to:
  /// **'None yet.'**
  String get grantsNone;

  /// No description provided for @grantDefaultAgent.
  ///
  /// In en, this message translates to:
  /// **'default agent'**
  String get grantDefaultAgent;

  /// No description provided for @grantConversation.
  ///
  /// In en, this message translates to:
  /// **'conversation {id}'**
  String grantConversation(String id);

  /// No description provided for @grantUsed.
  ///
  /// In en, this message translates to:
  /// **'used {ago}'**
  String grantUsed(String ago);

  /// No description provided for @grantTakeBack.
  ///
  /// In en, this message translates to:
  /// **'Take back'**
  String get grantTakeBack;

  /// No description provided for @taskControlTitle.
  ///
  /// In en, this message translates to:
  /// **'{action} the task of {agent}'**
  String taskControlTitle(String action, String agent);

  /// No description provided for @taskControlStops.
  ///
  /// In en, this message translates to:
  /// **'Its subtasks stop too.'**
  String get taskControlStops;

  /// No description provided for @taskControlOnly.
  ///
  /// In en, this message translates to:
  /// **'Only a task running on this hub can be controlled.'**
  String get taskControlOnly;

  /// No description provided for @taskAnAgent.
  ///
  /// In en, this message translates to:
  /// **'An agent'**
  String get taskAnAgent;

  /// No description provided for @taskWorking.
  ///
  /// In en, this message translates to:
  /// **'working'**
  String get taskWorking;

  /// No description provided for @taskProgress.
  ///
  /// In en, this message translates to:
  /// **'{finished} of {total} finished ({percent}%) · {parts}'**
  String taskProgress(int finished, int total, int percent, String parts);

  /// No description provided for @taskTok.
  ///
  /// In en, this message translates to:
  /// **'{tokens} tok'**
  String taskTok(String tokens);

  /// No description provided for @taskDetailTask.
  ///
  /// In en, this message translates to:
  /// **'Task'**
  String get taskDetailTask;

  /// No description provided for @taskDetailResult.
  ///
  /// In en, this message translates to:
  /// **'Result'**
  String get taskDetailResult;

  /// No description provided for @taskDetailWhy.
  ///
  /// In en, this message translates to:
  /// **'Why it stopped'**
  String get taskDetailWhy;

  /// No description provided for @taskNoResult.
  ///
  /// In en, this message translates to:
  /// **'{state}: no result yet.'**
  String taskNoResult(String state);

  /// No description provided for @tasksHint.
  ///
  /// In en, this message translates to:
  /// **'Tasks the agents handed each other in the background on this hub, where each one is and what it cost.'**
  String get tasksHint;

  /// No description provided for @tasksOnly.
  ///
  /// In en, this message translates to:
  /// **'Only the tasks of {agent}: the ones it received and the ones it delegated.'**
  String tasksOnly(String agent);

  /// No description provided for @tasksShowAll.
  ///
  /// In en, this message translates to:
  /// **'Show all'**
  String get tasksShowAll;

  /// No description provided for @tasksNoneFor.
  ///
  /// In en, this message translates to:
  /// **'No task involves {agent} yet.'**
  String tasksNoneFor(String agent);

  /// No description provided for @tasksNone.
  ///
  /// In en, this message translates to:
  /// **'Nothing yet. When an agent delegates a task with \"background\", it shows up here.'**
  String get tasksNone;

  /// No description provided for @activityHint.
  ///
  /// In en, this message translates to:
  /// **'What happened among the agents on this hub: who delegated, started and finished what, the notes they left each other and the messages an agent wrote first to you.'**
  String get activityHint;

  /// No description provided for @activityAll.
  ///
  /// In en, this message translates to:
  /// **'All agents'**
  String get activityAll;

  /// No description provided for @activityNothingFor.
  ///
  /// In en, this message translates to:
  /// **'Nothing involves {agent} yet.'**
  String activityNothingFor(String agent);

  /// No description provided for @activityNothing.
  ///
  /// In en, this message translates to:
  /// **'Nothing yet. When agents delegate tasks, leave each other notes or write to you, it shows up here.'**
  String get activityNothing;

  /// No description provided for @chatApprovalTitle.
  ///
  /// In en, this message translates to:
  /// **'Approval needed'**
  String get chatApprovalTitle;

  /// No description provided for @chatApprovalCategory.
  ///
  /// In en, this message translates to:
  /// **'Category: {category} (this agent asks for approval on this)'**
  String chatApprovalCategory(String category);

  /// No description provided for @chatDeny.
  ///
  /// In en, this message translates to:
  /// **'Deny'**
  String get chatDeny;

  /// No description provided for @chatApprove.
  ///
  /// In en, this message translates to:
  /// **'Approve'**
  String get chatApprove;

  /// No description provided for @chatSyncSnack.
  ///
  /// In en, this message translates to:
  /// **'Sync: {message}'**
  String chatSyncSnack(String message);

  /// No description provided for @chatEditTitle.
  ///
  /// In en, this message translates to:
  /// **'Edit message'**
  String get chatEditTitle;

  /// No description provided for @chatSend.
  ///
  /// In en, this message translates to:
  /// **'Send'**
  String get chatSend;

  /// No description provided for @chatEditWarning.
  ///
  /// In en, this message translates to:
  /// **'{count, plural, one{Sending this deletes the message after it, and the threads on it.} other{Sending this deletes the {count} messages after it, and the threads on them.}}'**
  String chatEditWarning(int count);

  /// No description provided for @chatChannelFailed.
  ///
  /// In en, this message translates to:
  /// **'Couldn\'t open the channel: {error}'**
  String chatChannelFailed(String error);

  /// No description provided for @chatFilesTitle.
  ///
  /// In en, this message translates to:
  /// **'Files'**
  String get chatFilesTitle;

  /// No description provided for @chatFilesNone.
  ///
  /// In en, this message translates to:
  /// **'No folder configured yet.'**
  String get chatFilesNone;

  /// No description provided for @chatFilesConfigured.
  ///
  /// In en, this message translates to:
  /// **'Configured folder:\n{uri}'**
  String chatFilesConfigured(String uri);

  /// No description provided for @chatFilesHint.
  ///
  /// In en, this message translates to:
  /// **'Lets the model list and read text files under this folder. Takes effect the next time you connect.'**
  String get chatFilesHint;

  /// No description provided for @chatFilesClear.
  ///
  /// In en, this message translates to:
  /// **'Clear'**
  String get chatFilesClear;

  /// No description provided for @chatFilesChoose.
  ///
  /// In en, this message translates to:
  /// **'Choose folder'**
  String get chatFilesChoose;

  /// No description provided for @chatNewConversation.
  ///
  /// In en, this message translates to:
  /// **'New conversation'**
  String get chatNewConversation;

  /// No description provided for @chatAs.
  ///
  /// In en, this message translates to:
  /// **'as {agent}'**
  String chatAs(String agent);

  /// No description provided for @chatChannelsTooltip.
  ///
  /// In en, this message translates to:
  /// **'Agent channels'**
  String get chatChannelsTooltip;

  /// No description provided for @chatChannelsTooltipNew.
  ///
  /// In en, this message translates to:
  /// **'Agent channels ({count} new)'**
  String chatChannelsTooltipNew(int count);

  /// No description provided for @chatAgentsTooltip.
  ///
  /// In en, this message translates to:
  /// **'Agents'**
  String get chatAgentsTooltip;

  /// No description provided for @chatUsageTooltip.
  ///
  /// In en, this message translates to:
  /// **'Usage'**
  String get chatUsageTooltip;

  /// No description provided for @chatDevicesTooltip.
  ///
  /// In en, this message translates to:
  /// **'Devices'**
  String get chatDevicesTooltip;

  /// No description provided for @chatConversationsTooltip.
  ///
  /// In en, this message translates to:
  /// **'Conversations'**
  String get chatConversationsTooltip;

  /// No description provided for @chatFilesTooltip.
  ///
  /// In en, this message translates to:
  /// **'Files'**
  String get chatFilesTooltip;

  /// No description provided for @chatDisconnect.
  ///
  /// In en, this message translates to:
  /// **'Disconnect'**
  String get chatDisconnect;

  /// No description provided for @chatThreadBack.
  ///
  /// In en, this message translates to:
  /// **'Back to the conversation'**
  String get chatThreadBack;

  /// No description provided for @chatChannelBack.
  ///
  /// In en, this message translates to:
  /// **'Back to the conversations'**
  String get chatChannelBack;

  /// No description provided for @chatWorkBack.
  ///
  /// In en, this message translates to:
  /// **'Back to notes and runs'**
  String get chatWorkBack;

  /// No description provided for @chatRegenerate.
  ///
  /// In en, this message translates to:
  /// **'Regenerate'**
  String get chatRegenerate;

  /// No description provided for @chatTopUp.
  ///
  /// In en, this message translates to:
  /// **'See balance'**
  String get chatTopUp;

  /// No description provided for @chatCredit.
  ///
  /// In en, this message translates to:
  /// **'Credit: {amount}'**
  String chatCredit(String amount);

  /// No description provided for @chatCreditEmpty.
  ///
  /// In en, this message translates to:
  /// **'Credit: none left'**
  String get chatCreditEmpty;

  /// No description provided for @folderBarNone.
  ///
  /// In en, this message translates to:
  /// **'Working folder: none'**
  String get folderBarNone;

  /// No description provided for @folderBarTooltip.
  ///
  /// In en, this message translates to:
  /// **'Pick a folder of the hub or of a node for the AI to work in'**
  String get folderBarTooltip;

  /// No description provided for @folderBarClear.
  ///
  /// In en, this message translates to:
  /// **'Remove the folder'**
  String get folderBarClear;

  /// No description provided for @channelsNone.
  ///
  /// In en, this message translates to:
  /// **'No agents configured yet.'**
  String get channelsNone;

  /// No description provided for @channelsAnswering.
  ///
  /// In en, this message translates to:
  /// **'answering…'**
  String get channelsAnswering;

  /// No description provided for @channelsNew.
  ///
  /// In en, this message translates to:
  /// **'new'**
  String get channelsNew;

  /// No description provided for @workNone.
  ///
  /// In en, this message translates to:
  /// **'{agent} has not traded notes with other agents or run on its own yet.'**
  String workNone(String agent);

  /// No description provided for @agentMenuTooltip.
  ///
  /// In en, this message translates to:
  /// **'Agent'**
  String get agentMenuTooltip;

  /// No description provided for @agentMenuNone.
  ///
  /// In en, this message translates to:
  /// **'No agent'**
  String get agentMenuNone;

  /// No description provided for @agentMenuRemoved.
  ///
  /// In en, this message translates to:
  /// **'{agent} (removed)'**
  String agentMenuRemoved(String agent);

  /// No description provided for @attachmentUnsupported.
  ///
  /// In en, this message translates to:
  /// **'📎 {mime} attached (playback not supported here yet)'**
  String attachmentUnsupported(String mime);

  /// No description provided for @attachmentDecodeFailed.
  ///
  /// In en, this message translates to:
  /// **'Could not decode {mime} attachment'**
  String attachmentDecodeFailed(String mime);

  /// No description provided for @attachmentPlayFailed.
  ///
  /// In en, this message translates to:
  /// **'Could not play {mime} attachment'**
  String attachmentPlayFailed(String mime);

  /// No description provided for @inputHint.
  ///
  /// In en, this message translates to:
  /// **'Message'**
  String get inputHint;

  /// No description provided for @usageTitle.
  ///
  /// In en, this message translates to:
  /// **'Usage'**
  String get usageTitle;

  /// No description provided for @usageLoadFailed.
  ///
  /// In en, this message translates to:
  /// **'Couldn\'t load the usage: {error}'**
  String usageLoadFailed(String error);

  /// No description provided for @usageCredit.
  ///
  /// In en, this message translates to:
  /// **'Credits'**
  String get usageCredit;

  /// No description provided for @usageCreditAvailable.
  ///
  /// In en, this message translates to:
  /// **'{amount} available after the calls in progress'**
  String usageCreditAvailable(String amount);

  /// No description provided for @usageCreditEmpty.
  ///
  /// In en, this message translates to:
  /// **'Out of credits. Top up your account to keep using paid models.'**
  String get usageCreditEmpty;

  /// No description provided for @usageTokens.
  ///
  /// In en, this message translates to:
  /// **'Tokens'**
  String get usageTokens;

  /// No description provided for @usageCalls.
  ///
  /// In en, this message translates to:
  /// **'Model calls'**
  String get usageCalls;

  /// No description provided for @usageSpent.
  ///
  /// In en, this message translates to:
  /// **'Spent, last {window}'**
  String usageSpent(String window);

  /// No description provided for @usageWindowDay.
  ///
  /// In en, this message translates to:
  /// **'24 h'**
  String get usageWindowDay;

  /// No description provided for @usageWindowDays.
  ///
  /// In en, this message translates to:
  /// **'{count} days'**
  String usageWindowDays(int count);

  /// No description provided for @usageWindowHours.
  ///
  /// In en, this message translates to:
  /// **'{count} h'**
  String usageWindowHours(int count);

  /// No description provided for @usageLimits.
  ///
  /// In en, this message translates to:
  /// **'Limits'**
  String get usageLimits;

  /// No description provided for @usageLimitsOff.
  ///
  /// In en, this message translates to:
  /// **'Spending limits are switched off on this hub.'**
  String get usageLimitsOff;

  /// No description provided for @usageLimitsNone.
  ///
  /// In en, this message translates to:
  /// **'No limit is set for you.'**
  String get usageLimitsNone;

  /// No description provided for @usageLimitsNote.
  ///
  /// In en, this message translates to:
  /// **'Only the limits written for you show here. To allow more, ask whoever runs this Warden.'**
  String get usageLimitsNote;

  /// No description provided for @usageByDay.
  ///
  /// In en, this message translates to:
  /// **'By day'**
  String get usageByDay;

  /// No description provided for @usageNoConversation.
  ///
  /// In en, this message translates to:
  /// **'No conversation yet.'**
  String get usageNoConversation;

  /// No description provided for @usageCallsCount.
  ///
  /// In en, this message translates to:
  /// **'{count, plural, one{1 call} other{{count} calls}}'**
  String usageCallsCount(int count);

  /// No description provided for @usageTokensCount.
  ///
  /// In en, this message translates to:
  /// **'{tokens} tokens'**
  String usageTokensCount(String tokens);

  /// No description provided for @usageTokensCost.
  ///
  /// In en, this message translates to:
  /// **'{tokens} tokens · {cost}'**
  String usageTokensCost(String tokens, String cost);

  /// No description provided for @usageByAgent.
  ///
  /// In en, this message translates to:
  /// **'By agent'**
  String get usageByAgent;

  /// No description provided for @usageNoAgent.
  ///
  /// In en, this message translates to:
  /// **'No agent'**
  String get usageNoAgent;

  /// No description provided for @usageByProvider.
  ///
  /// In en, this message translates to:
  /// **'By provider'**
  String get usageByProvider;

  /// No description provided for @usageDefaultProvider.
  ///
  /// In en, this message translates to:
  /// **'Default provider'**
  String get usageDefaultProvider;

  /// No description provided for @usageByCombo.
  ///
  /// In en, this message translates to:
  /// **'By combo'**
  String get usageByCombo;

  /// No description provided for @usageNoCombo.
  ///
  /// In en, this message translates to:
  /// **'No combo'**
  String get usageNoCombo;

  /// No description provided for @usageSpentByCombo.
  ///
  /// In en, this message translates to:
  /// **'Spent by combo, last {window}'**
  String usageSpentByCombo(String window);

  /// No description provided for @usageUnpriced.
  ///
  /// In en, this message translates to:
  /// **'{cost} (+{count} unpriced)'**
  String usageUnpriced(String cost, int count);

  /// No description provided for @usageBucketLine.
  ///
  /// In en, this message translates to:
  /// **'{tokens} tokens · {calls}'**
  String usageBucketLine(String tokens, String calls);

  /// No description provided for @usageLimitTokens.
  ///
  /// In en, this message translates to:
  /// **'{used} of {max} tokens'**
  String usageLimitTokens(String used, String max);

  /// No description provided for @usageLimitCost.
  ///
  /// In en, this message translates to:
  /// **'{used} of {max}'**
  String usageLimitCost(String used, String max);

  /// No description provided for @usageStateOver.
  ///
  /// In en, this message translates to:
  /// **'Used up'**
  String get usageStateOver;

  /// No description provided for @usageStateWarn.
  ///
  /// In en, this message translates to:
  /// **'Close to the limit'**
  String get usageStateWarn;

  /// No description provided for @usageStateOk.
  ///
  /// In en, this message translates to:
  /// **'Plenty left'**
  String get usageStateOk;

  /// No description provided for @usageLimitTitle.
  ///
  /// In en, this message translates to:
  /// **'{id} · last {window}'**
  String usageLimitTitle(String id, String window);

  /// No description provided for @usageLimitState.
  ///
  /// In en, this message translates to:
  /// **'{state} · {percent}% · {parts}'**
  String usageLimitState(String state, int percent, String parts);

  /// No description provided for @invCancelTitle.
  ///
  /// In en, this message translates to:
  /// **'Cancel invite'**
  String get invCancelTitle;

  /// No description provided for @invCancelMessage.
  ///
  /// In en, this message translates to:
  /// **'Cancels the invite for {name}; its code stops working.'**
  String invCancelMessage(String name);

  /// No description provided for @invFill.
  ///
  /// In en, this message translates to:
  /// **'Fill in who it is for and a validity of at least 1 minute.'**
  String get invFill;

  /// No description provided for @invCreateTitle.
  ///
  /// In en, this message translates to:
  /// **'Create invite'**
  String get invCreateTitle;

  /// No description provided for @invCreateMessage.
  ///
  /// In en, this message translates to:
  /// **'The code works once, for {minutes} minutes.'**
  String invCreateMessage(int minutes);

  /// No description provided for @invRotateTitle.
  ///
  /// In en, this message translates to:
  /// **'Change the pairing key'**
  String get invRotateTitle;

  /// No description provided for @invRotateMessage.
  ///
  /// In en, this message translates to:
  /// **'The old key stops opening the hub\'s administration. Devices already paired keep working.'**
  String get invRotateMessage;

  /// No description provided for @invTitle.
  ///
  /// In en, this message translates to:
  /// **'Devices'**
  String get invTitle;

  /// No description provided for @invHint.
  ///
  /// In en, this message translates to:
  /// **'The pairing key pairs only the hub\'s first device. For each other one, make an invite and give the person the code: they paste it where the key would go. It works once.'**
  String get invHint;

  /// No description provided for @invWho.
  ///
  /// In en, this message translates to:
  /// **'Who is it for'**
  String get invWho;

  /// No description provided for @invWhoHint.
  ///
  /// In en, this message translates to:
  /// **'Ana\'s phone'**
  String get invWhoHint;

  /// No description provided for @invMinutes.
  ///
  /// In en, this message translates to:
  /// **'Valid for (minutes)'**
  String get invMinutes;

  /// No description provided for @invCreate.
  ///
  /// In en, this message translates to:
  /// **'Create invite'**
  String get invCreate;

  /// No description provided for @invCancelled.
  ///
  /// In en, this message translates to:
  /// **'Invite for {name} cancelled.'**
  String invCancelled(String name);

  /// No description provided for @invWaiting.
  ///
  /// In en, this message translates to:
  /// **'Invites waiting'**
  String get invWaiting;

  /// No description provided for @invValidUntil.
  ///
  /// In en, this message translates to:
  /// **'valid until {when}'**
  String invValidUntil(String when);

  /// No description provided for @invMade.
  ///
  /// In en, this message translates to:
  /// **'Invite for {name}, valid until {when}. The code shows only now:'**
  String invMade(String name, String when);

  /// No description provided for @invNewKey.
  ///
  /// In en, this message translates to:
  /// **'New pairing key, shown only now:'**
  String get invNewKey;

  /// No description provided for @memberOrgNone.
  ///
  /// In en, this message translates to:
  /// **'The workspace\'s owner hasn\'t given you access to the organization of the agents.'**
  String get memberOrgNone;

  /// No description provided for @memberOrgEdit.
  ///
  /// In en, this message translates to:
  /// **'Who reports to whom among the workspace\'s agents. The owner let you change the hierarchy: the role and the superior of an agent, a new report, a removal. What the agents may do is the owner\'s.'**
  String get memberOrgEdit;

  /// No description provided for @memberOrgView.
  ///
  /// In en, this message translates to:
  /// **'Who reports to whom among the workspace\'s agents. You can look at the tree; changing it is the owner\'s.'**
  String get memberOrgView;

  /// No description provided for @skillsTitle.
  ///
  /// In en, this message translates to:
  /// **'Skills'**
  String get skillsTitle;

  /// No description provided for @skillsDeleteTitle.
  ///
  /// In en, this message translates to:
  /// **'Delete \'{name}\'?'**
  String skillsDeleteTitle(String name);

  /// No description provided for @skillsRejectTitle.
  ///
  /// In en, this message translates to:
  /// **'Reject \'{name}\'?'**
  String skillsRejectTitle(String name);

  /// No description provided for @skillsDeleteBody.
  ///
  /// In en, this message translates to:
  /// **'This removes the skill file and can\'t be undone.'**
  String get skillsDeleteBody;

  /// No description provided for @skillsReject.
  ///
  /// In en, this message translates to:
  /// **'Reject'**
  String get skillsReject;

  /// No description provided for @skillsAccept.
  ///
  /// In en, this message translates to:
  /// **'Accept'**
  String get skillsAccept;

  /// No description provided for @skillsSeeChanges.
  ///
  /// In en, this message translates to:
  /// **'See what changes in {name}'**
  String skillsSeeChanges(String name);

  /// No description provided for @skillsNoChange.
  ///
  /// In en, this message translates to:
  /// **'No change in the instruction text.'**
  String get skillsNoChange;

  /// No description provided for @skillsSuggested.
  ///
  /// In en, this message translates to:
  /// **'Suggested'**
  String get skillsSuggested;

  /// No description provided for @skillsChangeTo.
  ///
  /// In en, this message translates to:
  /// **'Change to {name}'**
  String skillsChangeTo(String name);

  /// No description provided for @skillsFrom.
  ///
  /// In en, this message translates to:
  /// **'From: {origin}'**
  String skillsFrom(String origin);

  /// No description provided for @skillsNew.
  ///
  /// In en, this message translates to:
  /// **'New skill'**
  String get skillsNew;

  /// No description provided for @skillsEdit.
  ///
  /// In en, this message translates to:
  /// **'Edit skill'**
  String get skillsEdit;

  /// No description provided for @skillsNone.
  ///
  /// In en, this message translates to:
  /// **'No skills yet. Create one here, or ask the AI to write one in chat — they sync with the rest of the vault.'**
  String get skillsNone;

  /// No description provided for @skillsSuggestedTitle.
  ///
  /// In en, this message translates to:
  /// **'Suggested by the AI'**
  String get skillsSuggestedTitle;

  /// No description provided for @skillsSuggestedNote.
  ///
  /// In en, this message translates to:
  /// **'Not active yet — the AI only uses a suggestion after you accept it.'**
  String get skillsSuggestedNote;

  /// No description provided for @skillsYours.
  ///
  /// In en, this message translates to:
  /// **'Your skills'**
  String get skillsYours;

  /// No description provided for @skillsNoDescription.
  ///
  /// In en, this message translates to:
  /// **'(no description)'**
  String get skillsNoDescription;

  /// No description provided for @skillsDeleteTooltip.
  ///
  /// In en, this message translates to:
  /// **'Delete {name}'**
  String skillsDeleteTooltip(String name);

  /// No description provided for @skillsName.
  ///
  /// In en, this message translates to:
  /// **'Name'**
  String get skillsName;

  /// No description provided for @skillsNameHelper.
  ///
  /// In en, this message translates to:
  /// **'Lowercase letters, digits and hyphens — it becomes the file name.'**
  String get skillsNameHelper;

  /// No description provided for @skillsDescription.
  ///
  /// In en, this message translates to:
  /// **'Description'**
  String get skillsDescription;

  /// No description provided for @skillsDescriptionHelper.
  ///
  /// In en, this message translates to:
  /// **'When to use it — the AI reads this every turn.'**
  String get skillsDescriptionHelper;

  /// No description provided for @skillsInstructions.
  ///
  /// In en, this message translates to:
  /// **'Instructions'**
  String get skillsInstructions;

  /// No description provided for @skillsAcceptThis.
  ///
  /// In en, this message translates to:
  /// **'Accept this skill'**
  String get skillsAcceptThis;

  /// No description provided for @skillsAcceptThisHint.
  ///
  /// In en, this message translates to:
  /// **'Off: it stays a suggestion and the AI still ignores it.'**
  String get skillsAcceptThisHint;

  /// No description provided for @skillsSaving.
  ///
  /// In en, this message translates to:
  /// **'Saving…'**
  String get skillsSaving;

  /// No description provided for @diffGapOne.
  ///
  /// In en, this message translates to:
  /// **'… 1 identical line'**
  String get diffGapOne;

  /// No description provided for @diffGapMany.
  ///
  /// In en, this message translates to:
  /// **'… {count} identical lines'**
  String diffGapMany(int count);

  /// No description provided for @syncTitle.
  ///
  /// In en, this message translates to:
  /// **'Warden — Sync'**
  String get syncTitle;

  /// No description provided for @syncNothingToSend.
  ///
  /// In en, this message translates to:
  /// **'Nothing to send — vault and config already match the last Send.'**
  String get syncNothingToSend;

  /// No description provided for @syncShowCode.
  ///
  /// In en, this message translates to:
  /// **'Show pairing code'**
  String get syncShowCode;

  /// No description provided for @syncWaitingDevice.
  ///
  /// In en, this message translates to:
  /// **'Waiting for another device…'**
  String get syncWaitingDevice;

  /// No description provided for @syncPaired.
  ///
  /// In en, this message translates to:
  /// **'Paired successfully!'**
  String get syncPaired;

  /// No description provided for @syncIntro.
  ///
  /// In en, this message translates to:
  /// **'Syncs the vault and config.toml across your devices via Arweave, paid by the TruthID app. Everything is encrypted on this device before it leaves — neither TruthID nor Arweave ever see the content in plain text.'**
  String get syncIntro;

  /// No description provided for @syncSetUp.
  ///
  /// In en, this message translates to:
  /// **'Set up'**
  String get syncSetUp;

  /// No description provided for @syncSetUpHint.
  ///
  /// In en, this message translates to:
  /// **'First device? Initialize a fresh key. Already have another device with sync set up? Ask it to show a pairing code (below) instead.'**
  String get syncSetUpHint;

  /// No description provided for @syncInit.
  ///
  /// In en, this message translates to:
  /// **'Initialize sync on this device'**
  String get syncInit;

  /// No description provided for @syncSendPull.
  ///
  /// In en, this message translates to:
  /// **'Send / Pull'**
  String get syncSendPull;

  /// No description provided for @syncSend.
  ///
  /// In en, this message translates to:
  /// **'Send'**
  String get syncSend;

  /// No description provided for @syncPull.
  ///
  /// In en, this message translates to:
  /// **'Pull'**
  String get syncPull;

  /// No description provided for @syncSent.
  ///
  /// In en, this message translates to:
  /// **'Sent — tx {tx} ({files} file(s)).'**
  String syncSent(String tx, int files);

  /// No description provided for @syncPulled.
  ///
  /// In en, this message translates to:
  /// **'Pull complete — {written} written, {deleted} deleted{config}.'**
  String syncPulled(int written, int deleted, String config);

  /// No description provided for @syncConfigUpdated.
  ///
  /// In en, this message translates to:
  /// **', config.toml updated'**
  String get syncConfigUpdated;

  /// No description provided for @syncPairDevice.
  ///
  /// In en, this message translates to:
  /// **'Pair a device'**
  String get syncPairDevice;

  /// No description provided for @syncShowThisCode.
  ///
  /// In en, this message translates to:
  /// **'Show this code on the device you want to pair.'**
  String get syncShowThisCode;

  /// No description provided for @syncHaveCode.
  ///
  /// In en, this message translates to:
  /// **'Have a code shown on another device? Enter it here.'**
  String get syncHaveCode;

  /// No description provided for @syncCodeLabel.
  ///
  /// In en, this message translates to:
  /// **'Pairing code'**
  String get syncCodeLabel;

  /// No description provided for @syncHostLabel.
  ///
  /// In en, this message translates to:
  /// **'Other device\'s IP (optional)'**
  String get syncHostLabel;

  /// No description provided for @syncHostHint.
  ///
  /// In en, this message translates to:
  /// **'e.g. a Tailscale IP for a hub on a VPS — without it, only the local network is searched'**
  String get syncHostHint;

  /// No description provided for @syncPair.
  ///
  /// In en, this message translates to:
  /// **'Pair'**
  String get syncPair;

  /// No description provided for @syncNever.
  ///
  /// In en, this message translates to:
  /// **'never'**
  String get syncNever;

  /// No description provided for @syncStatus.
  ///
  /// In en, this message translates to:
  /// **'Status'**
  String get syncStatus;

  /// No description provided for @syncStatusPaired.
  ///
  /// In en, this message translates to:
  /// **'Paired'**
  String get syncStatusPaired;

  /// No description provided for @syncYes.
  ///
  /// In en, this message translates to:
  /// **'Yes'**
  String get syncYes;

  /// No description provided for @syncNo.
  ///
  /// In en, this message translates to:
  /// **'No'**
  String get syncNo;

  /// No description provided for @syncOwner.
  ///
  /// In en, this message translates to:
  /// **'Owner address (Arweave)'**
  String get syncOwner;

  /// No description provided for @syncLast.
  ///
  /// In en, this message translates to:
  /// **'Last synced'**
  String get syncLast;

  /// No description provided for @syncPending.
  ///
  /// In en, this message translates to:
  /// **'Pending'**
  String get syncPending;

  /// No description provided for @syncPendingValue.
  ///
  /// In en, this message translates to:
  /// **'{files} file(s){config}'**
  String syncPendingValue(int files, String config);

  /// No description provided for @syncPlusConfig.
  ///
  /// In en, this message translates to:
  /// **' + config.toml'**
  String get syncPlusConfig;

  /// No description provided for @syncScan.
  ///
  /// In en, this message translates to:
  /// **'Scan with the TruthID app to approve and publish ({files} file(s){config}).'**
  String syncScan(int files, String config);

  /// No description provided for @syncWaitingApproval.
  ///
  /// In en, this message translates to:
  /// **'Waiting for approval…'**
  String get syncWaitingApproval;

  /// No description provided for @acctCodeNewTitle.
  ///
  /// In en, this message translates to:
  /// **'Your new recovery code'**
  String get acctCodeNewTitle;

  /// No description provided for @acctCodeTitle.
  ///
  /// In en, this message translates to:
  /// **'Your data is now encrypted'**
  String get acctCodeTitle;

  /// No description provided for @acctCodeBodyNew.
  ///
  /// In en, this message translates to:
  /// **'The old code stopped working. Keep this one: it is the only way to recover everything if you forget your password, or if whoever runs the hub resets it.'**
  String get acctCodeBodyNew;

  /// No description provided for @acctCodeBody.
  ///
  /// In en, this message translates to:
  /// **'Keep this code: it is the only way to recover everything if you forget your password, or if whoever runs the hub resets it.'**
  String get acctCodeBody;

  /// No description provided for @acctCodeOnce.
  ///
  /// In en, this message translates to:
  /// **'It is shown only this once, and nobody on the hub can see it afterwards — not even whoever runs it. Write it down somewhere safe, away from this phone.'**
  String get acctCodeOnce;

  /// No description provided for @acctCodeSaved.
  ///
  /// In en, this message translates to:
  /// **'I saved the code somewhere safe'**
  String get acctCodeSaved;

  /// No description provided for @acctContinue.
  ///
  /// In en, this message translates to:
  /// **'Continue'**
  String get acctContinue;

  /// No description provided for @acctPasswordShort.
  ///
  /// In en, this message translates to:
  /// **'The new password needs at least {min} characters.'**
  String acctPasswordShort(int min);

  /// No description provided for @acctPasswordMismatch.
  ///
  /// In en, this message translates to:
  /// **'The two new passwords don\'t match.'**
  String get acctPasswordMismatch;

  /// No description provided for @acctNeedCode.
  ///
  /// In en, this message translates to:
  /// **'Give your recovery code to keep your data.'**
  String get acctNeedCode;

  /// No description provided for @acctWrongProvisional.
  ///
  /// In en, this message translates to:
  /// **'The provisional password is wrong.'**
  String get acctWrongProvisional;

  /// No description provided for @acctWrongCurrent.
  ///
  /// In en, this message translates to:
  /// **'The current password is wrong.'**
  String get acctWrongCurrent;

  /// No description provided for @acctChooseTitle.
  ///
  /// In en, this message translates to:
  /// **'Choose your password'**
  String get acctChooseTitle;

  /// No description provided for @acctChangeTitle.
  ///
  /// In en, this message translates to:
  /// **'Change your password'**
  String get acctChangeTitle;

  /// No description provided for @acctHi.
  ///
  /// In en, this message translates to:
  /// **'Hi {name}. Before you start, replace the provisional password with one of your own.'**
  String acctHi(String name);

  /// No description provided for @acctProvisional.
  ///
  /// In en, this message translates to:
  /// **'Provisional password'**
  String get acctProvisional;

  /// No description provided for @acctCurrent.
  ///
  /// In en, this message translates to:
  /// **'Current password'**
  String get acctCurrent;

  /// No description provided for @acctRecoveryCode.
  ///
  /// In en, this message translates to:
  /// **'Recovery code'**
  String get acctRecoveryCode;

  /// No description provided for @acctRecoveryHelper.
  ///
  /// In en, this message translates to:
  /// **'Whoever runs the hub reset your password. Your data only comes back with the code you wrote down.'**
  String get acctRecoveryHelper;

  /// No description provided for @acctNewPassword.
  ///
  /// In en, this message translates to:
  /// **'New password'**
  String get acctNewPassword;

  /// No description provided for @acctNewPasswordAgain.
  ///
  /// In en, this message translates to:
  /// **'New password again'**
  String get acctNewPasswordAgain;

  /// No description provided for @acctWrongThat.
  ///
  /// In en, this message translates to:
  /// **'That password is wrong.'**
  String get acctWrongThat;

  /// No description provided for @acctPolicyTitle.
  ///
  /// In en, this message translates to:
  /// **'Who can help recover your data changed'**
  String get acctPolicyTitle;

  /// No description provided for @acctPolicyCompany.
  ///
  /// In en, this message translates to:
  /// **'Whoever runs the hub can recover your data alone, with their recovery key. Every recovery is recorded and you are told.'**
  String get acctPolicyCompany;

  /// No description provided for @acctPolicyConsent.
  ///
  /// In en, this message translates to:
  /// **'Whoever runs the hub can recover your data only together with your recovery code: neither opens it alone.'**
  String get acctPolicyConsent;

  /// No description provided for @acctPolicyToday.
  ///
  /// In en, this message translates to:
  /// **'Today your data follows the previous rule; it only follows this one if you accept.'**
  String get acctPolicyToday;

  /// No description provided for @acctPolicyKeep.
  ///
  /// In en, this message translates to:
  /// **'Keep in mind: whoever controls the machine the hub runs on can always, technically, see what the agent sees while it works. The record and the notice do not protect against someone editing the hub\'s files.'**
  String get acctPolicyKeep;

  /// No description provided for @acctPolicyPassword.
  ///
  /// In en, this message translates to:
  /// **'Your password, to accept'**
  String get acctPolicyPassword;

  /// No description provided for @acctNotNow.
  ///
  /// In en, this message translates to:
  /// **'Not now'**
  String get acctNotNow;

  /// No description provided for @acctAccept.
  ///
  /// In en, this message translates to:
  /// **'Accept'**
  String get acctAccept;

  /// No description provided for @acctRecoveredTitle.
  ///
  /// In en, this message translates to:
  /// **'Your data was recovered'**
  String get acctRecoveredTitle;

  /// No description provided for @acctRecoveredBody.
  ///
  /// In en, this message translates to:
  /// **'Whoever runs the hub recovered your data, with their recovery key, and you signed in just now with the provisional password they gave you.'**
  String get acctRecoveredBody;

  /// No description provided for @acctRecoveredCompany.
  ///
  /// In en, this message translates to:
  /// **'company recovery'**
  String get acctRecoveredCompany;

  /// No description provided for @acctRecoveredConsent.
  ///
  /// In en, this message translates to:
  /// **'recovery with your consent'**
  String get acctRecoveredConsent;

  /// No description provided for @acctRecoveredNotYou.
  ///
  /// In en, this message translates to:
  /// **'If it wasn\'t you who asked, tell whoever runs the hub and change your password.'**
  String get acctRecoveredNotYou;

  /// No description provided for @acctGotIt.
  ///
  /// In en, this message translates to:
  /// **'Got it'**
  String get acctGotIt;

  /// No description provided for @acctNewCodeTitle.
  ///
  /// In en, this message translates to:
  /// **'New recovery code'**
  String get acctNewCodeTitle;

  /// No description provided for @acctNewCodeHint.
  ///
  /// In en, this message translates to:
  /// **'The old code stops working. Type your password to get a new one.'**
  String get acctNewCodeHint;

  /// No description provided for @acctTitle.
  ///
  /// In en, this message translates to:
  /// **'Account'**
  String get acctTitle;

  /// No description provided for @acctEncrypted.
  ///
  /// In en, this message translates to:
  /// **'Your data is encrypted on the hub'**
  String get acctEncrypted;

  /// No description provided for @acctNotEncrypted.
  ///
  /// In en, this message translates to:
  /// **'Your data is not encrypted yet — it is once you sign in with your password'**
  String get acctNotEncrypted;

  /// No description provided for @acctChangePassword.
  ///
  /// In en, this message translates to:
  /// **'Change password'**
  String get acctChangePassword;

  /// No description provided for @acctGetNewCode.
  ///
  /// In en, this message translates to:
  /// **'Get a new recovery code'**
  String get acctGetNewCode;

  /// No description provided for @acctOldStops.
  ///
  /// In en, this message translates to:
  /// **'The old one stops working'**
  String get acctOldStops;

  /// No description provided for @acctLinkTruthid.
  ///
  /// In en, this message translates to:
  /// **'Link my TruthID'**
  String get acctLinkTruthid;

  /// No description provided for @acctTruthidIs.
  ///
  /// In en, this message translates to:
  /// **'TruthID: @{name}'**
  String acctTruthidIs(String name);

  /// No description provided for @acctLinkHint.
  ///
  /// In en, this message translates to:
  /// **'With the invite code whoever runs the hub gave you'**
  String get acctLinkHint;

  /// No description provided for @acctLinkedHint.
  ///
  /// In en, this message translates to:
  /// **'To change it, ask whoever runs the hub for a new invite'**
  String get acctLinkedHint;

  /// No description provided for @acctPassword.
  ///
  /// In en, this message translates to:
  /// **'Password'**
  String get acctPassword;

  /// No description provided for @acctInviteCode.
  ///
  /// In en, this message translates to:
  /// **'Invite code'**
  String get acctInviteCode;

  /// No description provided for @acctUsername.
  ///
  /// In en, this message translates to:
  /// **'My TruthID username'**
  String get acctUsername;

  /// No description provided for @acctLink.
  ///
  /// In en, this message translates to:
  /// **'Link'**
  String get acctLink;

  /// No description provided for @notifChannel.
  ///
  /// In en, this message translates to:
  /// **'Chat messages'**
  String get notifChannel;

  /// No description provided for @notifChannelDesc.
  ///
  /// In en, this message translates to:
  /// **'Notifies you when Warden replies while the app isn\'t in the foreground'**
  String get notifChannelDesc;

  /// No description provided for @notifAgentChannel.
  ///
  /// In en, this message translates to:
  /// **'Agent messages'**
  String get notifAgentChannel;

  /// No description provided for @notifAgentChannelDesc.
  ///
  /// In en, this message translates to:
  /// **'Notifies you when an agent writes to you in its channel'**
  String get notifAgentChannelDesc;

  /// No description provided for @notifError.
  ///
  /// In en, this message translates to:
  /// **'{server} — Error'**
  String notifError(String server);

  /// No description provided for @notifApproval.
  ///
  /// In en, this message translates to:
  /// **'{server} — Approval needed'**
  String notifApproval(String server);

  /// No description provided for @bucketLast7.
  ///
  /// In en, this message translates to:
  /// **'Previous 7 days'**
  String get bucketLast7;

  /// No description provided for @bucketLast30.
  ///
  /// In en, this message translates to:
  /// **'Previous 30 days'**
  String get bucketLast30;

  /// No description provided for @bucketOlder.
  ///
  /// In en, this message translates to:
  /// **'Older'**
  String get bucketOlder;

  /// No description provided for @drawerRenameTitle.
  ///
  /// In en, this message translates to:
  /// **'Rename conversation'**
  String get drawerRenameTitle;

  /// No description provided for @drawerDeleteTitle.
  ///
  /// In en, this message translates to:
  /// **'Delete conversation?'**
  String get drawerDeleteTitle;

  /// No description provided for @drawerDeleteBody.
  ///
  /// In en, this message translates to:
  /// **'\"{title}\" will be deleted from the hub. This can\'t be undone.'**
  String drawerDeleteBody(String title);

  /// No description provided for @drawerUntitled.
  ///
  /// In en, this message translates to:
  /// **'Untitled'**
  String get drawerUntitled;

  /// No description provided for @drawerAnswering.
  ///
  /// In en, this message translates to:
  /// **'Answering…'**
  String get drawerAnswering;

  /// No description provided for @drawerActions.
  ///
  /// In en, this message translates to:
  /// **'Conversation actions'**
  String get drawerActions;

  /// No description provided for @drawerUnpin.
  ///
  /// In en, this message translates to:
  /// **'Unpin'**
  String get drawerUnpin;

  /// No description provided for @drawerPin.
  ///
  /// In en, this message translates to:
  /// **'Pin to the top'**
  String get drawerPin;

  /// No description provided for @drawerUnarchive.
  ///
  /// In en, this message translates to:
  /// **'Unarchive'**
  String get drawerUnarchive;

  /// No description provided for @drawerArchive.
  ///
  /// In en, this message translates to:
  /// **'Archive'**
  String get drawerArchive;

  /// No description provided for @drawerRename.
  ///
  /// In en, this message translates to:
  /// **'Rename'**
  String get drawerRename;

  /// No description provided for @drawerNew.
  ///
  /// In en, this message translates to:
  /// **'New conversation'**
  String get drawerNew;

  /// No description provided for @drawerNone.
  ///
  /// In en, this message translates to:
  /// **'No conversations yet.'**
  String get drawerNone;

  /// No description provided for @drawerPinned.
  ///
  /// In en, this message translates to:
  /// **'Pinned'**
  String get drawerPinned;

  /// No description provided for @drawerArchived.
  ///
  /// In en, this message translates to:
  /// **'Archived ({count})'**
  String drawerArchived(int count);

  /// No description provided for @bannerDisconnected.
  ///
  /// In en, this message translates to:
  /// **'Disconnected'**
  String get bannerDisconnected;

  /// No description provided for @bannerDisconnectedWhy.
  ///
  /// In en, this message translates to:
  /// **'Disconnected: {reason}'**
  String bannerDisconnectedWhy(String reason);

  /// No description provided for @bannerConnecting.
  ///
  /// In en, this message translates to:
  /// **'Connecting…'**
  String get bannerConnecting;

  /// No description provided for @bannerError.
  ///
  /// In en, this message translates to:
  /// **'Connection error: {message}'**
  String bannerError(String message);

  /// No description provided for @liveStop.
  ///
  /// In en, this message translates to:
  /// **'Stop'**
  String get liveStop;

  /// No description provided for @thinking.
  ///
  /// In en, this message translates to:
  /// **'Thinking…'**
  String get thinking;

  /// No description provided for @threadReply.
  ///
  /// In en, this message translates to:
  /// **'Reply in thread'**
  String get threadReply;

  /// No description provided for @threadReplies.
  ///
  /// In en, this message translates to:
  /// **'{count, plural, one{1 reply} other{{count} replies}}'**
  String threadReplies(int count);
}

class _AppLocalizationsDelegate
    extends LocalizationsDelegate<AppLocalizations> {
  const _AppLocalizationsDelegate();

  @override
  Future<AppLocalizations> load(Locale locale) {
    return SynchronousFuture<AppLocalizations>(lookupAppLocalizations(locale));
  }

  @override
  bool isSupported(Locale locale) =>
      <String>['en', 'pt'].contains(locale.languageCode);

  @override
  bool shouldReload(_AppLocalizationsDelegate old) => false;
}

AppLocalizations lookupAppLocalizations(Locale locale) {
  // Lookup logic when only language code is specified.
  switch (locale.languageCode) {
    case 'en':
      return AppLocalizationsEn();
    case 'pt':
      return AppLocalizationsPt();
  }

  throw FlutterError(
    'AppLocalizations.delegate failed to load unsupported locale "$locale". This is likely '
    'an issue with the localizations generation tool. Please file an issue '
    'on GitHub with a reproducible sample app and the gen-l10n configuration '
    'that was used.',
  );
}
