import 'package:flutter/material.dart';

import '../l10n/generated/app_localizations.dart';
import '../services/app_language.dart';

/// The picker of the app's language (P136): follow the system, English or Portuguese. Sits in an AppBar, so it can be reached before
/// anybody is connected.
class LanguageMenu extends StatelessWidget {
  const LanguageMenu({super.key});

  @override
  Widget build(BuildContext context) {
    final l10n = AppLocalizations.of(context);
    return ValueListenableBuilder<Locale?>(
      valueListenable: AppLanguage.locale,
      builder: (context, picked, _) => PopupMenuButton<String>(
        icon: const Icon(Icons.translate),
        tooltip: l10n.languageLabel,
        initialValue: picked?.languageCode ?? 'system',
        onSelected: (value) => AppLanguage.pick(value == 'system' ? null : Locale(value)),
        itemBuilder: (context) => [
          PopupMenuItem(value: 'system', child: Text(l10n.languageSystem)),
          PopupMenuItem(value: 'en', child: Text(l10n.languageEnglish)),
          PopupMenuItem(value: 'pt', child: Text(l10n.languagePortuguese)),
        ],
      ),
    );
  }
}
