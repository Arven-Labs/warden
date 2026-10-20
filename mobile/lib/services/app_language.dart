// P136 — the language of the app: what the person picked (kept on this phone), else the system's (any Portuguese reads as `pt`), else
// English. `AppLanguage.locale` is what `WardenApp` hands to `MaterialApp`; `null` means "follow the system".

import 'dart:ui';

import 'package:flutter/foundation.dart';
import 'package:shared_preferences/shared_preferences.dart';

const supportedLanguageCodes = ['en', 'pt'];

class AppLanguage {
  AppLanguage._();

  static const storageKey = 'app.lang';

  /// The picked language, or `null` while the person has not picked one.
  static final ValueNotifier<Locale?> locale = ValueNotifier<Locale?>(null);

  /// Reads the pick kept on this phone. A value that is not a supported language is ignored.
  static Future<void> load() async {
    try {
      final prefs = await SharedPreferences.getInstance();
      locale.value = parse(prefs.getString(storageKey));
    } catch (_) {
      // The app still opens in the system's language when the phone won't give the pick back.
      locale.value = null;
    }
  }

  /// Picks a language (`null`: follow the system) and keeps the pick.
  static Future<void> pick(Locale? next) async {
    locale.value = next;
    try {
      final prefs = await SharedPreferences.getInstance();
      if (next == null) {
        await prefs.remove(storageKey);
      } else {
        await prefs.setString(storageKey, next.languageCode);
      }
    } catch (_) {
      // The choice holds for this run even when the phone won't keep it.
    }
  }

  static Locale? parse(String? stored) {
    if (stored == null || !supportedLanguageCodes.contains(stored)) return null;
    return Locale(stored);
  }

  /// What `MaterialApp` calls when no language was picked: the first system language that is Portuguese or English, else English.
  static Locale resolve(List<Locale>? system, Iterable<Locale> supported) {
    for (final candidate in system ?? const <Locale>[]) {
      if (candidate.languageCode == 'pt') return const Locale('pt');
      if (candidate.languageCode == 'en') return const Locale('en');
    }
    return const Locale('en');
  }
}
