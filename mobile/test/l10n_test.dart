import 'dart:convert';
import 'dart:io';

import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:mobile/l10n/generated/app_localizations.dart';
import 'package:mobile/l10n/generated/app_localizations_en.dart';
import 'package:mobile/l10n/generated/app_localizations_pt.dart';
import 'package:mobile/screens/language_menu.dart';
import 'package:mobile/services/app_language.dart';
import 'package:shared_preferences/shared_preferences.dart';

import 'helpers/test_app.dart';

Map<String, dynamic> _arb(String name) => jsonDecode(File('lib/l10n/$name.arb').readAsStringSync()) as Map<String, dynamic>;

/// The `{name}`s a message uses (plural bodies included), so the two languages can be compared.
Set<String> _placeholders(String message) => RegExp(r'\{(\w+)').allMatches(message).map((m) => m.group(1)!).toSet();

void main() {
  group('the catalogs', () {
    final en = _arb('app_en');
    final pt = _arb('app_pt');
    final enKeys = en.keys.where((k) => !k.startsWith('@')).toSet();
    final ptKeys = pt.keys.where((k) => !k.startsWith('@')).toSet();

    test('say the same things: no key is missing in either language', () {
      expect(enKeys.difference(ptKeys), isEmpty, reason: 'in English only');
      expect(ptKeys.difference(enKeys), isEmpty, reason: 'in Portuguese only');
    });

    test('use the placeholders each message declares, in both languages', () {
      for (final key in enKeys) {
        final declared = ((en['@$key'] as Map<String, dynamic>?)?['placeholders'] as Map<String, dynamic>?)?.keys.toSet() ?? <String>{};
        // A select/plural option such as `one{...}` is not a placeholder, so only the declared names are compared.
        expect(_placeholders(en[key] as String).intersection(declared), declared, reason: 'English $key');
        expect(_placeholders(pt[key] as String).intersection(declared), declared, reason: 'Portuguese $key');
      }
    });
  });

  group('the language', () {
    test('a picked language wins over the system', () {
      expect(AppLanguage.parse('pt'), const Locale('pt'));
      expect(AppLanguage.parse('en'), const Locale('en'));
    });

    test('a value that is not a language of the app is ignored', () {
      expect(AppLanguage.parse('fr'), isNull);
      expect(AppLanguage.parse(null), isNull);
    });

    test('follows the system when nothing was picked: any Portuguese reads as pt, anything else as English', () {
      const supported = AppLocalizations.supportedLocales;
      expect(AppLanguage.resolve(const [Locale('pt', 'BR')], supported), const Locale('pt'));
      expect(AppLanguage.resolve(const [Locale('pt', 'PT')], supported), const Locale('pt'));
      expect(AppLanguage.resolve(const [Locale('fr'), Locale('pt')], supported), const Locale('pt'));
      expect(AppLanguage.resolve(const [Locale('de')], supported), const Locale('en'));
      expect(AppLanguage.resolve(null, supported), const Locale('en'));
    });

    test('the pick is kept on the phone and read back', () async {
      SharedPreferences.setMockInitialValues({});
      await AppLanguage.pick(const Locale('pt'));
      AppLanguage.locale.value = null;
      await AppLanguage.load();
      expect(AppLanguage.locale.value, const Locale('pt'));
      await AppLanguage.pick(null);
      await AppLanguage.load();
      expect(AppLanguage.locale.value, isNull);
    });
  });

  group('the menu', () {
    setUp(() {
      SharedPreferences.setMockInitialValues({});
      AppLanguage.locale.value = null;
    });

    testWidgets('changes the words on screen without restarting', (tester) async {
      await tester.pumpWidget(ValueListenableBuilder<Locale?>(
        valueListenable: AppLanguage.locale,
        builder: (context, picked, _) => testApp(
          locale: picked ?? const Locale('en'),
          home: Scaffold(
            appBar: AppBar(actions: const [LanguageMenu()]),
            body: Builder(builder: (context) => Text(AppLocalizations.of(context).commonCancel)),
          ),
        ),
      ));
      expect(find.text('Cancel'), findsOneWidget);

      await tester.tap(find.byIcon(Icons.translate));
      await tester.pumpAndSettle();
      await tester.tap(find.text(AppLocalizationsEn().languagePortuguese).last);
      await tester.pumpAndSettle();

      expect(find.text(AppLocalizationsPt().commonCancel), findsOneWidget);
      expect(find.text('Cancel'), findsNothing);
    });
  });
}
