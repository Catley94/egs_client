import 'package:flutter/material.dart';

/// Shows a dialog to set the Unreal Engine version for a project.
///
/// This encapsulates the previous inline dialog from projects_grid_section.dart
/// into a reusable function.
Future<void> showSetUnrealVersionDialog({
  required BuildContext context,
  required String projectPath,
  required Future<({bool ok, String message})> Function({required String project, required String version}) setProjectVersion,
  required VoidCallback refreshProjects,
}) async {
  final controller = TextEditingController();
  String? errorText;
  await showDialog<void>(
    context: context,
    builder: (ctx) {
      return StatefulBuilder(
        builder: (ctx, setStateSB) => AlertDialog(
          title: const Text('Set Unreal Engine version'),
          content: Column(
            mainAxisSize: MainAxisSize.min,
            children: [
              TextField(
                controller: controller,
                autofocus: true,
                decoration: InputDecoration(
                  hintText: 'e.g., 5.6',
                  labelText: 'UE version (major.minor)',
                  errorText: errorText,
                ),
              ),
              const SizedBox(height: 8),
              const Text('Tip: You can enter 5.6 or UE_5.6. Patch like 5.6.1 is also accepted.'),
            ],
          ),
          actions: [
            TextButton(
              onPressed: () => Navigator.of(ctx).pop(),
              child: const Text('Cancel'),
            ),
            FilledButton(
              onPressed: () async {
                final v = controller.text.trim();
                final re = RegExp(r'^(?:UE_)?\d+\.\d+(?:\.\d+)?$');
                if (!re.hasMatch(v)) {
                  setStateSB(() => errorText = 'Enter a version like 5.6 or UE_5.6');
                  return;
                }
                try {
                  final r = await setProjectVersion(project: projectPath, version: v);
                  if (!context.mounted) return;
                  ScaffoldMessenger.of(context).showSnackBar(
                    SnackBar(content: Text(r.message.isNotEmpty ? r.message : 'UE version updated')),
                  );
                  refreshProjects();
                  Navigator.of(ctx).pop();
                } catch (e) {
                  if (!context.mounted) return;
                  setStateSB(() => errorText = e.toString());
                }
              },
              child: const Text('Save'),
            ),
          ],
        ),
      );
    },
  );
}
