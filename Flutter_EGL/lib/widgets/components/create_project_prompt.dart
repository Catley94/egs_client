import 'package:flutter/material.dart';

import '../../models/fab.dart';

class CreateParams {
  final String? enginePath;
  final String? templateProject;
  final String? assetName;
  final String outputDir;
  final String projectName;
  final String projectType; // 'bp' or 'cpp'
  final bool dryRun;
  final String? selectedVersion; // UE major.minor
  const CreateParams({
    required this.enginePath,
    required this.templateProject,
    required this.assetName,
    required this.outputDir,
    required this.projectName,
    required this.projectType,
    required this.dryRun,
    this.selectedVersion,
  });
}

Future<CreateParams?> showCreateProjectDialog({
  required BuildContext context,
  required FabAsset asset,
}) async {
  final enginePathCtrl = TextEditingController(text: '');
  final templateCtrl = TextEditingController(text: '');
  final outputDirCtrl = TextEditingController(text: '\$HOME/Documents/Unreal Projects');
  final projectNameCtrl = TextEditingController(text: '');
  String projectType = 'bp';
  bool dryRun = false;
  final assetNameCtrl = TextEditingController(text: asset.title.isNotEmpty ? asset.title : asset.assetId);

  // Build supported UE versions (major.minor) from this asset
  final engines = <String>{};
  for (final pv in asset.projectVersions) {
    for (final ev in pv.engineVersions) {
      final parts = ev.split('_');
      if (parts.length > 1) engines.add(parts[1]);
    }
  }
  int score(String v) {
    final parts = v.split('.');
    final major = int.tryParse(parts.isNotEmpty ? parts[0] : '0') ?? 0;
    final minor = int.tryParse(parts.length > 1 ? parts[1] : '0') ?? 0;
    return major * 100 + minor;
  }
  final versionsFull = engines.toList()..sort((a, b) => score(b).compareTo(score(a)));
  String? selectedVersion = versionsFull.isNotEmpty ? versionsFull.first : null;

  final result = await showDialog<CreateParams>(
    context: context,
    builder: (ctx) {
      return AlertDialog(
        title: const Text('Create Unreal Project'),
        content: SingleChildScrollView(
          child: Column(
            mainAxisSize: MainAxisSize.min,
            children: [
              TextField(
                controller: projectNameCtrl,
                decoration: const InputDecoration(
                  labelText: 'Project name',
                  hintText: 'e.g., MyNewGame',
                ),
              ),
              const SizedBox(height: 8),
              TextField(
                controller: outputDirCtrl,
                decoration: const InputDecoration(
                  labelText: 'Output folder',
                  hintText: "e.g., \$HOME/Documents/Unreal Projects",
                ),
              ),
              const SizedBox(height: 16),
              // TextField(
              //   controller: assetNameCtrl,
              //   decoration: const InputDecoration(
              //     labelText: 'Asset name (optional if template path used)',
              //     hintText: 'e.g., Stack O Bot',
              //   ),
              // ),
              // const SizedBox(height: 8),
              // TextField(
              //   controller: templateCtrl,
              //   decoration: const InputDecoration(
              //     labelText: 'Template .uproject path (optional)',
              //     hintText: '/path/to/Sample/Sample.uproject',
              //   ),
              // ),
              // const SizedBox(height: 8),
              // TextField(
              //   controller: enginePathCtrl,
              //   decoration: const InputDecoration(
              //     labelText: 'Engine path (optional)',
              //     hintText: '/path/to/Unreal/UE_5.xx',
              //   ),
              // ),
              const SizedBox(height: 8),
              if (versionsFull.isNotEmpty) ...[
                // UE version selector used to drive download selection and EngineAssociation
                StatefulBuilder(
                  builder: (context, setStateSB) {
                    return InputDecorator(
                      decoration: const InputDecoration(
                        labelText: 'UE Version',
                        border: OutlineInputBorder(gapPadding: 0),
                        isDense: true,
                        contentPadding: EdgeInsets.symmetric(horizontal: 8, vertical: 8),
                      ),
                      child: DropdownButtonHideUnderline(
                        child: DropdownButton<String>(
                          value: selectedVersion,
                          isExpanded: true,
                          isDense: true,
                          items: versionsFull
                              .map((v) => DropdownMenuItem<String>(
                                    value: v,
                                    child: Text(v),
                                  ))
                              .toList(),
                          onChanged: (v) => setStateSB(() => selectedVersion = v),
                        ),
                      ),
                    );
                  },
                ),
                const SizedBox(height: 8),
              ],
              Row(
                children: [
                  // const Text('Project type:'),
                  // const SizedBox(width: 12),
                  // DropdownButton<String>(
                  //   value: projectType,
                  //   items: const [
                  //     DropdownMenuItem(value: 'bp', child: Text('Blueprint (bp)')),
                  //     DropdownMenuItem(value: 'cpp', child: Text('C++ (cpp)')),
                  //   ],
                  //   onChanged: (v) {
                  //     if (v != null) {
                  //       projectType = v;
                  //       // refresh local state inside dialog
                  //       (ctx as Element).markNeedsBuild();
                  //     }
                  //   },
                  // ),
                ],
              ),
              // StatefulBuilder(
              //   builder: (context, setState) {
              //     return CheckboxListTile(
              //       contentPadding: EdgeInsets.zero,
              //       title: const Text('Dry run (do not actually create)'),
              //       value: dryRun,
              //       onChanged: (v) => setState(() => dryRun = v ?? false),
              //       controlAffinity: ListTileControlAffinity.leading,
              //     );
              //   },
              // ),
            ],
          ),
        ),
        actions: [
          TextButton(
            onPressed: () => Navigator.of(ctx).pop(),
            child: const Text('Cancel'),
          ),
          FilledButton(
            onPressed: () {
              final projectName = projectNameCtrl.text.trim();
              final outputDir = outputDirCtrl.text.trim();
              final assetName = assetNameCtrl.text.trim();
              final template = templateCtrl.text.trim();
              final enginePath = enginePathCtrl.text.trim();
              if (projectName.isEmpty || outputDir.isEmpty) {
                ScaffoldMessenger.of(ctx).showSnackBar(
                  const SnackBar(content: Text('Please enter project name and output folder')),
                );
                return;
              }
              Navigator.of(ctx).pop(CreateParams(
                enginePath: enginePath.isEmpty ? null : enginePath,
                templateProject: template.isEmpty ? null : template,
                assetName: assetName.isEmpty ? null : assetName,
                outputDir: outputDir,
                projectName: projectName,
                projectType: projectType,
                dryRun: dryRun,
                selectedVersion: selectedVersion,
              ));
            },
            child: const Text('Create'),
          ),
        ],
      );
    },
  );

  return result;
}
