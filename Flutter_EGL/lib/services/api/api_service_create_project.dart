part of '../api_service.dart';

class CreateProjectResult {
  final bool ok;
  final String message;
  final String? command;
  final String? projectPath;

  CreateProjectResult({required this.ok, required this.message, this.command, this.projectPath});

  // Backwards-compatible alias expected by some UI code
  bool get success => ok;

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
    String? ue,
    String? namespace,
    String? assetId,
    String? artifactId,
    required String outputDir,
    required String projectName,
    String projectType = 'bp',
    bool dryRun = false,
    String? jobId,
  }) async {
    final uri = _uri('/create-unreal-project');
    print("¬ createProject");
    final payload = <String, dynamic>{
      'engine_path': enginePath,
      'template_project': templateProject,
      'asset_name': assetName,
      if (ue != null && ue.isNotEmpty) 'ue': ue,
      if (namespace != null && namespace.isNotEmpty) 'namespace': namespace,
      if (assetId != null && assetId.isNotEmpty) 'asset_id': assetId,
      if (artifactId != null && artifactId.isNotEmpty) 'artifact_id': artifactId,
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
