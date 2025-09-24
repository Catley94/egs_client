import 'package:flutter/material.dart';
import '../../theme/theme_controller.dart';

/// Shows the Settings dialog for the Library tab.
///
/// This is extracted so the dialog UI is reusable and the main widget stays clean.
Future<void> showLibrarySettingsDialog({
  required BuildContext context,
  required TextEditingController projectsDirCtrl,
  required TextEditingController enginesDirCtrl,
  required TextEditingController cacheDirCtrl,
  required TextEditingController downloadsDirCtrl,
  required bool refreshingFab,
  required VoidCallback onRefreshFabPressed,
  required Future<void> Function() onApplyPressed,
}) async {
  await showDialog<void>(
    context: context,
    builder: (ctx) {
      return AlertDialog(
        title: const Text('Settings'),
        content: SingleChildScrollView(
          child: Column(
            mainAxisSize: MainAxisSize.min,
            children: [
              TextField(
                controller: projectsDirCtrl,
                decoration: const InputDecoration(
                  labelText: 'Projects directory',
                  hintText: '/path/to/Unreal Projects',
                ),
              ),
              const SizedBox(height: 8),
              TextField(
                controller: enginesDirCtrl,
                decoration: const InputDecoration(
                  labelText: 'Engines directory',
                  hintText: '/path/to/UnrealEngines',
                ),
              ),
              const SizedBox(height: 8),
              TextField(
                controller: cacheDirCtrl,
                decoration: const InputDecoration(
                  labelText: 'Cache directory',
                  hintText: './cache',
                ),
              ),
              const SizedBox(height: 8),
              TextField(
                controller: downloadsDirCtrl,
                decoration: const InputDecoration(
                  labelText: 'Downloads directory',
                  hintText: './downloads',
                ),
              ),
              const SizedBox(height: 12),
              // Theme selection
              Row(
                children: [
                  const Icon(Icons.brightness_6_outlined),
                  const SizedBox(width: 8),
                  const Text('Theme'),
                  const SizedBox(width: 12),
                  DropdownButton<ThemeMode>(
                    value: ThemeController.instance.mode.value,
                    items: const [
                      DropdownMenuItem(value: ThemeMode.system, child: Text('System')),
                      DropdownMenuItem(value: ThemeMode.light, child: Text('Light')),
                      DropdownMenuItem(value: ThemeMode.dark, child: Text('Dark')),
                    ],
                    onChanged: (mode) {
                      if (mode != null) ThemeController.instance.set(mode);
                    },
                  ),
                ],
              ),
              const SizedBox(height: 12),
              Align(
                alignment: Alignment.centerLeft,
                child: Row(
                  mainAxisSize: MainAxisSize.min,
                  children: [
                    OutlinedButton.icon(
                      onPressed: refreshingFab ? null : onRefreshFabPressed,
                      icon: refreshingFab
                          ? const SizedBox(width: 16, height: 16, child: CircularProgressIndicator(strokeWidth: 2))
                          : const Icon(Icons.refresh),
                      label: const Text('Refresh Fab List'),
                    ),
                  ],
                ),
              ),
            ],
          ),
        ),
        actions: [
          TextButton(
            onPressed: () => Navigator.of(ctx).pop(),
            child: const Text('Close'),
          ),
          ElevatedButton(
            onPressed: () async {
              await onApplyPressed();
              if (context.mounted) Navigator.of(ctx).pop();
            },
            child: const Text('Apply'),
          )
        ],
      );
    },
  );
}
