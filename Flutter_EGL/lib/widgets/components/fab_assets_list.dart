import 'dart:async';
import 'dart:math';

import 'package:flutter/material.dart';
import 'package:url_launcher/url_launcher.dart';

import '../../models/fab.dart';
import '../../services/api_service.dart';
import '../fab_library_item.dart';
import 'fab_asset_overlay.dart';
import 'job_progress_dialog.dart';
import '../../models/unreal.dart';

class FabAssetsList extends StatefulWidget {
  final VoidCallback? onLoadMore;
  final List<FabAsset> assets;
  final int crossAxisCount;
  final double spacing;
  final VoidCallback? onProjectsChanged;
  final VoidCallback? onFabListChanged;
  const FabAssetsList({super.key, required this.assets, required this.crossAxisCount, required this.spacing, this.onLoadMore, this.onProjectsChanged, this.onFabListChanged});

  @override
  State<FabAssetsList> createState() => FabAssetsListState();
}

class _ImportParams {
  final String project;
  final String targetSubdir;
  final bool overwrite;
  const _ImportParams({required this.project, required this.targetSubdir, required this.overwrite});
}

class _CreateParams {
  final String? enginePath;
  final String? templateProject;
  final String? assetName;
  final String outputDir;
  final String projectName;
  final String projectType; // 'bp' or 'cpp'
  final bool dryRun;
  const _CreateParams({
    required this.enginePath,
    required this.templateProject,
    required this.assetName,
    required this.outputDir,
    required this.projectName,
    required this.projectType,
    required this.dryRun,
  });
}

class FabAssetsListState extends State<FabAssetsList> {
  // Cached highest installed UE version string like '5.6' or '4.27'
  String? _maxInstalledUe;
  // Cached set of installed major.minor versions, e.g., {'5.6','5.5'}
  Set<String>? _installedMmSet;

  @override
  void initState() {
    super.initState();
    _loadInstalledMm();
  }

  // Parse UE version tokens from strings like 'UE_5.6', '5.6', '5.6.1'. Returns [major, minor, patch].
  List<int>? _parseUeVersion(String v) {
    if (v.isEmpty) return null;
    var s = v.trim();
    if (s.startsWith('UE_')) s = s.substring(3);
    // Keep only digits and dots
    final m = RegExp(r'^(\d+)(?:\.(\d+))?(?:\.(\d+))?$').firstMatch(s);
    if (m == null) return null;
    int p(String? x) => int.tryParse(x ?? '') ?? 0;
    return [p(m.group(1)), p(m.group(2)), p(m.group(3))];
  }

  String _mmFromVersion(String v) {
    final pr = _parseUeVersion(v) ?? [0, 0, 0];
    return '${pr[0]}.${pr[1]}';
  }

  Future<void> _loadInstalledMm() async {
    try {
      final engines = await _api.listUnrealEngines();
      final set = <String>{};
      for (final e in engines) {
        final v = e.version.trim();
        if (v.isEmpty) continue;
        set.add(_mmFromVersion(v));
      }
      if (mounted) setState(() => _installedMmSet = set);
    } catch (_) {
      // ignore
    }
  }

  int _cmpUeVersions(String a, String b) {
    final pa = _parseUeVersion(a) ?? [0, 0, 0];
    final pb = _parseUeVersion(b) ?? [0, 0, 0];
    for (var i = 0; i < 3; i++) {
      if (pa[i] != pb[i]) return pa[i].compareTo(pb[i]);
    }
    return 0;
  }

  // Returns highest supported engine version for the given asset (e.g., '5.6').
  String? _maxSupportedForAsset(FabAsset a) {
    String? best;
    for (final pv in a.projectVersions) {
      for (final ev in pv.engineVersions) {
        final token = ev.trim();
        if (token.isEmpty) continue;
        final normalized = token.startsWith('UE_') ? token.substring(3) : token;
        if (best == null || _cmpUeVersions(normalized, best) > 0) {
          best = normalized;
        }
      }
    }
    return best;
  }

  // Returns set of supported major.minor strings, e.g., {'5.6','5.5','4.27'}
  Set<String> _supportedMajorMinorSet(FabAsset a) {
    final out = <String>{};
    String mm(List<int> p) => p.length >= 2 ? '${p[0]}.${p[1]}' : '${p[0]}.0';
    for (final pv in a.projectVersions) {
      for (final ev in pv.engineVersions) {
        final norm = ev.startsWith('UE_') ? ev.substring(3) : ev;
        final pr = _parseUeVersion(norm);
        if (pr != null) out.add(mm(pr));
      }
    }
    return out;
  }

  Future<String?> _getHighestInstalledEngineVersion() async {
    if (_maxInstalledUe != null) return _maxInstalledUe;
    try {
      final engines = await _api.listUnrealEngines();
      String? best;
      for (final e in engines) {
        final v = e.version.trim();
        if (v.isEmpty) continue;
        if (best == null || _cmpUeVersions(v, best) > 0) best = v;
      }
      _maxInstalledUe = best;
      return best;
    } catch (_) {
      return null;
    }
  }

  Future<bool> _projectHasSupportInstalled(FabAsset a) async {
    if (_installedMmSet == null) {
      await _loadInstalledMm();
    }
    final installed = _installedMmSet ?? <String>{};
    final supported = _supportedMajorMinorSet(a);
    return installed.intersection(supported).isNotEmpty;
  }

  String _makeJobId() {
    final r = Random();
    final ts = DateTime.now().millisecondsSinceEpoch;
    final rand = r.nextInt(0x7FFFFFFF);
    return 'job_${ts.toRadixString(16)}_${rand.toRadixString(16)}';
  }

  // Kept for compatibility; no-op in pagination mode
  void increaseVisible(int by, int total) {
    // no-op
  }

  static const int _pageSize = 40; // max assets per page
  int _page = 0;

  final ApiService _api = ApiService();
  final Set<int> _busy = <int>{};

  String? _pickThumbnailUrl(FabAsset a) {
    if (a.images.isEmpty) return null;
    for (final img in a.images) {
      final t = (img.type ?? '').toLowerCase();
      if (t.contains('thumb')) return img.url;
    }
    return a.images.first.url;
  }

  Future<void> _launchExternal(String url) async {
    // Use root navigator context for SnackBars to avoid deactivated widget ancestor issues
    final BuildContext navCtx = Navigator.of(context, rootNavigator: true).context;
    final uri = Uri.tryParse(url);
    if (uri == null) {
      if (!mounted) return;
      ScaffoldMessenger.of(navCtx).showSnackBar(
        const SnackBar(content: Text('Invalid URL')),
      );
      return;
    }
    try {
      final ok = await launchUrl(uri, mode: LaunchMode.externalApplication);
      if (!ok && mounted) {
        ScaffoldMessenger.of(navCtx).showSnackBar(
          const SnackBar(content: Text('Could not open link')),
        );
      }
    } catch (e) {
      if (mounted) {
        ScaffoldMessenger.of(navCtx).showSnackBar(
          SnackBar(content: Text('Failed to launch: $e')),
        );
      }
    }
  }

  Future<void> _showAssetGalleryDialog(BuildContext context, FabAsset a) async {
    // Delegate to the extracted overlay component
    await showFabAssetOverlayDialog(
      context: context,
      asset: a,
      api: _api,
      promptCreateProject: (ctx, asset) => _promptCreateProject(ctx, asset),
      promptImport: (ctx, asset) => _promptImport(ctx, asset),
      showJobProgressDialog: ({required String jobId, required String title}) {
        final navCtx = Navigator.of(context, rootNavigator: true).context;
        return showJobProgressOverlayDialog(context: navCtx, api: _api, jobId: jobId, title: title);
      },
      makeJobId: () => _makeJobId(),
      launchExternal: (url) => _launchExternal(url),
      onProjectsChanged: widget.onProjectsChanged,
      onFabListChanged: widget.onFabListChanged,
    );
  }

  @override
  Widget build(BuildContext context) {
    final spacing = widget.spacing;

    final totalAssets = widget.assets.length;
    final totalPages = (totalAssets / _pageSize).ceil().clamp(1, 1000000);
    final start = (_page * _pageSize).clamp(0, totalAssets);
    final end = min(start + _pageSize, totalAssets);

    final visible = widget.assets.sublist(start, end);

    final grid = Padding(
      padding: const EdgeInsets.all(16),
      child: GridView.builder(
        shrinkWrap: true,
        physics: const NeverScrollableScrollPhysics(),
        itemCount: visible.length,
        gridDelegate: SliverGridDelegateWithFixedCrossAxisCount(
          crossAxisCount: widget.crossAxisCount,
          crossAxisSpacing: spacing,
          mainAxisSpacing: spacing,
          // Wider aspect: each card is ~ 0.23 in height compared to width, as it's a row layout
          // Slightly taller cards to avoid vertical overflow of title/button/label
          childAspectRatio: 2.8,
        ),
        itemBuilder: (context, index) {
          final a = visible[index];
          final globalIndex = start + index;
          final busy = _busy.contains(globalIndex);
          final name = a.title.isNotEmpty ? a.title : a.assetId;
          final hasComplete = a.isCompleteProject;
          final thumb = _pickThumbnailUrl(a);
          final sizeLabel = a.shortEngineLabel.isNotEmpty ? a.shortEngineLabel : (a.sizeLabel.isNotEmpty ? a.sizeLabel : '');
          final maxSupported = _maxSupportedForAsset(a);

          return FabLibraryItem(
            title: name,
            sizeLabel: sizeLabel,
            isCompleteProject: hasComplete,
            isBusy: busy,
            downloaded: a.anyDownloaded,
            thumbnailUrl: thumb,
            onTap: () async {
              await _showAssetGalleryDialog(context, a);
            },
            useWarningStyle: false,
            onPrimaryPressed: busy
                ? null
                : () async {
                    // Primary action: if complete project => create project; else => import asset
                    if (hasComplete) {
                      final params = await _promptCreateProject(context, a);
                      if (params == null) return;
                      setState(() => _busy.add(globalIndex));
                      try {
                        final jobId = _makeJobId();
                        final navCtx = Navigator.of(context, rootNavigator: true).context;
                        final dlg = showJobProgressOverlayDialog(context: navCtx, api: _api, jobId: jobId, title: 'Creating project...');
                        final res = await _api.createUnrealProject(
                          enginePath: params.enginePath,
                          templateProject: params.templateProject,
                          assetName: params.assetName,
                          outputDir: params.outputDir,
                          projectName: params.projectName,
                          projectType: params.projectType,
                          dryRun: params.dryRun,
                          jobId: jobId,
                        );
                        if (mounted) {
                          final nav = Navigator.of(context, rootNavigator: true);
                          if (nav.canPop()) nav.pop();
                        }
                        await dlg.catchError((_ ){});
                        if (!mounted) return;
                        final ok = res.success;
                        final msg = res.message.isNotEmpty ? res.message : (ok ? 'OK' : 'Failed');
                        ScaffoldMessenger.of(navCtx).showSnackBar(
                          SnackBar(content: Text(msg)),
                        );
                        if (ok && !params.dryRun) {
                          // Notify parent to refresh projects list
                          widget.onProjectsChanged?.call();
                          // Also refresh Fab list to update downloaded indicators
                          widget.onFabListChanged?.call();
                        }
                      } catch (e) {
                        if (!mounted) return;
                        final navCtx = Navigator.of(context, rootNavigator: true).context;
                        ScaffoldMessenger.of(navCtx).showSnackBar(
                          SnackBar(content: Text('Failed to create project: $e')),
                        );
                      } finally {
                        if (mounted) setState(() => _busy.remove(globalIndex));
                      }
                      return;
                    }
                    final params = await _promptImport(context, a);
                    if (params == null) return;
                    // Check for Unreal Engine version mismatch before proceeding
                    try {
                      final targetVersion = await _getHighestInstalledEngineVersion();
                      final supported = _supportedMajorMinorSet(a);
                      String? targetMM;
                      if (targetVersion != null && targetVersion.trim().isNotEmpty) {
                        final pv = _parseUeVersion(targetVersion.trim());
                        if (pv != null) {
                          targetMM = '${pv[0]}.${pv[1]}';
                        }
                      }
                      final mismatch = targetMM != null && !supported.contains(targetMM);
                      if (mismatch && mounted) {
                        final proceed = await showDialog<bool>(
                          context: context,
                          builder: (ctx) => AlertDialog(
                            title: const Text('Unsupported Unreal Engine version'),
                            content: const Text('Warning, you are about to import an asset into an unsupported version of Unreal Engine, this may cause issues, are you sure you want to go ahead?'),
                            actions: [
                              TextButton(onPressed: () => Navigator.of(ctx).pop(false), child: const Text('Cancel')),
                              FilledButton(onPressed: () => Navigator.of(ctx).pop(true), child: const Text('Proceed')),
                            ],
                          ),
                        );
                        if (proceed != true) {
                          return; // user cancelled
                        }
                      }
                    } catch (_) {
                      // If any error occurs during version check, ignore and proceed
                    }
                    setState(() => _busy.add(globalIndex));
                    try {
                      final name = a.title.isNotEmpty ? a.title : a.assetId;
                      final jobId = _makeJobId();
                      final navCtx = Navigator.of(context, rootNavigator: true).context;
                      final dlg = showJobProgressOverlayDialog(context: navCtx, api: _api, jobId: jobId, title: 'Importing asset...');
                      final result = await _api.importAsset(
                        assetName: name,
                        project: params.project,
                        targetSubdir: params.targetSubdir.isEmpty ? null : params.targetSubdir,
                        overwrite: params.overwrite,
                        jobId: jobId,
                      );
                      // Close progress dialog if still open
                      if (mounted) {
                        final nav = Navigator.of(context, rootNavigator: true);
                        if (nav.canPop()) nav.pop();
                      }
                      await dlg.catchError((_ ){});
                      if (!mounted) return;
                      final msg = result.message.isNotEmpty ? result.message : (result.success ? 'Import started' : 'Import failed');
                      ScaffoldMessenger.of(navCtx).showSnackBar(
                        SnackBar(content: Text(msg)),
                      );
                      if (result.success) {
                        // Refresh Fab list so the downloaded indicator updates
                        widget.onFabListChanged?.call();
                      }
                    } catch (e) {
                      if (!mounted) return;
                      final navCtx = Navigator.of(context, rootNavigator: true).context;
                      ScaffoldMessenger.of(navCtx).showSnackBar(
                        SnackBar(content: Text('Failed to import: $e')),
                      );
                    } finally {
                      if (mounted) setState(() => _busy.remove(globalIndex));
                    }
                  },
          );
        },
      ),
    );

    Widget controls = Padding(
      padding: const EdgeInsets.fromLTRB(16, 0, 16, 16),
      child: Row(
        children: [
          Text('Page ${_page + 1} of $totalPages'),
          const Spacer(),
          IconButton(
            tooltip: 'Previous page',
            onPressed: _page > 0 ? () => setState(() => _page -= 1) : null,
            icon: const Icon(Icons.chevron_left),
          ),
          IconButton(
            tooltip: 'Next page',
            onPressed: (_page + 1) < totalPages ? () => setState(() => _page += 1) : null,
            icon: const Icon(Icons.chevron_right),
          ),
        ],
      ),
    );

    return Column(
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: [
        grid,
        controls,
      ],
    );
  }

  Future<_ImportParams?> _promptImport(BuildContext context, FabAsset a) async {
    final projectCtrl = TextEditingController(text: '');
    final subdirCtrl = TextEditingController(text: '');
    bool overwrite = false;
    final result = await showDialog<_ImportParams>(
      context: context,
      builder: (ctx) {
        return AlertDialog(
          title: const Text('Import asset to project'),
          content: SingleChildScrollView(
            child: Column(
              mainAxisSize: MainAxisSize.min,
              children: [
                // Project picker dropdown (restores previous behavior)
                FutureBuilder<List<UnrealProjectInfo>>(
                  future: _api.listUnrealProjects(),
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
                Navigator.of(ctx).pop(_ImportParams(project: project, targetSubdir: subdir, overwrite: overwrite));
              },
              child: const Text('Import'),
            ),
          ],
        );
      },
    );
    return result;
  }

  Future<_CreateParams?> _promptCreateProject(BuildContext context, FabAsset asset) async {
    final enginePathCtrl = TextEditingController(text: '');
    final templateCtrl = TextEditingController(text: '');
    final outputDirCtrl = TextEditingController(text: '\$HOME/Documents/Unreal Projects');
    final projectNameCtrl = TextEditingController(text: 'MyNewGame');
    String projectType = 'bp';
    bool dryRun = true;
    final assetNameCtrl = TextEditingController(text: asset.title.isNotEmpty ? asset.title : asset.assetId);

    final result = await showDialog<_CreateParams>(
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
                const SizedBox(height: 8),
                TextField(
                  controller: assetNameCtrl,
                  decoration: const InputDecoration(
                    labelText: 'Asset name (optional if template path used)',
                    hintText: 'e.g., Stack O Bot',
                  ),
                ),
                const SizedBox(height: 8),
                TextField(
                  controller: templateCtrl,
                  decoration: const InputDecoration(
                    labelText: 'Template .uproject path (optional)',
                    hintText: '/path/to/Sample/Sample.uproject',
                  ),
                ),
                const SizedBox(height: 8),
                TextField(
                  controller: enginePathCtrl,
                  decoration: const InputDecoration(
                    labelText: 'Engine path (optional)',
                    hintText: '/path/to/Unreal/UE_5.xx',
                  ),
                ),
                const SizedBox(height: 8),
                Row(
                  children: [
                    const Text('Project type:'),
                    const SizedBox(width: 12),
                    DropdownButton<String>(
                      value: projectType,
                      items: const [
                        DropdownMenuItem(value: 'bp', child: Text('Blueprint (bp)')),
                        DropdownMenuItem(value: 'cpp', child: Text('C++ (cpp)')),
                      ],
                      onChanged: (v) {
                        if (v != null) {
                          projectType = v;
                          // refresh local state inside dialog
                          (ctx as Element).markNeedsBuild();
                        }
                      },
                    ),
                  ],
                ),
                StatefulBuilder(
                  builder: (context, setState) {
                    return CheckboxListTile(
                      contentPadding: EdgeInsets.zero,
                      title: const Text('Dry run (do not actually create)'),
                      value: dryRun,
                      onChanged: (v) => setState(() => dryRun = v ?? false),
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
                Navigator.of(ctx).pop(_CreateParams(
                  enginePath: enginePath.isEmpty ? null : enginePath,
                  templateProject: template.isEmpty ? null : template,
                  assetName: assetName.isEmpty ? null : assetName,
                  outputDir: outputDir,
                  projectName: projectName,
                  projectType: projectType,
                  dryRun: dryRun,
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

  @override
  void didUpdateWidget(covariant FabAssetsList oldWidget) {
    super.didUpdateWidget(oldWidget);
    if (oldWidget.assets.length != widget.assets.length) {
      // Reset to first page when data changes
      _page = 0;
    }
    // Clamp page if fewer total pages now
    final totalAssets = widget.assets.length;
    final totalPages = (totalAssets / _pageSize).ceil().clamp(1, 1000000);
    if (_page >= totalPages) _page = max(0, totalPages - 1);
  }
}
