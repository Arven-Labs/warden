import 'package:flutter/services.dart';

import '../l10n/generated/app_localizations.dart';
import '../src/rust/api/arca_pair.dart' as bridge;
import 'vault_paths.dart';

/// What the pairing screen needs from the Rust bridge, so a test can stand in for it (tests never load `RustLib`).
abstract class ArcaPairBackend {
  /// Which side this phone plays: the new device (no vault key yet) or the one that grants (it has one).
  Future<bridge.ArcaPairRole> role();

  /// As the new device.
  Future<void> start({required String hubUrl, required String deviceToken, required String deviceId});

  /// As the device that grants: the steps then are `chooseOffer`, `askOfferCode`, `showFingerprint`, `done`.
  Future<void> startGrant({required String hubUrl, required String deviceToken});

  /// The person picked the offer at [index] of the list `chooseOffer` carried.
  Future<void> pick(int index);

  /// Blocks (off the UI isolate) until the next step.
  Future<bridge.ArcaPairStep> next();

  Future<void> answer(String typed);

  Future<void> cancel();
}

/// Accepts what a person types one character at a time and refuses what arrives in a lump (paste, drop, autofill, a suggestion):
/// copying the fingerprint from one device to the other would defeat the check, which only works if a person compares them.
class TypedOnlyFormatter extends TextInputFormatter {
  @override
  TextEditingValue formatEditUpdate(TextEditingValue oldValue, TextEditingValue newValue) {
    // One character more is typing; the same or fewer is deleting or moving. More than one at once is not a person typing.
    if (newValue.text.length > oldValue.text.length + 1) return oldValue;
    return newValue;
  }
}

/// The sentence for a failure kind, in the app's language (the Rust text is English and never shown as the main message).
String arcaPairErrorText(AppLocalizations l10n, bridge.ArcaPairErrorKind kind) => switch (kind) {
      bridge.ArcaPairErrorKind.busy => l10n.arcaPairErrBusy,
      bridge.ArcaPairErrorKind.noSession => l10n.arcaPairErrNoSession,
      bridge.ArcaPairErrorKind.insecureUrl => l10n.arcaPairErrInsecureUrl,
      bridge.ArcaPairErrorKind.badUrl => l10n.arcaPairErrBadUrl,
      bridge.ArcaPairErrorKind.notAvailable => l10n.arcaPairErrNotAvailable,
      bridge.ArcaPairErrorKind.unauthorized => l10n.arcaPairErrUnauthorized,
      bridge.ArcaPairErrorKind.forbidden => l10n.arcaPairErrForbidden,
      bridge.ArcaPairErrorKind.conflict => l10n.arcaPairErrConflict,
      bridge.ArcaPairErrorKind.fingerprintMismatch => l10n.arcaPairErrFingerprintMismatch,
      bridge.ArcaPairErrorKind.alreadyHasKey => l10n.arcaPairErrAlreadyHasKey,
      bridge.ArcaPairErrorKind.timedOut => l10n.arcaPairErrTimedOut,
      bridge.ArcaPairErrorKind.storeFailed => l10n.arcaPairErrStoreFailed,
      bridge.ArcaPairErrorKind.noOffers => l10n.arcaPairErrNoOffers,
      bridge.ArcaPairErrorKind.noKey => l10n.arcaPairErrNoKey,
      bridge.ArcaPairErrorKind.codeMismatch => l10n.arcaPairErrCodeMismatch,
      bridge.ArcaPairErrorKind.noOffer => l10n.arcaPairErrNoOffer,
      bridge.ArcaPairErrorKind.hub => l10n.arcaPairErrHub,
    };

/// The real backend: the Rust bridge, over the same four sync paths the Sync screen uses.
class BridgeArcaPairBackend implements ArcaPairBackend {
  @override
  Future<bridge.ArcaPairRole> role() async {
    final paths = await VaultPaths.resolve();
    return bridge.arcaPairRole(vaultRoot: paths.vaultRoot, configPath: paths.configPath, secretsPath: paths.secretsPath, manifestPath: paths.manifestPath);
  }

  @override
  Future<void> startGrant({required String hubUrl, required String deviceToken}) async {
    final paths = await VaultPaths.resolve();
    await bridge.arcaPairStartGrant(
      vaultRoot: paths.vaultRoot,
      configPath: paths.configPath,
      secretsPath: paths.secretsPath,
      manifestPath: paths.manifestPath,
      hubUrl: hubUrl,
      deviceToken: deviceToken,
    );
  }

  @override
  Future<void> pick(int index) => bridge.arcaPairPick(index: index);

  @override
  Future<void> start({required String hubUrl, required String deviceToken, required String deviceId}) async {
    final paths = await VaultPaths.resolve();
    await bridge.arcaPairStart(
      vaultRoot: paths.vaultRoot,
      configPath: paths.configPath,
      secretsPath: paths.secretsPath,
      manifestPath: paths.manifestPath,
      hubUrl: hubUrl,
      deviceToken: deviceToken,
      deviceId: deviceId,
    );
  }

  @override
  Future<bridge.ArcaPairStep> next() => bridge.arcaPairNext();

  @override
  Future<void> answer(String typed) => bridge.arcaPairAnswer(typed: typed);

  @override
  Future<void> cancel() => bridge.arcaPairCancel();
}
