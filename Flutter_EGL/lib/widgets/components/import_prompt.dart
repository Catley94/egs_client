import 'package:flutter/material.dart';

import '../../models/fab.dart';
import '../../models/unreal.dart';
import '../../services/api_service.dart';

class ImportParams {
  final String project;
  final String targetSubdir;
  final bool overwrite;
  final String? selectedVersion; // UE major.minor (e.g., '5.6')
  const ImportParams({required this.project, required this.targetSubdir, required this.overwrite, this.selectedVersion});
}

Future<ImportParams?> showImportDialog({
  required BuildContext context,
  required FabAsset asset,
  required ApiService api,
}) async {
  final projectCtrl = TextEditingController(text: '');
  final subdirCtrl = TextEditingController(text: '');
  bool overwrite = false;
  // Build supported UE versions (major.minor)
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

  final result = await showDialog<ImportParams>(
    context: context,
    builder: (ctx) {
      return StatefulBuilder(builder: (ctx, setStateSB) {
        return AlertDialog(
          title: const Text('Import asset to project'),
          content: SingleChildScrollView(
            child: Column(
              mainAxisSize: MainAxisSize.min,
              crossAxisAlignment: CrossAxisAlignment.stretch,
              children: [
                // Project picker dropdown (restores previous behavior)
                FutureBuilder<List<UnrealProjectInfo>>(
                  future: api.listUnrealProjects(),
                  builder: (context, snapshot) {
                    final projects = snapshot.data ?? const <UnrealProjectInfo>[];
                    if (snapshot.connectionState == ConnectionState.waiting) {
                      return const Align(
                        alignment: Alignment.centerLeft,
                        child: Padding(
                          padding: EdgeInsets.only(bottom: 8.0),
                          child: SizedBox(height: 20, width: 20, child: CircularProgressIndicator(strokeWidth: 2)),
                        ),
                      );
                    }
                    if (projects.isEmpty) {
                      return const SizedBox.shrink();
                    }
                    String? selectedPath;
                    return DropdownButtonFormField<String>(
                      isExpanded: true,
                      decoration: const InputDecoration(
                        labelText: 'Select project',
                      ),
                      items: projects.map((p) {
                        final path = (p.uprojectFile.isNotEmpty ? p.uprojectFile : p.path).trim();
                        final version = (p.engineVersion.isNotEmpty) ? p.engineVersion : 'unknown';
                        final label = (p.name.isNotEmpty ? p.name : path);
                        return DropdownMenuItem<String>(
                          value: path,
                          child: Text(
                            '$label (UE $version)',
                            overflow: TextOverflow.ellipsis,
                          ),
                        );
                      }).toList(),
                      onChanged: (val) {
                        selectedPath = (val ?? '').trim();
                        projectCtrl.text = selectedPath ?? '';
                      },
                    );
                  },
                ),
                const SizedBox(height: 8),
                TextField(
                  controller: projectCtrl,
                  decoration: const InputDecoration(
                    labelText: 'Project path',
                    hintText: '/path/to/YourProject.uproject',
                  ),
                ),
                const SizedBox(height: 8),
                if (versionsFull.isNotEmpty) ...[
                  InputDecorator(
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
                  ),
                  const SizedBox(height: 8),
                ],
                TextField(
                  controller: subdirCtrl,
                  decoration: const InputDecoration(
                    labelText: 'Target subdirectory (optional)',
                    hintText: 'e.g., Content/MyAssets',
                  ),
                ),
                StatefulBuilder(
                  builder: (context, setState) {
                    return CheckboxListTile(
                      contentPadding: EdgeInsets.zero,
                      title: const Text('Overwrite if exists'),
                      value: overwrite,
                      onChanged: (v) => setState(() => overwrite = v ?? false),
                      controlAffinity: ListTileControlAffinity.leading,
                    );
                  },
                ),
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
                final project = projectCtrl.text.trim();
                final subdir = subdirCtrl.text.trim();
                if (project.isEmpty) {
                  ScaffoldMessenger.of(ctx).showSnackBar(
                    const SnackBar(content: Text('Please enter your .uproject path')),
                  );
                  return;
                }
                Navigator.of(ctx).pop(ImportParams(project: project, targetSubdir: subdir, overwrite: overwrite, selectedVersion: selectedVersion));
              },
              child: const Text('Import'),
            ),
          ],
        );
      });
    },
  );
  return result;
}
