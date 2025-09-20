import 'package:flutter/material.dart';

/// Simple global ThemeMode controller using ValueNotifier for app-wide toggling.
class ThemeController {
  ThemeController._();
  static final ThemeController instance = ThemeController._();

  // Default to dark to preserve current look if not changed.
  final ValueNotifier<ThemeMode> mode = ValueNotifier<ThemeMode>(ThemeMode.dark);

  void set(ThemeMode newMode) {
    if (mode.value != newMode) mode.value = newMode;
  }

  void toggle() {
    mode.value = mode.value == ThemeMode.dark ? ThemeMode.light : ThemeMode.dark;
  }
}
