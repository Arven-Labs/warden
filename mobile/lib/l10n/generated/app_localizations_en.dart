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
}
