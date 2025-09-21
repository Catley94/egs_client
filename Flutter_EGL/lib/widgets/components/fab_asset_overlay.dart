import 'package:flutter/material.dart';
import 'package:cached_network_image/cached_network_image.dart';

import '../../models/fab.dart';
import '../../services/api_service.dart';
import '../../services/image_cache.dart';

// Callback typedefs so the overlay can reuse existing flows from the caller
typedef PromptCreateProject = Future<dynamic> Function(BuildContext context, FabAsset asset);
typedef PromptImport = Future<dynamic> Function(BuildContext context, FabAsset asset);
typedef ShowJobProgressDialog = Future<dynamic> Function({required String jobId, required String title});
typedef MakeJobId = String Function();
typedef LaunchExternal = Future<void> Function(String url);

Future<void> showFabAssetOverlayDialog({
  required BuildContext context,
  required FabAsset asset,
  required ApiService api,
  required PromptCreateProject promptCreateProject,
  required PromptImport promptImport,
  required ShowJobProgressDialog showJobProgressDialog,
  required MakeJobId makeJobId,
  required LaunchExternal launchExternal,
  VoidCallback? onProjectsChanged,
  VoidCallback? onFabListChanged,
}) async {
  final a = asset;
  final images = a.images;
  int index = 0;
  bool working = false;
  bool downloadedNow = a.anyDownloaded;

  await showDialog<void>(
    context: context,
    builder: (ctx) {
      return StatefulBuilder(
        builder: (ctx, setStateSB) {
          final cs = Theme.of(ctx).colorScheme;
          return AlertDialog(
            contentPadding: const EdgeInsets.all(12),
            title: Text(a.title.isNotEmpty ? a.title : a.assetId),
            content: Builder(
              builder: (context) {
                final size = MediaQuery.of(context).size;
                final dialogWidth = (size.width * 0.9).clamp(300.0, 900.0);
                final dialogHeight = (size.height * 0.9).clamp(300.0, 700.0);
                // Compose extra meta
                String dist = a.distributionMethod.isNotEmpty ? a.distributionMethod : '';
                String idNs = '${a.assetNamespace}/${a.assetId}';
                // Compute full list of supported UE versions
                final engines = <String>{};
                for (final pv in a.projectVersions) {
                  for (final ev in pv.engineVersions) {
                    final parts = ev.split('_');
                    if (parts.length > 1) engines.add(parts[1]);
                  }
                }
                int score(String v) {
                  final p = v.split('.');
                  int maj = 0;
                  int min = 0;
                  if (p.isNotEmpty) maj = int.tryParse(p[0]) ?? 0;
                  if (p.length > 1) min = int.tryParse(p[1]) ?? 0;
                  return maj * 100 + min;
                }
                final versionsFull = engines.toList()
                  ..sort((a, b) => score(b).compareTo(score(a)));

                return SizedBox(
                  width: dialogWidth,
                  height: dialogHeight,
                  child: Column(
                    crossAxisAlignment: CrossAxisAlignment.stretch,
                    children: [
                      Expanded(
                        child: ClipRRect(
                          borderRadius: BorderRadius.circular(10),
                          child: images.isEmpty
                              ? Container(
                                  color: const Color(0xFF1F2933),
                                  child: const Center(child: Icon(Icons.image, size: 48, color: Color(0xFF9AA4AF))),
                                )
                              : PageView.builder(
                                  itemCount: images.length,
                                  onPageChanged: (i) => setStateSB(() => index = i),
                                  itemBuilder: (context, i) {
                                    final url = images[i].url;
                                    return CachedNetworkImage(
                                      imageUrl: url,
                                      cacheManager: AppImageCache.manager,
                                      fit: BoxFit.cover,
                                      errorWidget: (c, url, e) => const Center(child: Icon(Icons.broken_image, size: 48, color: Color(0xFF9AA4AF))),
                                      placeholder: (c, url) => const Center(child: SizedBox(width: 32, height: 32, child: CircularProgressIndicator(strokeWidth: 2))),
                                    );
                                  },
                                ),
                        ),
                      ),
                      const SizedBox(height: 8),
                      Row(
                        mainAxisAlignment: MainAxisAlignment.center,
                        children: List.generate(images.length, (i) => Container(
                          width: 8,
                          height: 8,
                          margin: const EdgeInsets.symmetric(horizontal: 3),
                          decoration: BoxDecoration(
                            shape: BoxShape.circle,
                            color: i == index ? cs.primary : cs.outlineVariant,
                          ),
                        )),
                      ),
                      const SizedBox(height: 8),
                      Row(
                        children: [
                          Expanded(
                            child: Column(
                              crossAxisAlignment: CrossAxisAlignment.start,
                              children: [
                                Text(a.title.isNotEmpty ? a.title : a.assetId, style: Theme.of(context).textTheme.titleMedium?.copyWith(fontWeight: FontWeight.bold)),
                                const SizedBox(height: 6),
                                Wrap(
                                  spacing: 8,
                                  runSpacing: 4,
                                  children: [
                                    if (a.anyDownloaded || downloadedNow)
                                      Chip(
                                        label: const Text('Downloaded'),
                                        backgroundColor: Colors.green.withOpacity(0.15),
                                        side: BorderSide(color: Colors.green.shade700),
                                      ),
                                    if (a.isCompleteProject)
                                      Chip(
                                        label: const Text('Complete Project'),
                                        backgroundColor: Colors.blue.withOpacity(0.12),
                                        side: BorderSide(color: Colors.blue.shade700),
                                      ),
                                    if (a.sizeLabel.isNotEmpty)
                                      Chip(
                                        label: Text(a.sizeLabel),
                                        backgroundColor: cs.surfaceVariant,
                                        side: BorderSide(color: cs.outlineVariant),
                                      ),
                                  ],
                                ),
                              ],
                            ),
                          ),
                        ],
                      ),
                      const SizedBox(height: 8),
                      if (a.description.isNotEmpty)
                        Align(
                          alignment: Alignment.centerLeft,
                          child: Text(
                            a.description,
                            style: Theme.of(context).textTheme.bodyMedium,
                            softWrap: true,
                            maxLines: 6,
                            overflow: TextOverflow.ellipsis,
                          ),
                        ),
                      if (versionsFull.isNotEmpty) ...[
                        const SizedBox(height: 6),
                        Align(
                          alignment: Alignment.centerLeft,
                          child: Text(
                            'Supported UE: ' + versionsFull.join(', '),
                            style: Theme.of(context).textTheme.bodySmall?.copyWith(color: cs.onSurfaceVariant),
                          ),
                        ),
                      ],
                      const SizedBox(height: 8),
                      Align(
                        alignment: Alignment.centerLeft,
                        child: Text(
                          'Distribution: ' + (dist.isEmpty ? 'unknown' : dist) + '    |    ' + idNs,
                          style: Theme.of(context).textTheme.bodySmall?.copyWith(color: cs.onSurfaceVariant),
                        ),
                      ),
                      const SizedBox(height: 8),
                      // Bottom action row: Primary + Download + Open + Refresh
                      Row(
                        children: [
                          FilledButton.icon(
                            onPressed: working
                                ? null
                                : () async {
                                    try {
                                      setStateSB(() => working = true);
                                      if (a.isCompleteProject) {
                                        final params = await promptCreateProject(context, a);
                                        if (params == null) return;
                                        final res = await api.createUnrealProject(
                                          enginePath: params.enginePath,
                                          templateProject: params.templateProject,
                                          assetName: params.assetName,
                                          outputDir: params.outputDir,
                                          projectName: params.projectName,
                                          projectType: params.projectType,
                                          dryRun: params.dryRun,
                                        );
                                        if (!context.mounted) return;
                                        final ok = res.success;
                                        final msg = res.message.isNotEmpty ? res.message : (ok ? 'OK' : 'Failed');
                                        ScaffoldMessenger.of(context).showSnackBar(
                                          SnackBar(content: Text(msg)),
                                        );
                                        if (ok && !params.dryRun) {
                                          onProjectsChanged?.call();
                                          onFabListChanged?.call();
                                        }
                                      } else {
                                        final params = await promptImport(context, a);
                                        if (params == null) return;
                                        final jobId = makeJobId();
                                        final dlg = showJobProgressDialog(jobId: jobId, title: 'Importing asset...');
                                        final result = await api.importAsset(
                                          assetName: a.title.isNotEmpty ? a.title : a.assetId,
                                          project: params.project,
                                          targetSubdir: params.targetSubdir.isEmpty ? null : params.targetSubdir,
                                          overwrite: params.overwrite,
                                          jobId: jobId,
                                        );
                                        if (context.mounted) {
                                          final nav = Navigator.of(context, rootNavigator: true);
                                          if (nav.canPop()) nav.pop();
                                        }
                                        await dlg.catchError((_ ){});
                                        if (!context.mounted) return;
                                        final ok = result.success;
                                        final msg = result.message.isNotEmpty ? result.message : (ok ? 'OK' : 'Failed');
                                        ScaffoldMessenger.of(context).showSnackBar(
                                          SnackBar(content: Text(msg)),
                                        );
                                        if (ok) {
                                          onFabListChanged?.call();
                                        }
                                      }
                                    } catch (e) {
                                      if (!context.mounted) return;
                                      ScaffoldMessenger.of(context).showSnackBar(
                                        SnackBar(content: Text('Operation failed: $e')),
                                      );
                                    } finally {
                                      setStateSB(() => working = false);
                                    }
                                  },
                            icon: working
                                ? const SizedBox(width: 16, height: 16, child: CircularProgressIndicator(strokeWidth: 2))
                                : Icon(a.isCompleteProject ? Icons.add : Icons.download),
                            label: Text(a.isCompleteProject ? 'Create Project' : 'Import Asset'),
                          ),
                          const SizedBox(width: 8),
                          OutlinedButton.icon(
                            onPressed: (working || downloadedNow || a.anyDownloaded)
                                ? null
                                : () async {
                                    try {
                                      setStateSB(() => working = true);
                                      if (a.projectVersions.isEmpty) {
                                        ScaffoldMessenger.of(context).showSnackBar(
                                          const SnackBar(content: Text('No versions available to download')),
                                        );
                                        return;
                                      }
                                      final artifactId = a.projectVersions.first.artifactId;
                                      if (artifactId.isEmpty) {
                                        ScaffoldMessenger.of(context).showSnackBar(
                                          const SnackBar(content: Text('No artifact ID found for this asset')),
                                        );
                                        return;
                                      }
                                      // Show progress overlay and stream WS progress for this download
                                      final jobId = makeJobId();
                                      final dlg = showJobProgressDialog(jobId: jobId, title: 'Downloading asset...');
                                      final res = await api.downloadAsset(
                                        namespace: a.assetNamespace,
                                        assetId: a.assetId,
                                        artifactId: artifactId,
                                        jobId: jobId,
                                      );
                                      // Ensure progress dialog is closed if still open
                                      if (context.mounted) {
                                        final nav = Navigator.of(context, rootNavigator: true);
                                        if (nav.canPop()) nav.pop();
                                      }
                                      await dlg.catchError((_ ){});
                                      if (!context.mounted) return;
                                      final msg = res.message.isNotEmpty ? res.message : 'Download started';
                                      final wasCancelled = msg.toLowerCase().contains('cancel');
                                      ScaffoldMessenger.of(context).showSnackBar(
                                        SnackBar(content: Text(msg)),
                                      );
                                      if (!wasCancelled) {
                                        // Mark as downloaded locally and request a list refresh
                                        setStateSB(() => downloadedNow = true);
                                        onFabListChanged?.call();
                                      }
                                    } catch (e) {
                                      if (!context.mounted) return;
                                      ScaffoldMessenger.of(context).showSnackBar(
                                        SnackBar(content: Text('Failed to start download: $e')),
                                      );
                                    } finally {
                                      setStateSB(() => working = false);
                                    }
                                  },
                            icon: const Icon(Icons.download),
                            label: const Text('Download'),
                          ),
                          const SizedBox(width: 8),
                          // Open in Browser
                          FilledButton.icon(
                            onPressed: working ? null : () async {
                              setStateSB(() => working = true);
                              try {
                                final url = 'https://www.fab.com/listings/${a.assetNamespace}/${a.assetId}';
                                await launchExternal(url);
                              } finally {
                                setStateSB(() => working = false);
                              }
                            },
                            icon: working ? const SizedBox(width: 16, height: 16, child: CircularProgressIndicator(strokeWidth: 2)) : const Icon(Icons.open_in_new),
                            label: const Text('Open in browser'),
                          ),
                          const Spacer(),
                          Tooltip(
                            message: 'Re-fetch the latest info for this asset from the store and your disk (downloaded status, supported versions, etc.). Use after downloads/imports or external changes.',
                            preferBelow: false,
                            child: TextButton.icon(
                              onPressed: () async {
                                try {
                                  setStateSB(() => working = true);
                                  final result = await api.refreshFabAsset(assetNamespace: a.assetNamespace, assetId: a.assetId);
                                  if (!context.mounted) return;
                                  final ok = result.success;
                                  if (ok) downloadedNow = result.anyDownloaded;
                                  final msg = result.message.isNotEmpty ? result.message : (ok ? 'Metadata refreshed' : 'Failed to refresh metadata');
                                  ScaffoldMessenger.of(context).showSnackBar(
                                    SnackBar(content: Text(msg)),
                                  );
                                  // Notify parent list to refresh the list display
                                  if (ok) onFabListChanged?.call();
                                } catch (e) {
                                  if (!context.mounted) return;
                                  ScaffoldMessenger.of(context).showSnackBar(
                                    SnackBar(content: Text('Failed to refresh metadata: $e')),
                                  );
                                } finally {
                                  setStateSB(() => working = false);
                                }
                              },
                              icon: const Icon(Icons.refresh),
                              label: const Text('Refresh metadata'),
                            ),
                          ),
                        ],
                      ),
                    ],
                  ),
                );
              },
            ),
            actions: [
              TextButton(
                onPressed: () => Navigator.of(ctx).pop(),
                child: const Text('Close'),
              )
            ],
          );
        },
      );
    },
  );
}
