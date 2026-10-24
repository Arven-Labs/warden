import 'package:flutter/material.dart';
import 'package:flutter_localizations/flutter_localizations.dart';

import 'l10n/generated/app_localizations.dart';
import 'screens/connection_screen.dart';
import 'services/app_language.dart';
import 'services/app_theme.dart';
import 'services/chat_notifications.dart';
import 'src/rust/frb_generated.dart';
import 'theme/warden_theme.dart';

Future<void> main() async {
  // Phase 7.5 — flutter_local_notifications talks over a MethodChannel, which asserts the binary
  // messenger is ready before any call — required here (and not before) because this is the first
  // plugin in main() that goes through a MethodChannel; RustLib.init() below is plain FFI, not a
  // platform channel, so it never needed this.
  WidgetsFlutterBinding.ensureInitialized();
  // Phase 4.4 — loads the native warden_mobile_bridge library (Rust: sync engine + vault) before
  // any screen can call into it. Must finish before runApp, same reasoning as any other native
  // plugin registration.
  await RustLib.init();
  // P136 — the language the person picked, read before the first screen is drawn.
  await AppLanguage.load();
  // P137 — and the theme (dark until the person picks light).
  await AppTheme.load();
  // Phase 7.5 — registers the Android notification channel before any screen could try to notify.
  await initializeChatNotifications();
  runApp(const WardenApp());
}

class WardenApp extends StatelessWidget {
  const WardenApp({super.key});

  @override
  Widget build(BuildContext context) {
    return ValueListenableBuilder<ThemeMode>(
      valueListenable: AppTheme.mode,
      builder: (context, mode, _) => ValueListenableBuilder<Locale?>(
        valueListenable: AppLanguage.locale,
        builder: (context, picked, _) => _app(picked, mode),
      ),
    );
  }

  Widget _app(Locale? picked, ThemeMode mode) {
    return MaterialApp(
      title: 'Warden',
      locale: picked,
      localizationsDelegates: const [
        AppLocalizations.delegate,
        GlobalMaterialLocalizations.delegate,
        GlobalWidgetsLocalizations.delegate,
        GlobalCupertinoLocalizations.delegate,
      ],
      supportedLocales: AppLocalizations.supportedLocales,
      localeListResolutionCallback: AppLanguage.resolve,
      // P137 — black with purple in the details, the palette of `design/tokens.css` (see `theme/warden_theme.dart`); dark until the
      // person picks light (`AppTheme`), it does not follow the system.
      theme: WardenTheme.light(),
      darkTheme: WardenTheme.dark(),
      themeMode: mode,
      home: const ConnectionScreen(),
    );
  }
}
