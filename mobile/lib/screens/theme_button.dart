import 'package:flutter/material.dart';

import '../l10n/generated/app_localizations.dart';
import '../services/app_theme.dart';

/// The dark/light switch of the app (P137): a moon while it is dark, a sun while it is light. Sits in an AppBar beside the language menu.
class ThemeButton extends StatelessWidget {
  const ThemeButton({super.key});

  @override
  Widget build(BuildContext context) {
    final l10n = AppLocalizations.of(context);
    return ValueListenableBuilder<ThemeMode>(
      valueListenable: AppTheme.mode,
      builder: (context, mode, _) {
        final dark = mode != ThemeMode.light;
        return IconButton(
          key: const Key('theme-button'),
          icon: Icon(dark ? Icons.dark_mode_outlined : Icons.light_mode_outlined),
          tooltip: dark ? l10n.themeToLight : l10n.themeToDark,
          onPressed: AppTheme.toggle,
        );
      },
    );
  }
}
