import 'package:flutter/material.dart';
import 'app_theme.dart';

/// DEPRECATED: AppColors is kept as a thin shim for backward compatibility.
/// All color constants/utilities now live in AppPalette (see app_theme.dart).
class AppColors {
  // Avoid fixed surface/header/border colors; use Theme.of(context).colorScheme.* instead.

  // Tile base colors are forwarded to AppPalette to keep a single source of truth.
  static const Color engineTileBase = AppPalette.engineTileBase;
  static const Color projectTileBase = AppPalette.projectTileBase;
  static const Color fabTileBase = AppPalette.fabTileBase;

  // Utility method delegates to AppPalette.varied
  static Color varied(Color base, int index, {int cycle = 5, double t = 0.15}) =>
      AppPalette.varied(base, index, cycle: cycle, t: t);
}
