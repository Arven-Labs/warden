import 'package:flutter/services.dart';

import '../l10n/generated/app_localizations.dart';
import '../src/rust/api/arca_pair.dart' as bridge;

/// What the pairing screen needs from the Rust bridge, so a test can stand in for it (tests never load `RustLib`).
abstract class ArcaPairBackend {
  Future<void> start({required String hubUrl, required String deviceToken, required String deviceId});

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
    // Stub: the tests are the contract; the body comes after they are reviewed.
    return newValue;
  }
}

/// The sentence for a failure kind, in the app's language (the Rust text is English and never shown as the main message).
String arcaPairErrorText(AppLocalizations l10n, bridge.ArcaPairErrorKind kind) {
  // Stub.
  return '';
}
