import 'package:flutter/services.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:mobile/services/secure_window.dart';

void main() {
  TestWidgetsFlutterBinding.ensureInitialized();

  const channel = MethodChannel(secureWindowChannelName);
  final messenger = TestDefaultBinaryMessengerBinding.instance.defaultBinaryMessenger;

  tearDown(() => messenger.setMockMethodCallHandler(channel, null));

  group('PlatformSecureWindow', () {
    test('enable and disable reach the Android side as "enable" and "disable" on the secure_window channel', () async {
      final calls = <String>[];
      messenger.setMockMethodCallHandler(channel, (call) async {
        calls.add(call.method);
        return null;
      });
      const window = PlatformSecureWindow();

      await window.enable();
      await window.enable();
      await window.disable();

      expect(secureWindowChannelName, 'com.warden.mobile/secure_window');
      expect(calls, ['enable', 'enable', 'disable'], reason: 'asking again is allowed: the screen re-asks when the app comes back to the front');
    });

    test('where nothing answers the channel (iOS, desktop) it does nothing and does not throw', () async {
      // A handler that throws MissingPluginException is answered with null, which is what a platform with no plugin on the channel does.
      var asked = 0;
      messenger.setMockMethodCallHandler(channel, (call) async {
        asked++;
        throw MissingPluginException();
      });
      // The premise: the channel really does raise MissingPluginException for the caller, so what follows proves it is caught.
      await expectLater(channel.invokeMethod<void>('enable'), throwsA(isA<MissingPluginException>()));
      expect(asked, 1);

      const window = PlatformSecureWindow();
      await window.enable();
      await window.disable();

      expect(asked, 3, reason: 'both calls reached the channel, and both exceptions were caught');
    });

    test('a platform error does not throw either: a window that cannot be protected is no reason to stop the person', () async {
      var asked = 0;
      messenger.setMockMethodCallHandler(channel, (call) async {
        asked++;
        throw PlatformException(code: 'no-window', message: 'English detail');
      });
      const window = PlatformSecureWindow();

      await window.enable();
      await window.disable();

      expect(asked, 2, reason: 'both calls reached the platform, and both errors were swallowed');
    });
  });
}
