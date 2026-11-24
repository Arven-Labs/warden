import 'package:flutter/services.dart';

/// Name of the platform channel `MainActivity` answers on (Android only).
const secureWindowChannelName = 'com.warden.mobile/secure_window';

/// Keeps the app's window out of screenshots, screen recordings and the recent-apps thumbnail while a secret is on the screen.
///
/// A seam for the screens that show a secret (the 24 words of the new key, P-49): they take a [SecureWindow], and a test hands them a
/// fake. Both calls are safe to repeat and never throw: a window that cannot be protected is no reason to stop the person.
abstract class SecureWindow {
  /// Asks the platform to block capture of this window. Idempotent.
  Future<void> enable();

  /// Lets the window be captured again. Idempotent.
  Future<void> disable();
}

/// Talks to Android's `FLAG_SECURE` over [secureWindowChannelName]. On iOS and the desktop nothing answers the channel, so it does nothing.
class PlatformSecureWindow implements SecureWindow {
  const PlatformSecureWindow();

  static const _channel = MethodChannel(secureWindowChannelName);

  @override
  Future<void> enable() => _call('enable');

  @override
  Future<void> disable() => _call('disable');

  Future<void> _call(String method) async {
    try {
      await _channel.invokeMethod<void>(method);
    } on MissingPluginException {
      // iOS and the desktop have no such flag: nothing to protect with, nothing to undo.
    } on PlatformException {
      // Android could not change the window. The screen still carries its own warning against photographing the words.
    }
  }
}
