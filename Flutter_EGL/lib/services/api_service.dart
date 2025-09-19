import 'dart:async';
import 'dart:convert';
import 'dart:io';
import 'package:http/http.dart' as http;
import 'package:web_socket_channel/web_socket_channel.dart';

import '../models/unreal.dart';
import '../models/fab.dart';

class ApiService {
  ApiService({String? baseUrl}) : baseUrl = baseUrl ?? defaultBaseUrl;

  static const String defaultBaseUrl = 'http://127.0.0.1:8080';

  final String baseUrl;

  Uri _uri(String path, [Map<String, String>? query]) {
    return Uri.parse(baseUrl).replace(path: path, queryParameters: query);
  }

  Uri _wsUri(String path, [Map<String, String>? query]) {
    final httpUri = _uri(path, query);
    final scheme = httpUri.scheme == 'https' ? 'wss' : 'ws';
    return httpUri.replace(scheme: scheme);
  }

  Future<List<UnrealEngineInfo>> listUnrealEngines({String? baseDir}) async {
    final uri = _uri('/list-unreal-engines', baseDir != null ? {'base': baseDir} : null);
    final res = await http.get(uri);
    if (res.statusCode != 200) {
      throw Exception('Failed to fetch engines: ${res.statusCode} ${res.body}');
    }
    final data = jsonDecode(res.body) as Map<String, dynamic>;
    final engines = (data['engines'] as List<dynamic>? ?? [])
        .map((e) => UnrealEngineInfo.fromJson(e as Map<String, dynamic>))
        .toList();
    return engines;
  }

  Future<OpenEngineResult> openUnrealEngine({required String version}) async {
    final uri = _uri('/open-unreal-engine', {'version': version});
    final res = await http.get(uri);
    final body = res.body;
    if (res.statusCode != 200) {
      // Try to parse message from JSON; otherwise surface body
      try {
        final data = jsonDecode(body) as Map<String, dynamic>;
        final msg = data['message']?.toString() ?? body;
        throw Exception('Failed to open Unreal Engine: ${res.statusCode} $msg');
      } catch (_) {
        throw Exception('Failed to open Unreal Engine: ${res.statusCode} $body');
      }
    }
    try {
      final data = jsonDecode(body) as Map<String, dynamic>;
      return OpenEngineResult.fromJson(data);
    } catch (_) {
      // Backend might return plain text; treat 200 as success with message
      return OpenEngineResult(launched: true, message: body.isNotEmpty ? body : 'Launched Unreal Engine');
    }
  }

  Future<List<UnrealProjectInfo>> listUnrealProjects({String? baseDir}) async {
    final uri = _uri('/list-unreal-projects', baseDir != null ? {'base': baseDir} : null);
    final res = await http.get(uri);
    if (res.statusCode != 200) {
      throw Exception('Failed to fetch projects: ${res.statusCode} ${res.body}');
    }
    final data = jsonDecode(res.body) as Map<String, dynamic>;
    final projects = (data['projects'] as List<dynamic>? ?? [])
        .map((e) => UnrealProjectInfo.fromJson(e as Map<String, dynamic>))
        .toList();

    // Enrich missing engine version by inspecting the .uproject file's EngineAssociation
    for (var i = 0; i < projects.length; i++) {
      final p = projects[i];
      if (p.engineVersion.isEmpty) {
        final path = p.uprojectFile.isNotEmpty ? p.uprojectFile : p.path;
        if (path.isNotEmpty) {
          try {
            final f = File(path);
            if (await f.exists()) {
              final txt = await f.readAsString();
              final dynamic decoded = jsonDecode(txt);
              if (decoded is Map<String, dynamic>) {
                final assoc = decoded['EngineAssociation']?.toString() ?? '';
                final norm = _normalizeEngineAssociation(assoc);
                if (norm.isNotEmpty) {
                  projects[i] = UnrealProjectInfo(
                    name: p.name,
                    path: p.path,
                    uprojectFile: p.uprojectFile,
                    engineVersion: norm,
                  );
                }
              }
            }
          } catch (_) {
            // Ignore read/parse errors silently; leave version unknown
          }
        }
      }
    }

    return projects;
  }

  String _normalizeEngineAssociation(String assoc) {
    var s = assoc.trim();
    if (s.isEmpty) return '';
    if (s.startsWith('UE_')) s = s.substring(3);
    // Accept patterns like 5, 5.4, 5.4.1
    final m = RegExp(r'^(\d+)(?:\.(\d+))?(?:\.(\d+))?$').firstMatch(s);
    if (m != null) {
      final maj = m.group(1) ?? '0';
      final min = m.group(2) ?? '0';
      return '$maj.$min';
    }
    // Unknown format (likely GUID); can't resolve client-side
    return '';
  }

  Future<Map<String, dynamic>> getPathsConfig() async {
    final uri = _uri('/config/paths');
    final res = await http.get(uri);
    if (res.statusCode != 200) {
      throw Exception('Failed to fetch paths config: ${res.statusCode} ${res.body}');
    }
    return jsonDecode(res.body) as Map<String, dynamic>;
  }

  Future<Map<String, dynamic>> setPathsConfig({String? projectsDir, String? enginesDir, String? cacheDir, String? downloadsDir}) async {
    final uri = _uri('/config/paths');
    final payload = <String, dynamic>{
      if (projectsDir != null) 'projects_dir': projectsDir,
      if (enginesDir != null) 'engines_dir': enginesDir,
      if (cacheDir != null) 'cache_dir': cacheDir,
      if (downloadsDir != null) 'downloads_dir': downloadsDir,
    };
    final res = await http.post(uri, headers: {'Content-Type': 'application/json'}, body: jsonEncode(payload));
    if (res.statusCode != 200) {
      throw Exception('Failed to update paths config: ${res.statusCode} ${res.body}');
    }
    return jsonDecode(res.body) as Map<String, dynamic>;
  }

  Future<List<FabAsset>> getFabList() async {
    final uri = _uri('/get-fab-list');
    final res = await http.get(uri);
    if (res.statusCode != 200) {
      throw Exception('Failed to fetch Fab library: ${res.statusCode} ${res.body}');
    }
    // The backend returns either the full JSON object or sometimes a string body on edge cases.
    final dynamic decoded = jsonDecode(res.body);
    if (decoded is Map<String, dynamic>) {
      final lib = FabLibraryResponse.fromJson(decoded);
      return lib.results;
    } else {
      // Unexpected format; return empty list but not crash UI
      return <FabAsset>[];
    }
  }

  Future<OpenProjectResult> openUnrealProject({required String project, required String version, String? engineBase, String? projectsBase}) async {
    final query = <String, String>{
      'project': project,
      'version': version,
      if (engineBase != null) 'engine_base': engineBase,
      if (projectsBase != null) 'projects_base': projectsBase,
    };
    final uri = _uri('/open-unreal-project', query);
    print("Query: $query");
    final res = await http.get(uri);
    final body = res.body;
    if (res.statusCode != 200) {
      // Backend may return JSON with message; surface it
      try {
        final data = jsonDecode(body) as Map<String, dynamic>;
        final msg = data['message']?.toString() ?? body;
        throw Exception('Failed to open project: ${res.statusCode} $msg');
      } catch (_) {
        throw Exception('Failed to open project: ${res.statusCode} $body');
      }
    }
    final data = jsonDecode(body) as Map<String, dynamic>;
    return OpenProjectResult.fromJson(data);
  }

  Future<ImportAssetResult> importAsset({required String assetName, required String project, String? targetSubdir, bool overwrite = false, String? jobId}) async {
    final uri = _uri('/import-asset');
    final payload = <String, dynamic>{
      'asset_name': assetName,
      'project': project,
      if (targetSubdir != null && targetSubdir.isNotEmpty) 'target_subdir': targetSubdir,
      if (overwrite) 'overwrite': true,
      if (jobId != null && jobId.isNotEmpty) 'job_id': jobId,
    };
    final res = await http.post(
      uri,
      headers: {'Content-Type': 'application/json'},
      body: jsonEncode(payload),
    );
    final body = res.body;
    if (res.statusCode != 200) {
      // Try to parse error message from JSON; otherwise surface plain text
      try {
        final data = jsonDecode(body) as Map<String, dynamic>;
        final msg = data['message']?.toString() ?? body;
        throw Exception('Import failed: ${res.statusCode} $msg');
      } catch (_) {
        throw Exception('Import failed: ${res.statusCode} $body');
      }
    }
    // Try parse JSON; otherwise treat as success with message
    try {
      final data = jsonDecode(body) as Map<String, dynamic>;
      return ImportAssetResult.fromJson(data);
    } catch (_) {
      return ImportAssetResult(success: true, message: body.isNotEmpty ? body : 'Import started');
    }
  }

  // Open a WebSocket channel for a given job to receive progress events
  WebSocketChannel openProgressChannel(String jobId) {
    final uri = _wsUri('/ws', {'jobId': jobId});
    // Debug: log WS connection attempts
    // ignore: avoid_print
    print('[WS] Connecting to $uri for job $jobId');
    return WebSocketChannel.connect(uri);
  }

  // Convenience: map events to strongly-typed ProgressEvent with debug info
  Stream<ProgressEvent> progressEvents(String jobId) {
    final channel = openProgressChannel(jobId);

    // Use a controller to expose connection lifecycle and errors to listeners
    final controller = StreamController<ProgressEvent>();

    // Immediately emit a connecting status to update UI and logs
    controller.add(
      ProgressEvent(
        jobId: jobId,
        phase: 'ws:connecting',
        message: 'Connecting to progress channel...',
        progress: null,
        details: null,
      ),
    );

    late final StreamSubscription sub;
    sub = channel.stream.listen(
      (dynamic data) {
        try {
          // ignore: avoid_print
          print('[WS] message (job=$jobId): $data');
          final map = jsonDecode(data as String) as Map<String, dynamic>;
          controller.add(ProgressEvent.fromJson(map));
        } catch (e) {
          // Fallback: wrap plain text as message
          controller.add(ProgressEvent(jobId: jobId, phase: 'message', message: data?.toString() ?? '', progress: null, details: null));
        }
      },
      onError: (Object err, [StackTrace? st]) {
        // ignore: avoid_print
        print('[WS] error (job=$jobId): $err');
        controller.add(ProgressEvent(jobId: jobId, phase: 'ws:error', message: err.toString(), progress: null, details: null));
        controller.close();
      },
      onDone: () {
        // ignore: avoid_print
        print('[WS] closed (job=$jobId)');
        controller.add(ProgressEvent(jobId: jobId, phase: 'ws:closed', message: 'WebSocket closed', progress: null, details: null));
        controller.close();
      },
      cancelOnError: true,
    );

    // Ensure we clean up when the consumer cancels
    controller.onCancel = () async {
      await sub.cancel();
      await channel.sink.close();
    };

    return controller.stream;
  }
}

class ProgressEvent {
  final String jobId;
  final String phase;
  final String message;
  final double? progress;
  final Map<String, dynamic>? details;

  ProgressEvent({
    required this.jobId,
    required this.phase,
    required this.message,
    this.progress,
    this.details,
  });

  factory ProgressEvent.fromJson(Map<String, dynamic> json) {
    double? _parseProgress(dynamic v) {
      if (v == null) return null;
      if (v is num) return v.toDouble();
      if (v is String) {
        final s = v.trim();
        // Accept values like "0.56", "56", or "56%"
        final stripped = s.endsWith('%') ? s.substring(0, s.length - 1) : s;
        final d = double.tryParse(stripped);
        return d;
      }
      return null;
    }

    double? progress = _parseProgress(json['progress'])
        ?? _parseProgress(json['percentage'])
        ?? _parseProgress(json['percent']);

    Map<String, dynamic>? details;
    if (json['details'] is Map<String, dynamic>) {
      details = json['details'] as Map<String, dynamic>;
      progress = progress ?? _parseProgress(details['progress']) ?? _parseProgress(details['percentage']) ?? _parseProgress(details['percent']);
    }

    return ProgressEvent(
      jobId: json['job_id']?.toString() ?? '',
      phase: json['phase']?.toString() ?? '',
      message: json['message']?.toString() ?? '',
      progress: progress,
      details: details,
    );
  }
}

class OpenProjectResult {
  final bool launched;
  final String? engineName;
  final String? engineVersion;
  final String? editorPath;
  final String project;
  final String message;

  OpenProjectResult({
    required this.launched,
    required this.engineName,
    required this.engineVersion,
    required this.editorPath,
    required this.project,
    required this.message,
  });

  factory OpenProjectResult.fromJson(Map<String, dynamic> json) {
    return OpenProjectResult(
      launched: json['launched'] as bool? ?? false,
      engineName: json['engine_name'] as String?,
      engineVersion: json['engine_version'] as String?,
      editorPath: json['editor_path'] as String?,
      project: json['project'] as String? ?? '',
      message: json['message'] as String? ?? '',
    );
  }
}

class OpenEngineResult {
  final bool launched;
  final String message;

  OpenEngineResult({required this.launched, required this.message});

  factory OpenEngineResult.fromJson(Map<String, dynamic> json) {
    return OpenEngineResult(
      launched: json['launched'] as bool? ?? false,
      message: json['message'] as String? ?? '',
    );
  }
}

class ImportAssetResult {
  final bool success;
  final String message;
  final String? project;
  final String? assetName;

  ImportAssetResult({required this.success, required this.message, this.project, this.assetName});

  factory ImportAssetResult.fromJson(Map<String, dynamic> json) {
    // Backend may return keys like { success, message, project, asset_name }
    return ImportAssetResult(
      success: json['success'] as bool? ?? true,
      message: json['message'] as String? ?? '',
      project: json['project'] as String?,
      assetName: (json['asset_name'] ?? json['assetName']) as String?,
    );
  }
}

class CreateProjectResult {
  final bool ok;
  final String message;
  final String? command;
  final String? projectPath;

  CreateProjectResult({required this.ok, required this.message, this.command, this.projectPath});

  factory CreateProjectResult.fromJson(Map<String, dynamic> json) {
    return CreateProjectResult(
      ok: json['ok'] as bool? ?? false,
      message: json['message'] as String? ?? '',
      command: json['command'] as String?,
      projectPath: (json['project_path'] ?? json['projectPath']) as String?,
    );
  }
}

extension CreateUnrealProjectApi on ApiService {
  Future<CreateProjectResult> createUnrealProject({
    String? enginePath,
    String? templateProject,
    String? assetName,
    required String outputDir,
    required String projectName,
    String projectType = 'bp',
    bool dryRun = false,
    String? jobId,
  }) async {
    final uri = _uri('/create-unreal-project');
    final payload = <String, dynamic>{
      'engine_path': enginePath,
      'template_project': templateProject,
      'asset_name': assetName,
      'output_dir': outputDir,
      'project_name': projectName,
      'project_type': projectType,
      'dry_run': dryRun,
      if (jobId != null && jobId.isNotEmpty) 'job_id': jobId,
    }..removeWhere((key, value) => value == null);

    final res = await http.post(
      uri,
      headers: {'Content-Type': 'application/json'},
      body: jsonEncode(payload),
    );
    final body = res.body;
    if (res.statusCode != 200) {
      try {
        final data = jsonDecode(body) as Map<String, dynamic>;
        final msg = data['message']?.toString() ?? body;
        throw Exception('Create project failed: ${res.statusCode} $msg');
      } catch (_) {
        throw Exception('Create project failed: ${res.statusCode} $body');
      }
    }
    try {
      final data = jsonDecode(body) as Map<String, dynamic>;
      return CreateProjectResult.fromJson(data);
    } catch (_) {
      return CreateProjectResult(ok: true, message: body.isNotEmpty ? body : 'OK', command: null, projectPath: null);
    }
  }
}
