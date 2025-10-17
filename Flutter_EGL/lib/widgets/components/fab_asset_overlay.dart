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
  final _asset = asset;
  final images = _asset.images;
  int index = 0;
  bool working = false;
  bool downloadedNow = _asset.anyDownloaded;
  String? selectedVersion; // e.g., '5.6'
  final Set<String> downloadedVersionsNow = <String>{};
  // Use a root-level context for SnackBars to avoid using a possibly deactivated dialog context across awaits
  final BuildContext rootScaffoldCtx = Navigator.of(context, rootNavigator: true).context;

  await showDialog<void>(
    context: context,
    builder: (ctx) {
      return StatefulBuilder(
        builder: (ctx, setStateSB) {
          final colorScheme = Theme.of(ctx).colorScheme;
          return AlertDialog(
            contentPadding: const EdgeInsets.all(12),
            // TODO: Currently the title is explicitly being used to move the app around
            title: Text(_asset.title.isNotEmpty ? _asset.title : _asset.assetId),
            content: Builder(
              builder: (context) {
                final size = MediaQuery.of(context).size;
                final dialogWidth = (size.width * 0.9).clamp(300.0, 900.0);
                final dialogHeight = (size.height * 0.9).clamp(300.0, 700.0);
                // Compose extra meta
                String dist = _asset.distributionMethod.isNotEmpty ? _asset.distributionMethod : '';
                String idNs = '${_asset.assetNamespace}/${_asset.assetId}';
                // Compute full list of supported UE versions
                final engines = <String>{};
                for (final projectVersion in _asset.projectVersions) {
                  for (final ev in projectVersion.engineVersions) {
                    final parts = ev.split('_');
                    if (parts.length > 1) engines.add(parts[1]);
                  }
                }
                int score(String v) {
                  final parts = v.split('.');
                  int major = 0;
                  int min = 0;
                  if (parts.isNotEmpty) major = int.tryParse(parts[0]) ?? 0;
                  if (parts.length > 1) min = int.tryParse(parts[1]) ?? 0;
                  return major * 100 + min;
                }
                final versionsFull = engines.toList()
                  ..sort((a, b) => score(b).compareTo(score(a)));

                // Initialize default selected version to latest if not set
                selectedVersion ??= versionsFull.isNotEmpty ? versionsFull.first : null;

                // Seed in-memory set with versions reported by backend (once)
                if (downloadedVersionsNow.isEmpty) {
                  downloadedVersionsNow.addAll(_asset.downloadedVersions);
                }

                bool _pvSupportsMm(FabProjectVersion pv, String mm) => pv.engineVersions.any((ev) => ev.trim() == 'UE_' + mm);
                bool _isVersionDownloaded(String majorMinor) {
                  // print("Downloaded versions: $downloadedVersionsNow");
                  // print("Major Minor: $majorMinor");
                  if (downloadedVersionsNow.contains(majorMinor)) {
                    print("downloadedVersionsNow: $downloadedVersionsNow");
                    print("Major Minor: $majorMinor");
                    print("Contains version");
                    return true;
                  }

                  // for (final _version in _asset.downloadedVersions) {
                  //   print("_version: $_version");
                  //   print("Major Minor: $majorMinor");
                  //   if (_version.trim().contains(majorMinor)) {
                  //     print("_version matches!");
                  //     return true;
                  //   }
                  // }

                  // for (final pv in _asset.projectVersions) {
                  //   if (pv.downloaded && _pvSupportsMm(pv, majorMinor)) {
                  //     print("_________________________________________________");
                  //     print("Foreach project version in projectVersions");
                  //     print("Major Minor: $majorMinor");
                  //     print("Project version downloaded: ${pv.downloaded}");
                  //     print("&&");
                  //     print("Project Version Supports Major Minor: ${_pvSupportsMm(pv, majorMinor)}");
                  //     print("_________________________________________________");
                  //     return true;
                  //   }
                  // }
                  return false;
                }
                final bool selectedDownloaded = (selectedVersion != null) ? _isVersionDownloaded(selectedVersion!) : false;
                final bool disableDownload = working || (versionsFull.isNotEmpty ? ((selectedVersion != null) && selectedDownloaded) : (downloadedNow || _asset.anyDownloaded));

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
                            color: i == index ? colorScheme.primary : colorScheme.outlineVariant,
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
                                Text(_asset.title.isNotEmpty ? _asset.title : _asset.assetId, style: Theme.of(context).textTheme.titleMedium?.copyWith(fontWeight: FontWeight.bold)),
                                const SizedBox(height: 6),
                                Wrap(
                                  spacing: 8,
                                  runSpacing: 4,
                                  children: [
                                    if (((versionsFull.isNotEmpty) && (selectedVersion != null) && selectedDownloaded) || (versionsFull.isEmpty && (_asset.anyDownloaded || downloadedNow)))
                                      Chip(
                                        label: const Text('Downloaded'),
                                        backgroundColor: Colors.green.withValues(alpha: 0.15),
                                        side: BorderSide(color: Colors.green.shade700),
                                      ),
                                    // if (a.isCompleteProject)
                                    //   Chip(
                                    //     label: const Text('Complete Project'),
                                    //     backgroundColor: Colors.blue.withOpacity(0.12),
                                    //     side: BorderSide(color: Colors.blue.shade700),
                                    //   ),
                                    if (_asset.sizeLabel.isNotEmpty)
                                      Chip(
                                        label: Text(_asset.sizeLabel),
                                        backgroundColor: colorScheme.surfaceContainerHighest,
                                        side: BorderSide(color: colorScheme.outlineVariant),
                                      ),
                                  ],
                                ),
                              ],
                            ),
                          ),
                        ],
                      ),
                      const SizedBox(height: 8),
                      if (_asset.description.isNotEmpty)
                        Align(
                          alignment: Alignment.centerLeft,
                          child: Text(
                            _asset.description,
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
                            'Supported UE: ${versionsFull.join(', ')}',
                            style: Theme.of(context).textTheme.bodySmall?.copyWith(color: colorScheme.onSurfaceVariant),
                          ),
                        ),
                      ],
                      const SizedBox(height: 8),
                      Align(
                        alignment: Alignment.centerLeft,
                        child: Text(
                          'Distribution: ${dist.isEmpty ? 'unknown' : dist}    |    $idNs',
                          style: Theme.of(context).textTheme.bodySmall?.copyWith(color: colorScheme.onSurfaceVariant),
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
                                      if (_asset.isCompleteProject) {
                                        final params = await promptCreateProject(context, _asset);
                                        if (params == null) return;
                                        final jobId = makeJobId();
                                        final dlg = showJobProgressDialog(jobId: jobId, title: 'Creating project...');
                                        final res = await api.createUnrealProject(
                                          enginePath: params.enginePath,
                                          templateProject: params.templateProject,
                                          assetName: params.assetName,
                                          ue: (selectedVersion != null && selectedVersion!.isNotEmpty) ? selectedVersion : null,
                                          outputDir: params.outputDir,
                                          projectName: params.projectName,
                                          projectType: params.projectType,
                                          dryRun: params.dryRun,
                                          jobId: jobId,
                                        );
                                        if (context.mounted) {
                                          final nav = Navigator.of(context, rootNavigator: true);
                                          if (nav.canPop()) nav.pop();
                                        }
                                        await dlg.catchError((_) {});
                                        if (!context.mounted) return;
                                        final ok = res.success;
                                        final msg = res.message.isNotEmpty ? res.message : (ok ? 'OK' : 'Failed');
                                        ScaffoldMessenger.of(rootScaffoldCtx).showSnackBar(
                                          SnackBar(content: Text(msg)),
                                        );
                                        if (ok && !params.dryRun) {
                                          onProjectsChanged?.call();
                                          onFabListChanged?.call();
                                        }
                                      } else {
                                        // Import Asset
                                        final params = await promptImport(context, _asset);
                                        if (params == null) return;
                                        final jobId = makeJobId();
                                        final dlg = showJobProgressDialog(jobId: jobId, title: 'Importing asset...');
                                        // Determine best artifact for selected UE version to align import with download folder naming
                                        String? _artifactForVersion(FabAsset asset, String? mm) {
                                          if (mm == null || mm.isEmpty) return null;
                                          final token = 'UE_' + mm;
                                          for (final pv in asset.projectVersions) {
                                            if (pv.engineVersions.any((ev) => ev.trim() == token)) {
                                              return pv.artifactId;
                                            }
                                          }
                                          return null;
                                        }
                                        String? _pickBestArtifactId(FabAsset asset) {
                                          int scoreMm(String mm) {
                                            final parts = mm.split('.');
                                            final maj = int.tryParse(parts.isNotEmpty ? parts[0] : '0') ?? 0;
                                            final min = int.tryParse(parts.length > 1 ? parts[1] : '0') ?? 0;
                                            return maj * 100 + min;
                                          }
                                          String? bestArtifact;
                                          int bestScore = -1;
                                          for (final pv in asset.projectVersions) {
                                            int pvBest = -1;
                                            for (final ev in pv.engineVersions) {
                                              final parts = ev.split('_');
                                              if (parts.length > 1) {
                                                final mm = parts[1];
                                                final sc = scoreMm(mm);
                                                pvBest = pvBest < 0 ? sc : (pvBest > sc ? pvBest : sc);
                                              }
                                            }
                                            if (pvBest < 0) pvBest = 0;
                                            if (pvBest > bestScore) {
                                              bestScore = pvBest;
                                              bestArtifact = pv.artifactId.isNotEmpty ? pv.artifactId : bestArtifact;
                                            }
                                          }
                                          return bestArtifact;
                                        }
                                        final computedArtifactId = _artifactForVersion(_asset, selectedVersion) ?? _pickBestArtifactId(_asset) ?? (_asset.projectVersions.isNotEmpty ? _asset.projectVersions.last.artifactId : '');
                                        final result = await api.importAsset(
                                          assetName: _asset.title.isNotEmpty ? _asset.title : _asset.assetId,
                                          project: params.project,
                                          targetSubdir: params.targetSubdir.isEmpty ? null : params.targetSubdir,
                                          overwrite: params.overwrite,
                                          jobId: jobId,
                                          namespace: _asset.assetNamespace,
                                          assetId: _asset.assetId,
                                          artifactId: computedArtifactId.isEmpty ? null : computedArtifactId,
                                          ue: (selectedVersion != null && selectedVersion!.isNotEmpty) ? selectedVersion : null,
                                        );
                                        if (context.mounted) {
                                          final nav = Navigator.of(context, rootNavigator: true);
                                          if (nav.canPop()) nav.pop();
                                        }
                                        await dlg.catchError((_ ){});
                                        if (!context.mounted) return;
                                        final ok = result.success;
                                        final msg = result.message.isNotEmpty ? result.message : (ok ? 'OK' : 'Failed');
                                        ScaffoldMessenger.of(rootScaffoldCtx).showSnackBar(
                                          SnackBar(content: Text(msg)),
                                        );
                                        if (ok) {
                                          onFabListChanged?.call();
                                        }
                                      }
                                    } catch (e) {
                                      if (!context.mounted) return;
                                      ScaffoldMessenger.of(rootScaffoldCtx).showSnackBar(
                                        SnackBar(content: Text('Operation failed: $e')),
                                      );
                                    } finally {
                                      setStateSB(() => working = false);
                                    }
                                  },
                            icon: working
                                ? const SizedBox(width: 16, height: 16, child: CircularProgressIndicator(strokeWidth: 2))
                                : Icon(_asset.isCompleteProject ? Icons.add : Icons.download),
                            label: Text(_asset.isCompleteProject ? 'Create Project' : 'Import Asset'),
                          ),
                          const SizedBox(width: 8),
                          if (versionsFull.isNotEmpty) ...[
                            SizedBox(
                              width: 140,
                              child: InputDecorator(
                                decoration: const InputDecoration(
                                  labelText: 'UE Version',
                                  border: OutlineInputBorder(gapPadding: 0),
                                  isDense: true,
                                  contentPadding: EdgeInsets.symmetric(horizontal: 8, vertical: 8),
                                ),
                                child: DropdownButtonHideUnderline(
                                  child: DropdownButton<String>(
                                    value: selectedVersion,
                                    isDense: true,
                                    items: versionsFull.map((v) => DropdownMenuItem<String>(
                                      value: v,
                                      child: Row(
                                        mainAxisSize: MainAxisSize.min,
                                        children: [
                                          if (_isVersionDownloaded(v)) ...[
                                            Icon(Icons.check, size: 16, color: Colors.green.shade700),
                                            const SizedBox(width: 6),
                                          ] else ...[
                                            const SizedBox(width: 22), // reserve space to align text
                                          ],
                                          Text(v),
                                        ],
                                      ),
                                    )).toList(),
                                    onChanged: (v) => setStateSB(() => selectedVersion = v),
                                  ),
                                ),
                              ),
                            ),
                            const SizedBox(width: 8),
                          ],
                          OutlinedButton.icon(
                            onPressed: disableDownload
                                ? null
                                : () async {
                                    bool overlayClosed = false;
                                    try {
                                      setStateSB(() => working = true);
                                      if (_asset.projectVersions.isEmpty) {
                                        ScaffoldMessenger.of(rootScaffoldCtx).showSnackBar(
                                          const SnackBar(content: Text('No versions available to download')),
                                        );
                                        return;
                                      }
                                      // Resolve artifact based on selected UE version (if any), otherwise pick best available.
                                      String? artifactForVersion(FabAsset asset, String? mm) {
                                        if (mm == null || mm.isEmpty) return null;
                                        final token = 'UE_' + mm;
                                        for (final pv in asset.projectVersions) {
                                          if (pv.engineVersions.any((ev) => ev.trim() == token)) {
                                            return pv.artifactId;
                                          }
                                        }
                                        return null;
                                      }

                                      String? pickBestArtifactId(FabAsset asset) {
                                        int scoreMm(String mm) {
                                          final parts = mm.split('.');
                                          final maj = int.tryParse(parts.isNotEmpty ? parts[0] : '0') ?? 0;
                                          final min = int.tryParse(parts.length > 1 ? parts[1] : '0') ?? 0;
                                          return maj * 100 + min;
                                        }
                                        String? bestArtifact;
                                        int bestScore = -1;
                                        for (final pv in asset.projectVersions) {
                                          int pvBest = -1;
                                          for (final ev in pv.engineVersions) {
                                            final parts = ev.split('_');
                                            if (parts.length > 1) {
                                              final mm = parts[1];
                                              pvBest = pvBest < 0 ? scoreMm(mm) : (pvBest > scoreMm(mm) ? pvBest : scoreMm(mm));
                                            }
                                          }
                                          if (pvBest < 0) pvBest = 0;
                                          if (pvBest > bestScore) {
                                            bestScore = pvBest;
                                            bestArtifact = pv.artifactId.isNotEmpty ? pv.artifactId : bestArtifact;
                                          }
                                        }
                                        return bestArtifact;
                                      }

                                      final artifactId = artifactForVersion(_asset, selectedVersion) ?? pickBestArtifactId(_asset) ?? (_asset.projectVersions.isNotEmpty ? _asset.projectVersions.last.artifactId : '');
                                      if (artifactId.isEmpty) {
                                        ScaffoldMessenger.of(rootScaffoldCtx).showSnackBar(
                                          const SnackBar(content: Text('No artifact ID found for this asset')),
                                        );
                                        return;
                                      }
                                      // Close the overlay first to avoid deactivated context issues when starting another download
                                      final NavigatorState rootNav = Navigator.of(context, rootNavigator: true);
                                      if (rootNav.canPop()) rootNav.pop();
                                      overlayClosed = true;
                                      // Show progress overlay and stream WS progress for this download
                                      final jobId = makeJobId();
                                      final dlg = showJobProgressDialog(jobId: jobId, title: 'Downloading asset...');
                                      final res = await api.downloadAsset(
                                        namespace: _asset.assetNamespace,
                                        assetId: _asset.assetId,
                                        artifactId: artifactId,
                                        jobId: jobId,
                                        ueVersion: selectedVersion,
                                      );
                                      // Ensure progress dialog is closed if still open
                                      if (context.mounted) {
                                        final nav = Navigator.of(context, rootNavigator: true);
                                        if (nav.canPop()) nav.pop();
                                      }
                                      await dlg.catchError((_ ){});
                                      // Do not use setState after overlay has been closed
                                      final msg = res.message.isNotEmpty ? res.message : 'Download started';
                                      final wasCancelled = msg.toLowerCase().contains('cancel');
                                      ScaffoldMessenger.of(rootScaffoldCtx).showSnackBar(
                                        SnackBar(content: Text(msg)),
                                      );
                                      if (!wasCancelled) {
                                        // Mark as downloaded locally for the selected version and request a list refresh
                                        if (!overlayClosed) {
                                          setStateSB(() {
                                            downloadedNow = true;
                                            if (selectedVersion != null && selectedVersion!.isNotEmpty) {
                                              downloadedVersionsNow.add(selectedVersion!);
                                            }
                                          });
                                        }
                                        onFabListChanged?.call();
                                      }
                                    } catch (e) {
                                      // If overlay is closed, avoid context-mounted checks & setState
                                      ScaffoldMessenger.of(rootScaffoldCtx).showSnackBar(
                                        SnackBar(content: Text('Failed to start download: $e')),
                                      );
                                    } finally {
                                      if (!overlayClosed) {
                                        setStateSB(() => working = false);
                                      }
                                    }
                                  },
                            icon: const Icon(Icons.download),
                            label: Text(disableDownload && versionsFull.isNotEmpty ? 'Downloaded' : 'Download'),
                          ),
                          const SizedBox(width: 8),
                          // Open in Browser
                          FilledButton.icon(
                            onPressed: working ? null : () async {
                              setStateSB(() => working = true);
                              try {
                                final url = (_asset.url != null && _asset.url!.isNotEmpty)
                                    ? _asset.url!
                                    : 'https://www.fab.com/listings/${_asset.assetId}';
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
                                  // Refreshes the whole list currently
                                  final result = await api.refreshFabAsset(assetNamespace: _asset.assetNamespace, assetId: _asset.assetId);
                                  if (!context.mounted) return;
                                  final ok = result.success;
                                  if (ok) downloadedNow = result.anyDownloaded;
                                  final msg = result.message.isNotEmpty ? result.message : (ok ? 'Metadata refreshed' : 'Failed to refresh metadata');
                                  ScaffoldMessenger.of(rootScaffoldCtx).showSnackBar(
                                    SnackBar(content: Text(msg)),
                                  );
                                  // Notify parent list to refresh the list display
                                  if (ok) onFabListChanged?.call();
                                } catch (e) {
                                  if (!context.mounted) return;
                                  ScaffoldMessenger.of(rootScaffoldCtx).showSnackBar(
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
