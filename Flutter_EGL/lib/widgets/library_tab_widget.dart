import 'package:flutter/material.dart';
import 'dart:async';
import 'components/fab_library_header.dart';
import 'components/unreal_engine_versions_list_widget.dart';
import '../services/api_service.dart';
import '../models/unreal.dart';
import '../models/fab.dart';
import '../theme/app_theme.dart';
import './components/unreal_engine_header.dart';
import 'components/fab_auth_card.dart';
import 'components/projects_grid_section.dart';
import 'components/fab_search_bar.dart';
import 'components/fab_version_filter_dropdown.dart';
import 'components/fab_complete_projects_filter.dart';
import 'components/fab_sort_by_dropdown.dart';
import 'components/fab_assets_list.dart';
import 'components/settings_dialog.dart';

class LibraryTab extends StatefulWidget {
  const LibraryTab({super.key});

  @override
  State<LibraryTab> createState() => _LibraryTabState();
}

enum AssetSortMode { newerUEFirst, olderUEFirst, alphaAZ, alphaZA }

class _LibraryTabState extends State<LibraryTab> {
  Widget _buildUnauthenticatedCard(BuildContext context, String authUrl, String? message) {
    return FabAuthCard(
      authUrl: authUrl,
      message: message,
      controller: _authCodeController,
      onSubmit: _submitAuthCode,
      isWorking: _authWorking,
    );
  }

  Future<void> _submitAuthCode() async {
    final code = _authCodeController.text.trim();
    if (code.isEmpty) {
      if (!mounted) return;
      ScaffoldMessenger.of(context).showSnackBar(const SnackBar(content: Text('Please paste the authorizationCode.')));
      return;
    }
    setState(() { _authWorking = true; });
    try {
      final ok = await _api.completeAuth(code);
      if (!mounted) return;
      if (ok) {
        _authCodeController.clear();
        ScaffoldMessenger.of(context).showSnackBar(const SnackBar(content: Text('Login successful. Loading your library...')));
        setState(() { _fabFuture = _api.getFabList(); });
      } else {
        ScaffoldMessenger.of(context).showSnackBar(const SnackBar(content: Text('Login failed. Please verify the code and try again.')));
      }
    } finally {
      if (mounted) setState(() { _authWorking = false; });
    }
  }

  final TextEditingController _searchController = TextEditingController();
  String _query = '';
  String _versionFilter = '';
  bool _onlyCompleteProjects = false;
  AssetSortMode _sortMode = AssetSortMode.newerUEFirst;
  final ScrollController _scrollController = ScrollController();
  final GlobalKey<FabAssetsListState> _fabKey = GlobalKey<FabAssetsListState>();
  late final ApiService _api;
  late Future<List<UnrealEngineInfo>> _enginesFuture;
  late Future<List<UnrealProjectInfo>> _projectsFuture;
  late Future<List<FabAsset>> _fabFuture;
  final TextEditingController _authCodeController = TextEditingController();
  bool _authWorking = false;

  // cache of engines for deciding version on open
  List<UnrealEngineInfo> _engines = const [];
  bool _refreshingFab = false;

  // Settings: user-configurable paths
  final TextEditingController _projectsDirCtrl = TextEditingController();
  final TextEditingController _enginesDirCtrl = TextEditingController();
  final TextEditingController _cacheDirCtrl = TextEditingController();
  final TextEditingController _downloadsDirCtrl = TextEditingController();

  @override
  void initState() {
    super.initState();
    _api = ApiService();
  }



  @override
  void dispose() {
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

    String? _appVersion;
    try {
      _appVersion = await _api.getVersion();
    } catch (_) {}

    await showLibrarySettingsDialog(
      context: context,
      projectsDirCtrl: _projectsDirCtrl,
      enginesDirCtrl: _enginesDirCtrl,
      cacheDirCtrl: _cacheDirCtrl,
      downloadsDirCtrl: _downloadsDirCtrl,
      refreshingFab: _refreshingFab,
      appVersion: _appVersion,
      onRefreshFabPressed: () async {
        setState(() {
          _refreshingFab = true;
          // Clear the current fab list to show progress indicator
          _fabFuture = Future.value(<FabAsset>[]);
        });
        try {
          final items = await _api.refreshFabList();
          if (mounted) {
            setState(() {
              _fabFuture = Future.value(items);
            });
            ScaffoldMessenger.of(context).showSnackBar(
              SnackBar(content: Text('Fab list refreshed (${items.length} items)')),
            );
          }
        } catch (e) {
          if (mounted) {
            ScaffoldMessenger.of(context).showSnackBar(
              SnackBar(content: Text('Failed to refresh Fab list: $e')),
            );
            // Restore original fab list on error
            setState(() {
              _fabFuture = _api.getFabList();
            });
          }
        } finally {
          if (mounted) {
            setState(() {
              _refreshingFab = false;
            });
          }
        }
      },
      onApplyPressed: _applyPaths,
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
            // const Divider(height: 24),
            UnrealEnginesHeader("Engine Versions"),
            const SizedBox(height: 10),
            UnrealEngineVersionsList<UnrealEngineInfo>(
              enginesFuture: _enginesFuture,
              nameOf: (e) => e.name,
              versionOf: (e) => e.version,
              openEngine: (version) async {
                final r = await _api.openUnrealEngine(version: version);
                return (launched: r.launched, message: r.message);
              },
              tileColorBuilder: (i) => AppPalette.varied(AppPalette.engineTileBase, i, cycle: 5, t: 0.2),
            ),
            // const SizedBox(height: 8),
            const Divider(height: 24),
            ProjectsList<UnrealProjectInfo, UnrealEngineInfo>(
              projectsFuture: _projectsFuture,
              engines: _engines,
              nameOf: (p) => p.name.isEmpty ? p.uprojectFile.split('/').last : p.name,
              projectPathOf: (p) => p.uprojectFile.isNotEmpty ? p.uprojectFile : p.path,
              engineVersionOf: (p) => p.engineVersion,
              engineVersionOfEngine: (e) => e.version,
              openProject: ({required String project, required String version}) async {
                final r = await _api.openUnrealProject(project: project, version: version);
                return (launched: r.launched, message: r.message);
              },
              setProjectVersion: ({required String project, required String version}) async {
                final r = await _api.setUnrealProjectVersion(project: project, version: version);
                return (ok: r.ok, message: r.message);
              },
              refreshProjects: () {
                setState(() {
                  _projectsFuture = _api.listUnrealProjects();
                });
              },
              tileColorBuilder: (i) => AppPalette.varied(AppPalette.projectTileBase, i, cycle: 5, t: 0.25),
            ),
            const SizedBox(height: 24),
            // Header row for filters/actions
            const Divider(height: 24),
            Row(
              children: [
                FabLibraryHeader("Fab Library"),
                const SizedBox(width: 16),
                // Search bar
                Expanded(
                  child: FabSearchBar(
                    controller: _searchController,
                    query: _query,
                    onChanged: (v) => setState(() => _query = v),
                    onClear: () {
                      _searchController.clear();
                      setState(() => _query = '');
                    },
                  ),
                ),
                const SizedBox(width: 16),
                FabVersionFilterDropdown(
                  fabFuture: _fabFuture,
                  value: _versionFilter,
                  onChanged: (v) => setState(() => _versionFilter = v ?? ''),
                ),
                const SizedBox(width: 12),
                FabCompleteProjectsFilter(
                  selected: _onlyCompleteProjects,
                  onChanged: (v) => setState(() => _onlyCompleteProjects = v),
                ),
                const SizedBox(width: 12),
                FabSortByDropdown<AssetSortMode>(
                  value: _sortMode,
                  items: const [
                    DropdownMenuItem(value: AssetSortMode.newerUEFirst, child: Text('Sort: Newer UE first')),
                    DropdownMenuItem(value: AssetSortMode.olderUEFirst, child: Text('Sort: Older UE first')),
                    DropdownMenuItem(value: AssetSortMode.alphaAZ, child: Text('Sort: Alphabetical A–Z')),
                    DropdownMenuItem(value: AssetSortMode.alphaZA, child: Text('Sort: Alphabetical Z–A')),
                  ],
                  onChanged: (v) => setState(() => _sortMode = v ?? AssetSortMode.newerUEFirst),
                ),
              ],
            ),
            const SizedBox(height: 16),
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
                        final err = snapshot.error;
                        // Preferred path: explicit unauthenticated exception from ApiService
                        if (err is UnauthenticatedException) {
                          final authUrl = err.authUrl.isNotEmpty ? err.authUrl : 'https://www.epicgames.com/id/login';
                          return _buildUnauthenticatedCard(context, authUrl, err.message);
                        }
                        // Fallback path: detect 401/unauthorized style errors and show auth UI instead of a raw error
                        final errStr = (err?.toString() ?? '').toLowerCase();
                        final looksUnauthed = errStr.contains('401') || errStr.contains('unauthorized') || errStr.contains('unauth');
                        if (looksUnauthed) {
                          return FutureBuilder<AuthStart>(
                            future: _api.getAuthStart(),
                            builder: (ctx, snap) {
                              final url = (snap.data?.authUrl ?? 'https://www.epicgames.com/id/login');
                              return _buildUnauthenticatedCard(context, url, 'No cached credentials. Please log in via your browser and enter the authorization code.');
                            },
                          );
                        }
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
                          final parts = v.split('_');
                          if (parts.length > 1) v = parts[1];
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
                        // Show loading indicator if we're refreshing, otherwise show "no assets" message
                        if (_refreshingFab) {
                          return const Padding(
                            padding: EdgeInsets.all(24.0),
                            child: Center(child: CircularProgressIndicator()),
                          );
                        }
                        return const Padding(
                          padding: EdgeInsets.all(16.0),
                          child: Text('No assets match your search.'),
                        );
                      }
                      return FabAssetsList(
                        key: _fabKey,
                        assets: filtered,
                        crossAxisCount: crossAxisCount,
                        spacing: spacing,
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
