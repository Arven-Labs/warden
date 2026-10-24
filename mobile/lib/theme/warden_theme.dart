// P137 — the look of the app: black with purple in the details. The values are the ones of `design/tokens.css` (the palette every surface
// shares: the site, the web page, the desktop app and the extension), written out here because Dart can't import a CSS file;
// `test/theme/warden_theme_test.dart` reads `design/tokens.css` and fails when a hex here drifts from it.

import 'package:flutter/material.dart';

/// The colors of one theme, by the names `design/tokens.css` gives them.
class WardenPalette {
  const WardenPalette({
    required this.bg,
    required this.surface,
    required this.surfaceAlt,
    required this.border,
    required this.text,
    required this.textMuted,
    required this.accent,
    required this.accentDark,
    required this.onAccent,
    required this.danger,
    required this.brightness,
  });

  final Color bg;
  final Color surface;
  final Color surfaceAlt;
  final Color border;
  final Color text;
  final Color textMuted;
  final Color accent;
  final Color accentDark;
  final Color onAccent;
  final Color danger;
  final Brightness brightness;

  static const dark = WardenPalette(
    bg: Color(0xFF07060B),
    surface: Color(0xFF100D1A),
    surfaceAlt: Color(0xFF171326),
    border: Color(0xFF272043),
    text: Color(0xFFEDE9F7),
    textMuted: Color(0xFFA599C2),
    accent: Color(0xFFA78BFA),
    accentDark: Color(0xFF8B5CF6),
    onAccent: Color(0xFF12082B),
    danger: Color(0xFFF87171),
    brightness: Brightness.dark,
  );

  static const light = WardenPalette(
    bg: Color(0xFFFBF9FF),
    surface: Color(0xFFFFFFFF),
    surfaceAlt: Color(0xFFF1EAFB),
    border: Color(0xFFE3D9F7),
    text: Color(0xFF1B1330),
    textMuted: Color(0xFF6B6280),
    accent: Color(0xFF7C3AED),
    accentDark: Color(0xFF6D28D9),
    onAccent: Color(0xFFFFFFFF),
    danger: Color(0xFFDC2626),
    brightness: Brightness.light,
  );
}

class WardenTheme {
  WardenTheme._();

  static ThemeData dark() => _build(WardenPalette.dark);
  static ThemeData light() => _build(WardenPalette.light);

  static ThemeData _build(WardenPalette p) {
    final scheme = ColorScheme(
      brightness: p.brightness,
      primary: p.accent,
      onPrimary: p.onAccent,
      primaryContainer: p.surfaceAlt,
      onPrimaryContainer: p.text,
      secondary: p.accentDark,
      onSecondary: p.brightness == Brightness.dark ? p.onAccent : p.onAccent,
      secondaryContainer: p.surfaceAlt,
      onSecondaryContainer: p.text,
      tertiary: p.accent,
      onTertiary: p.onAccent,
      error: p.danger,
      onError: p.brightness == Brightness.dark ? p.onAccent : Colors.white,
      surface: p.surface,
      onSurface: p.text,
      onSurfaceVariant: p.textMuted,
      surfaceContainerLowest: p.bg,
      surfaceContainerLow: p.surface,
      surfaceContainer: p.surfaceAlt,
      surfaceContainerHigh: p.surfaceAlt,
      surfaceContainerHighest: p.surfaceAlt,
      outline: p.border,
      outlineVariant: p.border,
    );
    final line = BorderSide(color: p.border);
    // The text styles come from the platform's typography (Roboto on Android), so a style set here keeps its font.
    final base = ThemeData(useMaterial3: true, brightness: p.brightness);
    return base.copyWith(
      colorScheme: scheme,
      scaffoldBackgroundColor: p.bg,
      canvasColor: p.bg,
      dividerColor: p.border,
      appBarTheme: AppBarTheme(
        backgroundColor: p.surface,
        foregroundColor: p.text,
        surfaceTintColor: Colors.transparent,
        elevation: 0,
        scrolledUnderElevation: 0,
        shape: Border(bottom: line),
        titleTextStyle: base.textTheme.titleLarge?.copyWith(color: p.text, fontSize: 18, fontWeight: FontWeight.w600),
      ),
      cardTheme: CardThemeData(color: p.surfaceAlt, surfaceTintColor: Colors.transparent, elevation: 0, shape: RoundedRectangleBorder(borderRadius: BorderRadius.circular(12), side: line)),
      dialogTheme: DialogThemeData(backgroundColor: p.surfaceAlt, surfaceTintColor: Colors.transparent),
      popupMenuTheme: PopupMenuThemeData(color: p.surfaceAlt, surfaceTintColor: Colors.transparent),
      bottomSheetTheme: BottomSheetThemeData(backgroundColor: p.surfaceAlt, surfaceTintColor: Colors.transparent),
      inputDecorationTheme: InputDecorationTheme(
        enabledBorder: UnderlineInputBorder(borderSide: line),
        focusedBorder: UnderlineInputBorder(borderSide: BorderSide(color: p.accent, width: 2)),
      ),
      filledButtonTheme: FilledButtonThemeData(style: FilledButton.styleFrom(backgroundColor: p.accent, foregroundColor: p.onAccent)),
      progressIndicatorTheme: ProgressIndicatorThemeData(color: p.accent),
      switchTheme: SwitchThemeData(
        thumbColor: WidgetStateProperty.resolveWith((s) => s.contains(WidgetState.selected) ? p.onAccent : p.textMuted),
        trackColor: WidgetStateProperty.resolveWith((s) => s.contains(WidgetState.selected) ? p.accent : p.surfaceAlt),
        trackOutlineColor: WidgetStateProperty.resolveWith((s) => s.contains(WidgetState.selected) ? p.accent : p.border),
      ),
      dividerTheme: DividerThemeData(color: p.border),
      iconButtonTheme: IconButtonThemeData(style: IconButton.styleFrom(visualDensity: VisualDensity.compact)),
    );
  }
}
