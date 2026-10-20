import 'package:flutter/material.dart';
import 'package:flutter_localizations/flutter_localizations.dart';
import 'package:mobile/l10n/generated/app_localizations.dart';

/// A `MaterialApp` with the app's words (P136), in English unless a test asks for [locale]. Every screen reads its text from
/// `AppLocalizations`, so a test that builds its own app uses this instead of a bare `MaterialApp`.
Widget testApp({required Widget home, Locale locale = const Locale('en')}) => MaterialApp(
      locale: locale,
      localizationsDelegates: const [
        AppLocalizations.delegate,
        GlobalMaterialLocalizations.delegate,
        GlobalWidgetsLocalizations.delegate,
        GlobalCupertinoLocalizations.delegate,
      ],
      supportedLocales: AppLocalizations.supportedLocales,
      home: home,
    );
