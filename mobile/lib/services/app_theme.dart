// P137 — the theme of the app: dark until the person picks light (it does not follow the system). The pick is kept on this phone, the way
// `AppLanguage` keeps the language; `AppTheme.mode` is what `WardenApp` hands to `MaterialApp`.

import 'package:flutter/material.dart';
import 'package:shared_preferences/shared_preferences.dart';

class AppTheme {
  AppTheme._();

  static const storageKey = 'app.theme';

  static final ValueNotifier<ThemeMode> mode = ValueNotifier<ThemeMode>(ThemeMode.dark);

  /// Reads the pick kept on this phone; anything but `light` is dark.
  static Future<void> load() async {
    try {
      final prefs = await SharedPreferences.getInstance();
      mode.value = parse(prefs.getString(storageKey));
    } catch (_) {
      // The app still opens dark when the phone won't give the pick back.
      mode.value = ThemeMode.dark;
    }
  }

  /// Picks a theme and keeps the pick.
  static Future<void> pick(ThemeMode next) async {
    mode.value = next;
    try {
      final prefs = await SharedPreferences.getInstance();
      await prefs.setString(storageKey, next == ThemeMode.light ? 'light' : 'dark');
    } catch (_) {
      // The choice holds for this run even when the phone won't keep it.
    }
  }

  static ThemeMode parse(String? stored) => stored == 'light' ? ThemeMode.light : ThemeMode.dark;

  /// Flips between dark and light.
  static Future<void> toggle() => pick(mode.value == ThemeMode.light ? ThemeMode.dark : ThemeMode.light);
}
