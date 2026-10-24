import 'dart:io';

import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:mobile/main.dart';
import 'package:mobile/services/app_theme.dart';
import 'package:mobile/theme/warden_theme.dart';
import 'package:shared_preferences/shared_preferences.dart';

/// The hex of `--name: #rrggbb;` inside the block that starts at [header] in `design/tokens.css`.
String token(String css, String header, String name) {
  final start = css.indexOf(header);
  expect(start, greaterThanOrEqualTo(0), reason: 'no "$header" block in design/tokens.css');
  final block = css.substring(start, css.indexOf('}', start));
  final match = RegExp('--$name:\\s*(#[0-9a-fA-F]{6})').firstMatch(block);
  expect(match, isNotNull, reason: '--$name is not in the "$header" block');
  return match!.group(1)!.toUpperCase();
}

String hex(Color c) => '#${c.toARGB32().toRadixString(16).substring(2).toUpperCase()}';

void main() {
  group('the palette is the one of design/tokens.css', () {
    final css = File('../design/tokens.css').readAsStringSync();
    final themes = {':root {': WardenPalette.dark, ':root[data-theme="light"] {': WardenPalette.light};
    themes.forEach((header, palette) {
      test(header, () {
        expect(hex(palette.bg), token(css, header, 'color-bg'));
        expect(hex(palette.surface), token(css, header, 'color-surface'));
        expect(hex(palette.surfaceAlt), token(css, header, 'color-surface-alt'));
        expect(hex(palette.border), token(css, header, 'color-border'));
        expect(hex(palette.text), token(css, header, 'color-text'));
        expect(hex(palette.textMuted), token(css, header, 'color-text-muted'));
        expect(hex(palette.accent), token(css, header, 'color-accent'));
        expect(hex(palette.accentDark), token(css, header, 'color-accent-dark'));
        expect(hex(palette.onAccent), token(css, header, 'color-on-accent'));
        expect(hex(palette.danger), token(css, header, 'color-danger'));
      });
    });
  });

  group('the theme of the app', () {
    setUp(() {
      SharedPreferences.setMockInitialValues({});
      AppTheme.mode.value = ThemeMode.dark;
    });

    test('is dark until the person picks light, and ignores anything else that was kept', () async {
      await AppTheme.load();
      expect(AppTheme.mode.value, ThemeMode.dark);
      expect(AppTheme.parse('system'), ThemeMode.dark);
      expect(AppTheme.parse(null), ThemeMode.dark);
      expect(AppTheme.parse('light'), ThemeMode.light);
    });

    test('keeps the pick for the next run', () async {
      await AppTheme.pick(ThemeMode.light);
      AppTheme.mode.value = ThemeMode.dark; // a new run starts dark, then reads what was kept
      await AppTheme.load();
      expect(AppTheme.mode.value, ThemeMode.light);
    });

    testWidgets('the button in the bar flips the theme of the whole app', (tester) async {
      await tester.pumpWidget(const WardenApp());
      await tester.pump();
      ThemeData theme() => Theme.of(tester.element(find.byType(Scaffold).first));
      expect(theme().colorScheme.surface, WardenPalette.dark.surface);

      await tester.tap(find.byKey(const Key('theme-button')));
      await tester.pumpAndSettle();
      expect(AppTheme.mode.value, ThemeMode.light);
      expect(theme().colorScheme.surface, WardenPalette.light.surface);

      await tester.tap(find.byKey(const Key('theme-button')));
      await tester.pumpAndSettle();
      expect(theme().colorScheme.surface, WardenPalette.dark.surface);
    });
  });
}
