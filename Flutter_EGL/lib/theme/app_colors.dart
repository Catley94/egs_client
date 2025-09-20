import 'package:flutter/material.dart';

/// Centralized app color definitions for easy theming.
/// Update these values to change the look across the app.
class AppColors {
  // Core surfaces
  static const Color background = Color(0xFF12151A);
  static const Color surface = Color(0xFF171B21);
  static const Color border = Color(0xFF1A2027);

  // Section header text color
  static const Color sectionHeader = Color(0xFFE5EAF0);
  static const Color sectionSubtle = Color(0xFF9AA6B2);

  // Tiles base colors
  // Engine tiles keep a cool blue tone
  static const Color engineTileBase = Color(0xFF2563EB); // Blue 600
  // Project tiles use a warm yellow tone to clearly differ from engine tiles
  static const Color projectTileBase = Color(0xFFF59E0B); // Amber 500

  // Optional: Fab/asset tile base (if needed later)
  static const Color fabTileBase = Color(0xFF22C55E); // Green 500

  // Utility method: generate a subtle shade variation for grids
  static Color varied(Color base, int index, {int cycle = 5, double t = 0.15}) {
    final double factor = ((index % cycle) / cycle) * t;
    return Color.lerp(base, Colors.white, factor) ?? base;
  }
}
