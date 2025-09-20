import 'package:flutter/material.dart';

/// Centralized app ThemeData for light and dark modes.
/// Uses Material 3 ColorSchemes and keeps accent colors consistent.
class AppTheme {
  AppTheme._();

  static ThemeData light() {
    const background = Color(0xFFF4F6FA); // slightly off-white
    const surface = Color(0xFFFAFBFE); // cards/panels
    const primary = Color(0xFF2563EB); // keep accent blue
    return ThemeData(
      useMaterial3: true,
      brightness: Brightness.light,
      colorScheme: const ColorScheme.light(
        background: background,
        surface: surface,
        primary: primary,
        secondary: primary,
      ).copyWith(
        outlineVariant: const Color(0xFFCDD5DF),
      ),
      scaffoldBackgroundColor: background,
    );
  }

  static ThemeData dark() {
    const background = Color(0xFF0F1115);
    const surface = Color(0xFF12151A);
    const primary = Color(0xFF2E95FF);
    return ThemeData(
      useMaterial3: true,
      brightness: Brightness.dark,
      colorScheme: const ColorScheme.dark(
        background: background,
        surface: surface,
        primary: primary,
        secondary: primary,
      ).copyWith(
        outlineVariant: const Color(0xFF1F2630),
      ),
      scaffoldBackgroundColor: background,
    );
  }
}

/// App-specific palette merged here so all color constants live in one place.
class AppPalette {
  // Tiles base colors (theme-independent accents)
  static const Color engineTileBase = Color(0xFF2563EB); // Blue 600
  static const Color projectTileBase = Color(0xFFF59E0B); // Amber 500
  static const Color fabTileBase = Color(0xFF22C55E); // Green 500

  /// Utility: generate a subtle shade variation for grids
  static Color varied(Color base, int index, {int cycle = 5, double t = 0.15}) {
    final double factor = ((index % cycle) / cycle) * t;
    return Color.lerp(base, Colors.white, factor) ?? base;
  }
}
