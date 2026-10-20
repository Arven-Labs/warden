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
