// lib/widgets/library_tab.dart (new file)
import 'package:flutter/material.dart';
import 'dart:async';
import 'dart:math';
import 'fab_library_item.dart';
import '../services/api_service.dart';
import '../models/unreal.dart';
import '../models/fab.dart';
import 'package:url_launcher/url_launcher.dart';
import 'package:window_manager/window_manager.dart';

class LibraryTab extends StatefulWidget {
  const LibraryTab({super.key});

  @override
  State<LibraryTab> createState() => _LibraryTabState();
}

enum AssetSortMode { newerUEFirst, olderUEFirst, alphaAZ, alphaZA }

class _LibraryTabState extends State<LibraryTab> {
  final TextEditingController _searchController = TextEditingController();
  String _query = '';
  String _versionFilter = '';
  bool _onlyCompleteProjects = false;
  AssetSortMode _sortMode = AssetSortMode.newerUEFirst;
  final ScrollController _scrollController = ScrollController();
  final GlobalKey<_FabAssetsGridState> _fabKey = GlobalKey<_FabAssetsGridState>();
  late final ApiService _api;
  late Future<List<UnrealEngineInfo>> _enginesFuture;
  late Future<List<UnrealProjectInfo>> _projectsFuture;
  late Future<List<FabAsset>> _fabFuture;

  // cache of engines for deciding version on open
  List<UnrealEngineInfo> _engines = const [];
  bool _opening = false;

  // Settings: user-configurable paths
  final TextEditingController _projectsDirCtrl = TextEditingController();
  final TextEditingController _enginesDirCtrl = TextEditingController();
  final TextEditingController _cacheDirCtrl = TextEditingController();
  final TextEditingController _downloadsDirCtrl = TextEditingController();

  @override
  void initState() {
    super.initState();
    _scrollController.addListener(_onScroll);
    _api = ApiService();
  }

  void _requestMoreFabItems() {
    // Pagination mode: no-op (infinite scroll disabled)
  }

  void _onScroll() {
    if (!_scrollController.hasClients) return;
    final max = _scrollController.position.maxScrollExtent;
    final pixels = _scrollController.position.pixels;
    if (pixels >= max - 400) {
      _requestMoreFabItems();
    }
  }

  @override
  void dispose() {
    _scrollController.removeListener(_onScroll);
    _scrollController.dispose();
    _searchController.dispose();
    _projectsDirCtrl.dispose();
    _enginesDirCtrl.dispose();
    _cacheDirCtrl.dispose();
    _downloadsDirCtrl.dispose();
    super.dispose();
  }

  Future<void> _applyPaths() async {
    final projectsDir = _projectsDirCtrl.text.trim();
    final enginesDir = _enginesDirCtrl.text.trim();
    final cacheDir = _cacheDirCtrl.text.trim();
    final downloadsDir = _downloadsDirCtrl.text.trim();
    try {
      await _api.setPathsConfig(
        projectsDir: projectsDir.isNotEmpty ? projectsDir : null,
        enginesDir: enginesDir.isNotEmpty ? enginesDir : null,
        cacheDir: cacheDir.isNotEmpty ? cacheDir : null,
        downloadsDir: downloadsDir.isNotEmpty ? downloadsDir : null,
      );
      // Refresh lists using new effective bases
      setState(() {
        _enginesFuture = _api.listUnrealEngines().then((v) => _engines = v).then((_) => _engines).catchError((_) => _engines);
        _projectsFuture = _api.listUnrealProjects();
        _fabFuture = _api.getFabList();
      });
    } catch (e) {
      if (mounted) {
        ScaffoldMessenger.of(context).showSnackBar(
          SnackBar(content: Text('Failed to apply paths: $e')),
        );
      }
    }
  }

  Future<void> _openSettingsDialog() async {
    // Ensure latest values
    try {
      final cfg = await _api.getPathsConfig();
      final configured = cfg['configured'] as Map<String, dynamic>?;
      final effectiveProjects = cfg['effective_projects_dir']?.toString() ?? '';
      final effectiveEngines = cfg['effective_engines_dir']?.toString() ?? '';
      final effectiveCache = cfg['effective_cache_dir']?.toString() ?? '';
      final effectiveDownloads = cfg['effective_downloads_dir']?.toString() ?? '';
      setState(() {
        _projectsDirCtrl.text = (configured?['projects_dir']?.toString() ?? effectiveProjects);
        _enginesDirCtrl.text = (configured?['engines_dir']?.toString() ?? effectiveEngines);
        _cacheDirCtrl.text = (configured?['cache_dir']?.toString() ?? effectiveCache);
        _downloadsDirCtrl.text = (configured?['downloads_dir']?.toString() ?? effectiveDownloads);
      });
    } catch (_) {}

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
                  controller: _projectsDirCtrl,
                  decoration: const InputDecoration(
                    labelText: 'Projects directory',
                    hintText: '/path/to/Unreal Projects',
                  ),
                ),
                const SizedBox(height: 8),
                TextField(
                  controller: _enginesDirCtrl,
                  decoration: const InputDecoration(
                    labelText: 'Engines directory',
                    hintText: '/path/to/UnrealEngines',
                  ),
                ),
                const SizedBox(height: 8),
                TextField(
                  controller: _cacheDirCtrl,
                  decoration: const InputDecoration(
                    labelText: 'Cache directory',
                    hintText: './cache',
                  ),
                ),
                const SizedBox(height: 8),
                TextField(
                  controller: _downloadsDirCtrl,
                  decoration: const InputDecoration(
                    labelText: 'Downloads directory',
                    hintText: './downloads',
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
                await _applyPaths();
                if (context.mounted) Navigator.of(ctx).pop();
              },
              child: const Text('Apply'),
            )
          ],
        );
      },
    );
  }

  @override
  void didChangeDependencies() {
    super.didChangeDependencies();
    // kick off futures after widget is mounted
    _enginesFuture = _api.listUnrealEngines().then((v) => _engines = v).then((_) => _engines).catchError((_) => _engines);
    _projectsFuture = _api.listUnrealProjects();
    _fabFuture = _api.getFabList();
    // load configured paths
    _api.getPathsConfig().then((cfg) {
      final configured = cfg['configured'] as Map<String, dynamic>?;
      final effectiveProjects = cfg['effective_projects_dir']?.toString() ?? '';
      final effectiveEngines = cfg['effective_engines_dir']?.toString() ?? '';
      final effectiveCache = cfg['effective_cache_dir']?.toString() ?? '';
      final effectiveDownloads = cfg['effective_downloads_dir']?.toString() ?? '';
      setState(() {
        _projectsDirCtrl.text = (configured?['projects_dir']?.toString() ?? effectiveProjects);
        _enginesDirCtrl.text = (configured?['engines_dir']?.toString() ?? effectiveEngines);
        _cacheDirCtrl.text = (configured?['cache_dir']?.toString() ?? effectiveCache);
        _downloadsDirCtrl.text = (configured?['downloads_dir']?.toString() ?? effectiveDownloads);
      });
    }).catchError((_) {});
  }

  void _refreshProjects() {
    setState(() {
      _projectsFuture = _api.listUnrealProjects();
    });
  }

  void _refreshFabList() {
    setState(() {
      _fabFuture = _api.getFabList();
    });
  }

  @override
  Widget build(BuildContext context) {
    final cs = Theme.of(context).colorScheme;
    return Scrollbar(
      controller: _scrollController,
      child: SingleChildScrollView(
        controller: _scrollController,
        primary: false,
        padding: const EdgeInsets.all(16),
        child: Column(
          crossAxisAlignment: CrossAxisAlignment.start,
          children: [
            // Settings icon to open overlay
            Row(
              children: [
                const Spacer(),
                IconButton(
                  tooltip: 'Settings',
                  icon: const Icon(Icons.settings),
                  onPressed: _openSettingsDialog,
                ),
              ],
            ),

            // Engine Versions grid (new)
            Text(
              'Engine Versions',
              style: Theme.of(context).textTheme.titleMedium?.copyWith(
                    fontWeight: FontWeight.w700,
                  ),
            ),
            const SizedBox(height: 12),
            LayoutBuilder(
              builder: (context, constraints) {
                const tileMinWidth = 140.0;
                const spacing = 12.0;
                final count = (constraints.maxWidth / (tileMinWidth + spacing))
                    .floor()
                    .clamp(1, 6);
                return FutureBuilder<List<UnrealEngineInfo>>(
                  future: _enginesFuture,
                  builder: (context, snapshot) {
                    if (snapshot.connectionState == ConnectionState.waiting) {
                      return const Center(child: Padding(padding: EdgeInsets.all(24), child: CircularProgressIndicator()));
                    }
                    if (snapshot.hasError) {
                      return Padding(
                        padding: const EdgeInsets.all(8.0),
                        child: Text('Failed to load engines: ${snapshot.error}', style: const TextStyle(color: Colors.redAccent)),
                      );
                    }
                    final engines = snapshot.data ?? const <UnrealEngineInfo>[];
                    if (engines.isEmpty) {
                      return const Padding(
                        padding: EdgeInsets.all(8.0),
                        child: Text('No engines found'),
                      );
                    }
                    return GridView.builder(
                      shrinkWrap: true,
                      physics: const NeverScrollableScrollPhysics(),
                      itemCount: engines.length,
                      gridDelegate: SliverGridDelegateWithFixedCrossAxisCount(
                        crossAxisCount: count,
                        mainAxisSpacing: spacing,
                        crossAxisSpacing: spacing,
                        childAspectRatio: 0.78,
                      ),
                      itemBuilder: (context, index) {
                        final e = engines[index];
                        return _ProjectTile(
                          name: e.name,
                          version: e.version.isEmpty ? 'unknown' : 'UE ${e.version}',
                          color: Color.lerp(
                            const Color(0xFF1F2933),
                            cs.primary,
                            (index % 5) / 5.0,
                          )!,
                          onTap: () async {
                            if (_opening) return;
                            if (e.version.isEmpty) {
                              ScaffoldMessenger.of(context).showSnackBar(
                                const SnackBar(content: Text('Cannot open Unreal Engine: version is unknown')),
                              );
                              return;
                            }
                            setState(() => _opening = true);
                            try {
                              final result = await _api.openUnrealEngine(version: e.version);
                              if (!mounted) return;
                              ScaffoldMessenger.of(context).showSnackBar(
                                SnackBar(content: Text(result.message.isNotEmpty ? result.message : (result.launched ? 'Launched Unreal Engine' : 'Failed to launch Unreal Engine'))),
                              );
                            } catch (err) {
                              if (!mounted) return;
                              ScaffoldMessenger.of(context).showSnackBar(
                                SnackBar(content: Text('Error opening Unreal Engine: $err')),
                              );
                            } finally {
                              if (mounted) setState(() => _opening = false);
                            }
                          },
                        );
                      },
                    );
                  },
                );
              },
            ),
            const SizedBox(height: 24),
            // My Projects grid (kept)
            Text(
              'My Projects',
              style: Theme.of(context).textTheme.titleMedium?.copyWith(
                    fontWeight: FontWeight.w700,
                  ),
            ),
            const SizedBox(height: 12),
            LayoutBuilder(
              builder: (context, constraints) {
                const tileMinWidth = 140.0;
                const spacing = 12.0;
                final count = (constraints.maxWidth / (tileMinWidth + spacing))
                    .floor()
                    .clamp(1, 6);
                return FutureBuilder<List<UnrealProjectInfo>>(
                  future: _projectsFuture,
                  builder: (context, snapshot) {
                    if (snapshot.connectionState == ConnectionState.waiting) {
                      return const Center(child: Padding(padding: EdgeInsets.all(24), child: CircularProgressIndicator()));
                    }
                    if (snapshot.hasError) {
                      return Padding(
                        padding: const EdgeInsets.all(8.0),
                        child: Text('Failed to load projects: ${snapshot.error}', style: const TextStyle(color: Colors.redAccent)),
                      );
                    }
                    final projects = snapshot.data ?? const <UnrealProjectInfo>[];
                    if (projects.isEmpty) {
                      return const Padding(
                        padding: EdgeInsets.all(8.0),
                        child: Text('No projects found'),
                      );
                    }
                    return GridView.builder(
                      shrinkWrap: true,
                      physics: const NeverScrollableScrollPhysics(),
                      itemCount: projects.length,
                      gridDelegate: SliverGridDelegateWithFixedCrossAxisCount(
                        crossAxisCount: count,
                        mainAxisSpacing: spacing,
                        crossAxisSpacing: spacing,
                        childAspectRatio: 0.78,
                      ),
                      itemBuilder: (context, index) {
                        final p = projects[index];
                        return _ProjectTile(
                          name: p.name.isEmpty ? p.uprojectFile.split('/').last : p.name,
                          version: p.engineVersion.isNotEmpty ? 'UE ${p.engineVersion}' : 'UE unknown',
                          color: Color.lerp(
                            const Color(0xFF1F2933),
                            cs.primary,
                            (index % 5) / 5.0,
                          )!,
                          onTap: () async {
                            if (_opening) return;
                            setState(() => _opening = true);
                            try {
                              // Choose engine: use last item from sorted list (assumed highest version)
                              String? version;
                              if (_engines.isNotEmpty) {
                                version = _engines.last.version.isNotEmpty ? _engines.last.version : null;
                              }
                              if (version == null) {
                                if (!mounted) return;
                                ScaffoldMessenger.of(context).showSnackBar(
                                  const SnackBar(content: Text('No installed Unreal Engine version found')),
                                );
                              } else {
                                final result = await _api.openUnrealProject(
                                  project: p.uprojectFile.isNotEmpty ? p.uprojectFile : p.path,
                                  version: version,
                                );
                                if (!mounted) return;
                                ScaffoldMessenger.of(context).showSnackBar(
                                  SnackBar(content: Text(result.message.isNotEmpty ? result.message : (result.launched ? 'Launched Unreal Editor' : 'Failed to launch'))),
                                );
                              }
                            } catch (e) {
                              if (!mounted) return;
                              ScaffoldMessenger.of(context).showSnackBar(
                                SnackBar(content: Text('Error opening project: $e')),
                              );
                            } finally {
                              if (mounted) setState(() => _opening = false);
                            }
                          },
                        );
                      },
                    );
                  },
                );
              },
            ),
            const SizedBox(height: 24),
            // Header row for filters/actions
            Row(
              children: [
                Text(
                  'Fab Library',
                  style: Theme.of(context).textTheme.titleMedium?.copyWith(
                        fontWeight: FontWeight.w700,
                      ),
                ),
                const SizedBox(width: 16),
                // Search bar
                Expanded(
                  child: ConstrainedBox(
                    constraints: const BoxConstraints(maxWidth: 420),
                    child: TextField(
                      controller: _searchController,
                      onChanged: (v) => setState(() => _query = v),
                      decoration: InputDecoration(
                        prefixIcon: const Icon(Icons.search),
                        hintText: 'Search assets...',
                        isDense: true,
                        border: const OutlineInputBorder(),
                        suffixIcon: _query.isNotEmpty
                            ? IconButton(
                                tooltip: 'Clear',
                                icon: const Icon(Icons.clear),
                                onPressed: () {
                                  _searchController.clear();
                                  setState(() => _query = '');
                                },
                              )
                            : null,
                      ),
                    ),
                  ),
                ),
                const SizedBox(width: 16),
                // Version filter dropdown
                FutureBuilder<List<FabAsset>>(
                  future: _fabFuture,
                  builder: (context, snapshot) {
                    final assets = snapshot.data ?? const <FabAsset>[];
                    // collect unique versions like '5.6', '5.5'
                    final versions = <String>{};
                    for (final a in assets) {
                      for (final pv in a.projectVersions) {
                        for (final ev in pv.engineVersions) {
                          final parts = ev.split('_');
                          if (parts.length > 1) {
                            versions.add(parts[1]);
                          }
                        }
                      }
                    }
                    int cmp(String a, String b) {
                      int parseOrZero(String s) => int.tryParse(s) ?? 0;
                      final as = a.split('.');
                      final bs = b.split('.');
                      final amaj = parseOrZero(as.isNotEmpty ? as[0] : '0');
                      final amin = parseOrZero(as.length > 1 ? as[1] : '0');
                      final bmaj = parseOrZero(bs.isNotEmpty ? bs[0] : '0');
                      final bmin = parseOrZero(bs.length > 1 ? bs[1] : '0');
                      if (amaj != bmaj) return bmaj.compareTo(amaj);
                      return bmin.compareTo(amin);
                    }
                    final sorted = versions.toList()..sort(cmp);
                    final items = <DropdownMenuItem<String>>[
                      const DropdownMenuItem<String>(
                        value: '',
                        child: Text('All versions'),
                      ),
                      ...sorted.map((v) => DropdownMenuItem<String>(
                            value: v,
                            child: Text('UE $v'),
                          )),
                    ];
                    return Row(
                      mainAxisSize: MainAxisSize.min,
                      children: [
                        ConstrainedBox(
                          constraints: const BoxConstraints(maxWidth: 200),
                          child: DropdownButtonFormField<String>(
                            value: _versionFilter.isEmpty ? '' : _versionFilter,
                            items: items,
                            onChanged: (v) => setState(() => _versionFilter = v ?? ''),
                            decoration: const InputDecoration(
                              isDense: true,
                              labelText: 'Filter by version',
                              border: OutlineInputBorder(),
                            ),
                          ),
                        ),
                        const SizedBox(width: 12),
                        FilterChip(
                          label: const Text('Complete projects only'),
                          selected: _onlyCompleteProjects,
                          onSelected: (v) => setState(() => _onlyCompleteProjects = v),
                        ),
                        const SizedBox(width: 12),
                        ConstrainedBox(
                          constraints: const BoxConstraints(maxWidth: 220),
                          child: DropdownButtonFormField<AssetSortMode>(
                            value: _sortMode,
                            items: const [
                              DropdownMenuItem(value: AssetSortMode.newerUEFirst, child: Text('Sort: Newer UE first')),
                              DropdownMenuItem(value: AssetSortMode.olderUEFirst, child: Text('Sort: Older UE first')),
                              DropdownMenuItem(value: AssetSortMode.alphaAZ, child: Text('Sort: Alphabetical A–Z')),
                              DropdownMenuItem(value: AssetSortMode.alphaZA, child: Text('Sort: Alphabetical Z–A')),
                            ],
                            onChanged: (v) => setState(() => _sortMode = v ?? AssetSortMode.newerUEFirst),
                            decoration: const InputDecoration(
                              isDense: true,
                              labelText: 'Sort by',
                              border: OutlineInputBorder(),
                            ),
                          ),
                        ),
                      ],
                    );
                  },
                ),
              ],
            ),
            const SizedBox(height: 16),
            // Responsive grid (now non-scrollable; page scrolls instead)
            Container(
              decoration: BoxDecoration(
                color: cs.surface,
                borderRadius: BorderRadius.circular(12),
                border: Border.all(color: const Color(0xFF1A2027)),
              ),
              child: LayoutBuilder(
                builder: (context, constraints) {
                  const minTileWidth = 320.0;
                  const spacing = 16.0;
                  final crossAxisCount =
                      (constraints.maxWidth / (minTileWidth + spacing))
                          .floor()
                          .clamp(3, 5);
                  return FutureBuilder<List<FabAsset>>(
                    future: _fabFuture,
                    builder: (context, snapshot) {
                      if (snapshot.connectionState == ConnectionState.waiting) {
                        return const Padding(
                          padding: EdgeInsets.all(24.0),
                          child: Center(child: CircularProgressIndicator()),
                        );
                      }
                      if (snapshot.hasError) {
                        return Padding(
                          padding: const EdgeInsets.all(16.0),
                          child: Text('Failed to load Fab library: ${snapshot.error}', style: const TextStyle(color: Colors.redAccent)),
                        );
                      }
                      final assets = snapshot.data ?? const <FabAsset>[];
                      final q = _query.trim().toLowerCase();
                      List<FabAsset> filtered = q.isEmpty
                          ? assets
                          : assets.where((a) {
                              final title = a.title.toLowerCase();
                              final id = a.assetId.toLowerCase();
                              final ns = a.assetNamespace.toLowerCase();
                              final label = a.shortEngineLabel.toLowerCase();
                              return title.contains(q) || id.contains(q) || ns.contains(q) || label.contains(q);
                            }).toList();
                      // Apply COMPLETE_PROJECT filter if enabled
                      if (_onlyCompleteProjects) {
                        filtered = filtered.where((a) => a.isCompleteProject).toList();
                      }
                      // Apply version filter if set
                      final vf = _versionFilter.trim();
                      if (vf.isNotEmpty) {
                        bool supportsVersion(FabAsset a, String v) {
                          for (final pv in a.projectVersions) {
                            for (final ev in pv.engineVersions) {
                              final parts = ev.split('_');
                              if (parts.length > 1 && parts[1] == v) return true;
                              // Also handle plain '5.6' without prefix
                              if (parts.length == 1 && ev == v) return true;
                            }
                          }
                          return false;
                        }
                        filtered = filtered.where((a) => supportsVersion(a, vf)).toList();
                      }
                      // Sort according to user selection
                      int versionScoreOf(String ver) {
                        // Accept formats like '5.6', '5', 'UE_5.6'
                        String v = ver;
                        if (v.contains('_')) {
                          final p = v.split('_');
                          if (p.length > 1) v = p[1];
                        }
                        final parts = v.split('.');
                        int major = 0;
                        int minor = 0;
                        if (parts.isNotEmpty) major = int.tryParse(parts[0]) ?? 0;
                        if (parts.length > 1) minor = int.tryParse(parts[1]) ?? 0;
                        return major * 100 + minor; // 5.6 -> 506
                      }
                      int maxSupportedVersionScore(FabAsset a) {
                        int maxScore = -1;
                        for (final pv in a.projectVersions) {
                          for (final ev in pv.engineVersions) {
                            final s = versionScoreOf(ev);
                            if (s > maxScore) maxScore = s;
                          }
                        }
                        return maxScore;
                      }
                      int titleCompare(FabAsset a, FabAsset b) => a.title.toLowerCase().compareTo(b.title.toLowerCase());
                      switch (_sortMode) {
                        case AssetSortMode.alphaAZ:
                          filtered.sort(titleCompare);
                          break;
                        case AssetSortMode.alphaZA:
                          filtered.sort((a, b) => titleCompare(b, a));
                          break;
                        case AssetSortMode.olderUEFirst:
                          filtered.sort((a, b) {
                            final av = maxSupportedVersionScore(a);
                            final bv = maxSupportedVersionScore(b);
                            if (av != bv) return av.compareTo(bv); // older first
                            return titleCompare(a, b);
                          });
                          break;
                        case AssetSortMode.newerUEFirst:
                        default:
                          filtered.sort((a, b) {
                            final av = maxSupportedVersionScore(a);
                            final bv = maxSupportedVersionScore(b);
                            if (av != bv) return bv.compareTo(av); // newer first
                            return titleCompare(a, b);
                          });
                      }
                      if (filtered.isEmpty) {
                        return const Padding(
                          padding: EdgeInsets.all(16.0),
                          child: Text('No assets match your search.'),
                        );
                      }
                      return _FabAssetsGrid(
                        key: _fabKey,
                        assets: filtered,
                        crossAxisCount: crossAxisCount,
                        spacing: spacing,
                        onLoadMore: _requestMoreFabItems,
                        onProjectsChanged: _refreshProjects,
                        onFabListChanged: _refreshFabList,
                      );
                    },
                  );
                },
              ),
            ),
            const SizedBox(height: 16),
          ],
        ),
      ),
    );
  }
}

class _FabAssetsGrid extends StatefulWidget {
  final VoidCallback? onLoadMore;
  final List<FabAsset> assets;
  final int crossAxisCount;
  final double spacing;
  final VoidCallback? onProjectsChanged;
  final VoidCallback? onFabListChanged;
  const _FabAssetsGrid({Key? key, required this.assets, required this.crossAxisCount, required this.spacing, this.onLoadMore, this.onProjectsChanged, this.onFabListChanged}) : super(key: key);

  @override
  State<_FabAssetsGrid> createState() => _FabAssetsGridState();
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

class _FabAssetsGridState extends State<_FabAssetsGrid> {
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
        if (best == null || _cmpUeVersions(normalized, best!) > 0) {
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

  // Progress dialog for long-running jobs via WebSocket events
  Future<void> _showJobProgressDialog({required String jobId, required String title}) async {
    double? percent;
    String message = 'Starting...';
    StreamSubscription? sub;
    try {
      await showDialog<void>(
        context: context,
        barrierDismissible: false,
        builder: (ctx) {
          return StatefulBuilder(
            builder: (ctx, setStateSB) {
              sub ??= _api.progressEvents(jobId).listen((ev) async { 
                // Debug: log event as interpreted by UI
                final ptxtRaw = ev.progress == null ? 'null' : ev.progress!.toStringAsFixed(3);
                // ignore: avoid_print
                print('[UI][progress] job=$jobId phase=${ev.phase} message="${ev.message}" progress(raw)=$ptxtRaw');

                // Normalize progress to 0..100 regardless of backend scale (0..1 or 0..100)
                double? normalized;
                final raw = ev.progress;
                if (raw != null) {
                  if (raw.isNaN) {
                    normalized = null; // treat as unknown
                  } else if (raw <= 1.01) {
                    normalized = (raw * 100).clamp(0, 100);
                  } else {
                    normalized = raw.clamp(0, 100);
                  }
                }

                // Fallback/override: derive progress from messages like "123 / 5851"
                double? fromCounts;
                try {
                  final m = RegExp(r'\b(\d+)\s*/\s*(\d+)\b').firstMatch(ev.message);
                  if (m != null) {
                    final cur = double.tryParse(m.group(1) ?? '');
                    final tot = double.tryParse(m.group(2) ?? '');
                    if (cur != null && tot != null && tot > 0 && cur >= 0 && cur <= tot) {
                      fromCounts = ((cur / tot) * 100).clamp(0, 100);
                    }
                  }
                } catch (_) {}
                // Prefer count-derived progress when available (more reliable for downloading phases)
                final effective = fromCounts ?? normalized;

                setStateSB(() {
                  // Update in-dialog progress state
                  percent = effective; // percent represents 0..100 scale now
                  message = ev.message.isNotEmpty ? ev.message : ev.phase;
                });
                // Update OS-level window/taskbar progress if available
                if (effective != null) {
                  final norm01 = (effective / 100.0);
                  try { await windowManager.setProgressBar(norm01); } catch (_) {}
                }
                // Auto-close when we clearly reach 100% or receive a done phase
                if ((effective != null && effective >= 100.0) || ev.phase.toLowerCase() == 'done' || ev.phase.toLowerCase() == 'completed') {
                  try { await windowManager.setProgressBar(-1); } catch (_) {}
                  if (Navigator.of(ctx).canPop()) {
                    Navigator.of(ctx).pop();
                  }
                }
              }, onError: (_) {
                // Ignore errors; dialog can be closed manually or by caller
              });
              final p = (percent ?? 0).clamp(0, 100);
              return AlertDialog(
                title: Text(title),
                content: SizedBox(
                  width: 420,
                  child: Column(
                    mainAxisSize: MainAxisSize.min,
                    crossAxisAlignment: CrossAxisAlignment.stretch,
                    children: [
                      LinearProgressIndicator(value: percent != null ? (p / 100.0) : null),
                      const SizedBox(height: 12),
                      Row(
                        children: [
                          Expanded(child: Text(message, overflow: TextOverflow.ellipsis)),
                          // if (percent != null) Text('${p.toStringAsFixed(0)}%'), // Rounds up from 99.5 to 100
                          if (percent != null) Text('${p.floor().toString()}%'), // Test as this should keep it at 99% until 100%
                        ],
                      ),
                    ],
                  ),
                ),
              );
            },
          );
        },
      );
    }
    finally {
      await sub?.cancel();
      try { await windowManager.setProgressBar(-1); } catch (_) {}
    }
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
    final uri = Uri.tryParse(url);
    if (uri == null) {
      if (!mounted) return;
      ScaffoldMessenger.of(context).showSnackBar(
        const SnackBar(content: Text('Invalid URL')),
      );
      return;
    }
    try {
      final ok = await launchUrl(uri, mode: LaunchMode.externalApplication);
      if (!ok && mounted) {
        ScaffoldMessenger.of(context).showSnackBar(
          const SnackBar(content: Text('Could not open link')),
        );
      }
    } catch (e) {
      if (mounted) {
        ScaffoldMessenger.of(context).showSnackBar(
          SnackBar(content: Text('Failed to launch: $e')),
        );
      }
    }
  }

  Future<void> _showAssetGalleryDialog(BuildContext context, FabAsset a) async {
    final images = a.images;
    int index = 0;
    await showDialog<void>(
      context: context,
      builder: (ctx) {
        return StatefulBuilder(
          builder: (ctx, setStateSB) {
            bool working = false;
            return AlertDialog(
              contentPadding: const EdgeInsets.all(12),
              title: Text(a.title.isNotEmpty ? a.title : a.assetId),
              content: Builder(
                builder: (context) {
                  final size = MediaQuery.of(context).size;
                  final dialogWidth = (size.width * 0.9).clamp(300.0, 900.0);
                  final dialogHeight = (size.height * 0.9).clamp(300.0, 700.0);
                  final cs = Theme.of(context).colorScheme;
                  // Compose extra meta
                  String dist = a.distributionMethod.isNotEmpty ? a.distributionMethod : '';
                  String idNs = '${a.assetNamespace}/${a.assetId}';
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
                                      return Image.network(
                                        url,
                                        fit: BoxFit.cover,
                                        errorBuilder: (c, e, s) => const Center(child: Icon(Icons.broken_image, size: 48, color: Color(0xFF9AA4AF))),
                                        loadingBuilder: (c, child, progress) {
                                          if (progress == null) return child;
                                          return const Center(child: SizedBox(width: 32, height: 32, child: CircularProgressIndicator(strokeWidth: 2)));
                                        },
                                      );
                                    },
                                  ),
                          ),
                        ),
                        const SizedBox(height: 8),
                        if (images.length > 1)
                          Row(
                            mainAxisAlignment: MainAxisAlignment.center,
                            children: List.generate(images.length, (i) {
                              final active = i == index;
                              return Container(
                                margin: const EdgeInsets.symmetric(horizontal: 4),
                                width: active ? 10 : 8,
                                height: active ? 10 : 8,
                                decoration: BoxDecoration(
                                  color: active ? Theme.of(context).colorScheme.primary : const Color(0xFF39424C),
                                  shape: BoxShape.circle,
                                ),
                              );
                            }),
                          ),
                        const SizedBox(height: 12),
                        // Meta row
                        Wrap(
                          spacing: 8,
                          runSpacing: 8,
                          crossAxisAlignment: WrapCrossAlignment.center,
                          children: [
                            if (dist.isNotEmpty)
                              Container(
                                padding: const EdgeInsets.symmetric(horizontal: 8, vertical: 4),
                                decoration: BoxDecoration(
                                  color: cs.surfaceVariant,
                                  borderRadius: BorderRadius.circular(6),
                                  border: Border.all(color: const Color(0xFF1A2027)),
                                ),
                                child: Text(dist, style: Theme.of(context).textTheme.labelSmall),
                              ),
                            if (a.shortEngineLabel.isNotEmpty)
                              Text(a.shortEngineLabel, style: Theme.of(context).textTheme.bodySmall),
                            Text(idNs, style: Theme.of(context).textTheme.bodySmall?.copyWith(color: cs.onSurfaceVariant)),
                          ],
                        ),
                        const SizedBox(height: 12),
                        Expanded(
                          flex: 0,
                          child: ConstrainedBox(
                            constraints: const BoxConstraints(maxHeight: 140),
                            child: SingleChildScrollView(
                              child: Text(
                                a.description.isNotEmpty ? a.description : 'No description provided.',
                              ),
                            ),
                          ),
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
                ),
                if ((a.url ?? '').isNotEmpty)
                  TextButton.icon(
                    onPressed: () => _launchExternal(a.url!),
                    icon: const Icon(Icons.open_in_new),
                    label: const Text('Open listing'),
                  ),
                const SizedBox(width: 8),
                FilledButton.icon(
                  onPressed: working
                      ? null
                      : () async {
                          setStateSB(() => working = true);
                          try {
                            if (a.isCompleteProject) {
                              // Check installed support and prompt create
                              try {
                                final hasSupport = await _projectHasSupportInstalled(a);
                                if (!hasSupport) {
                                  final latest = _maxSupportedForAsset(a) ?? '';
                                  if (mounted) {
                                    await showDialog<void>(
                                      context: context,
                                      builder: (dctx) => AlertDialog(
                                        title: const Text('No supported Unreal Engine installed'),
                                        content: Text(latest.isNotEmpty
                                            ? 'There are no installed versions of Unreal Engine supported by this project. Please download the latest supported version: UE $latest.'
                                            : 'There are no installed versions of Unreal Engine supported by this project. Please install a supported version.'),
                                        actions: [
                                          TextButton(onPressed: () => Navigator.of(dctx).pop(), child: const Text('OK')),
                                        ],
                                      ),
                                    );
                                  }
                                  setStateSB(() => working = false);
                                  return;
                                }
                              } catch (_) {}
                              final params = await _promptCreateProject(context, a);
                              if (params == null) { setStateSB(() => working = false); return; }
                              final jobId = _makeJobId();
                              final dlg = _showJobProgressDialog(jobId: jobId, title: 'Creating project...');
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
                              await dlg.catchError((_){ });
                              if (mounted) {
                                final ok = res.ok;
                                final msg = res.message.isNotEmpty ? res.message : (ok ? 'OK' : 'Failed');
                                ScaffoldMessenger.of(context).showSnackBar(SnackBar(content: Text(msg)));
                                if (ok && !params.dryRun) {
                                  widget.onProjectsChanged?.call();
                                }
                              }
                            } else {
                              final params = await _promptImport(context, a);
                              if (params == null) { setStateSB(() => working = false); return; }
                              final jobId = _makeJobId();
                              final dlg = _showJobProgressDialog(jobId: jobId, title: 'Importing asset...');
                              final res = await _api.importAsset(
                                assetName: a.title.isNotEmpty ? a.title : a.assetId,
                                project: params.project,
                                targetSubdir: params.targetSubdir,
                                overwrite: params.overwrite,
                                jobId: jobId,
                              );
                              if (mounted) {
                                final nav = Navigator.of(context, rootNavigator: true);
                                if (nav.canPop()) nav.pop();
                              }
                              await dlg.catchError((_){ });
                              if (mounted) {
                                ScaffoldMessenger.of(context).showSnackBar(
                                  SnackBar(content: Text(res.message.isNotEmpty ? res.message : (res.success ? 'Import started' : 'Import failed'))),
                                );
                              }
                            }
                          } catch (e) {
                            if (mounted) {
                              ScaffoldMessenger.of(context).showSnackBar(SnackBar(content: Text('Action failed: $e')));
                            }
                          } finally {
                            if (mounted) setStateSB(() => working = false);
                          }
                        },
                  icon: Icon(a.isCompleteProject ? Icons.add : Icons.download),
                  label: Text(a.isCompleteProject ? 'Create Project' : 'Import Asset'),
                ),
              ],
            );
          },
        );
      },
    );
  }

  Future<_ImportParams?> _promptImport(BuildContext context, FabAsset asset) async {
    final subdirCtrl = TextEditingController(text: '');
    bool overwrite = false;

    String? selectedProject; // will hold selected .uproject path

    final result = await showDialog<_ImportParams>(
      context: context,
      builder: (ctx) {
        return AlertDialog(
          title: const Text('Import Asset'),
          content: SingleChildScrollView(
            child: Column(
              mainAxisSize: MainAxisSize.min,
              children: [
                FutureBuilder<List<UnrealProjectInfo>>(
                  future: _api.listUnrealProjects(),
                  builder: (context, snapshot) {
                    if (snapshot.connectionState == ConnectionState.waiting) {
                      return const Padding(
                        padding: EdgeInsets.symmetric(vertical: 8.0),
                        child: Row(
                          children: [
                            SizedBox(width: 20, height: 20, child: CircularProgressIndicator(strokeWidth: 2)),
                            SizedBox(width: 12),
                            Text('Loading projects...'),
                          ],
                        ),
                      );
                    }
                    if (snapshot.hasError) {
                      return Padding(
                        padding: const EdgeInsets.symmetric(vertical: 8.0),
                        child: Text('Failed to load projects: ${snapshot.error}', style: const TextStyle(color: Colors.red)),
                      );
                    }
                    final projects = snapshot.data ?? const <UnrealProjectInfo>[];
                    if (projects.isEmpty) {
                      return const Padding(
                        padding: EdgeInsets.symmetric(vertical: 8.0),
                        child: Text('No Unreal projects found.'),
                      );
                    }
                    // Default to first project if none selected yet
                    selectedProject ??= projects.first.uprojectFile.isNotEmpty
                        ? projects.first.uprojectFile
                        : projects.first.path;
                    return DropdownButtonFormField<String>(
                      value: selectedProject,
                      items: projects.map((p) {
                        final value = p.uprojectFile.isNotEmpty ? p.uprojectFile : p.path;
                        final label = p.name.isNotEmpty ? p.name : value;
                        return DropdownMenuItem<String>(
                          value: value,
                          child: Text(label, overflow: TextOverflow.ellipsis),
                        );
                      }).toList(),
                      onChanged: (v) {
                        selectedProject = v;
                      },
                      decoration: const InputDecoration(
                        labelText: 'Select Project',
                      ),
                    );
                  },
                ),
                const SizedBox(height: 8),
                TextField(
                  controller: subdirCtrl,
                  decoration: const InputDecoration(
                    labelText: 'Target subfolder (optional)',
                    hintText: 'e.g., Imported/Industry',
                  ),
                ),
                const SizedBox(height: 8),
                StatefulBuilder(
                  builder: (context, setState) {
                    return CheckboxListTile(
                      contentPadding: EdgeInsets.zero,
                      title: const Text('Overwrite existing files'),
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
                final project = (selectedProject ?? '').trim();
                final subdir = subdirCtrl.text.trim();
                if (project.isEmpty) {
                  ScaffoldMessenger.of(ctx).showSnackBar(
                    const SnackBar(content: Text('Please select a project')),
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
  void didUpdateWidget(covariant _FabAssetsGrid oldWidget) {
    super.didUpdateWidget(oldWidget);
    if (oldWidget.assets.length != widget.assets.length) {
      // Reset to first page when data changes
      _page = 0;
    }
    // Clamp page if fewer total pages now
    final totalPages = (widget.assets.isEmpty) ? 1 : ((widget.assets.length - 1) ~/ _pageSize + 1);
    if (_page >= totalPages) _page = totalPages - 1;
  }

  @override
  Widget build(BuildContext context) {
    final total = widget.assets.length;
    final totalPages = total == 0 ? 1 : ((total - 1) ~/ _pageSize + 1);
    final start = (_page * _pageSize).clamp(0, total);
    final end = (start + _pageSize).clamp(0, total);
    final count = end - start;

    Widget grid = GridView.builder(
      padding: const EdgeInsets.all(16),
      physics: const NeverScrollableScrollPhysics(),
      shrinkWrap: true,
      gridDelegate: SliverGridDelegateWithFixedCrossAxisCount(
        crossAxisCount: widget.crossAxisCount,
        mainAxisSpacing: widget.spacing,
        crossAxisSpacing: widget.spacing,
        childAspectRatio: 2.4,
      ),
      itemCount: count,
      itemBuilder: (context, index) {
        final globalIndex = start + index;
        final a = widget.assets[globalIndex];
        // Determine if this COMPLETE_PROJECT item lacks any compatible installed engines
        final supportedSet = _supportedMajorMinorSet(a);
        final warnNoSupport = a.isCompleteProject && (_installedMmSet != null) && _installedMmSet!.intersection(supportedSet).isEmpty;
        return FabLibraryItem(
          title: a.title.isNotEmpty ? a.title : a.assetId,
          sizeLabel: a.shortEngineLabel.isNotEmpty ? a.shortEngineLabel : '${a.assetNamespace}/${a.assetId}',
          isCompleteProject: a.isCompleteProject,
          downloaded: a.anyDownloaded,
          isBusy: _busy.contains(globalIndex),
          thumbnailUrl: _pickThumbnailUrl(a),
          useWarningStyle: warnNoSupport,
          onTap: () => _showAssetGalleryDialog(context, a),
          onPrimaryPressed: () async {
            if (a.isCompleteProject) {
              // If no installed UE versions match this project's supported versions, warn and abort
              try {
                final hasSupport = await _projectHasSupportInstalled(a);
                if (!hasSupport) {
                  final latest = _maxSupportedForAsset(a) ?? '';
                  if (mounted) {
                    await showDialog<void>(
                      context: context,
                      builder: (ctx) => AlertDialog(
                        title: const Text('No supported Unreal Engine installed'),
                        content: Text(latest.isNotEmpty
                            ? 'There are no installed versions of Unreal Engine supported by this project. Please download the latest supported version: UE $latest.'
                            : 'There are no installed versions of Unreal Engine supported by this project. Please install a supported version.'),
                        actions: [
                          TextButton(onPressed: () => Navigator.of(ctx).pop(), child: const Text('OK')),
                        ],
                      ),
                    );
                  }
                  return;
                }
              } catch (_) {
                // If check fails, continue to prompt; user can decide engine path manually
              }

              final params = await _promptCreateProject(context, a);
              if (params == null) return;
              setState(() => _busy.add(globalIndex));
              try {
                final jobId = _makeJobId();
                                // Start listening to progress in a dialog
                                final dlg = _showJobProgressDialog(jobId: jobId, title: 'Creating project...');
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
                // Close progress dialog if still open (in case no events)
                if (mounted) {
                  final nav = Navigator.of(context, rootNavigator: true);
                  if (nav.canPop()) {
                    nav.pop();
                  }
                }
                // Ensure dialog future completes
                await dlg.catchError((_){});
                if (!mounted) return;
                final ok = res.ok;
                final msg = res.message.isNotEmpty ? res.message : (ok ? 'OK' : 'Failed');
                ScaffoldMessenger.of(context).showSnackBar(
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
                ScaffoldMessenger.of(context).showSnackBar(
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
              final dlg = _showJobProgressDialog(jobId: jobId, title: 'Importing asset...');
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
              await dlg.catchError((_){});
              if (!mounted) return;
              final msg = result.message.isNotEmpty ? result.message : (result.success ? 'Import started' : 'Import failed');
              ScaffoldMessenger.of(context).showSnackBar(
                SnackBar(content: Text(msg)),
              );
              if (result.success) {
                // Refresh Fab list so the downloaded indicator updates
                widget.onFabListChanged?.call();
              }
            } catch (e) {
              if (!mounted) return;
              ScaffoldMessenger.of(context).showSnackBar(
                SnackBar(content: Text('Failed to import: $e')),
              );
            } finally {
              if (mounted) setState(() => _busy.remove(globalIndex));
            }
          },
        );
      },
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
}

// ... existing code ...
class _ProjectTile extends StatelessWidget {
  final String name;
  final String version;
  final Color color;
  final VoidCallback? onTap;

  const _ProjectTile({
    required this.name,
    required this.version,
    required this.color,
    this.onTap,
  });

  @override
  Widget build(BuildContext context) {
    final cs = Theme.of(context).colorScheme;
    return Column(
      crossAxisAlignment: CrossAxisAlignment.start,
      children: [
        // Square thumbnail with version tag
        AspectRatio(
          aspectRatio: 1,
          child: Stack(
            children: [
              Container(
                decoration: BoxDecoration(
                  color: color,
                  borderRadius: BorderRadius.circular(10),
                  border: Border.all(color: const Color(0xFF1A2027)),
                ),
                // Placeholder for a future screenshot/thumbnail
              ),
              Positioned(
                right: 8,
                bottom: 8,
                child: Container(
                  padding:
                      const EdgeInsets.symmetric(horizontal: 8, vertical: 4),
                  decoration: BoxDecoration(
                    color: cs.primary.withOpacity(0.9),
                    borderRadius: BorderRadius.circular(6),
                  ),
                  child: Text(
                    version,
                    style: const TextStyle(
                      fontSize: 11,
                      fontWeight: FontWeight.w700,
                    ),
                  ),
                ),
              ),
              // Full-surface tappable overlay with ripple
              Positioned.fill(
                child: Material(
                  color: Colors.transparent,
                  borderRadius: BorderRadius.circular(10),
                  child: InkWell(
                    borderRadius: BorderRadius.circular(10),
                    onTap: onTap,
                  ),
                ),
              ),
            ],
          ),
        ),
        const SizedBox(height: 8),
        Text(
          name,
          maxLines: 1,
          overflow: TextOverflow.ellipsis,
          style: Theme.of(context).textTheme.bodyMedium?.copyWith(
                fontWeight: FontWeight.w600,
              ),
        ),
      ],
    );
  }
}

