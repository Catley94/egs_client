class UnrealEngineInfo {
  final String name;
  final String version;
  final String path;
  final String? editorPath;

  UnrealEngineInfo({
    required this.name,
    required this.version,
    required this.path,
    this.editorPath,
  });

  factory UnrealEngineInfo.fromJson(Map<String, dynamic> json) {
    return UnrealEngineInfo(
      name: json['name'] as String? ?? '',
      version: json['version'] as String? ?? 'unknown',
      path: json['path'] as String? ?? '',
      editorPath: json['editor_path'] as String?,
    );
  }
}

class UnrealProjectInfo {
  final String name;
  final String path;
  final String uprojectFile;
  final String engineVersion; // e.g., "5.3", "5.4"; may be empty if unknown

  UnrealProjectInfo({
    required this.name,
    required this.path,
    required this.uprojectFile,
    required this.engineVersion,
  });

  factory UnrealProjectInfo.fromJson(Map<String, dynamic> json) {
    // Try multiple possible keys that the backend might provide
    final ver = (json['engine_version'] ?? json['engineVersion'] ?? json['ue_version'] ?? json['version'])?.toString() ?? '';
    return UnrealProjectInfo(
      name: json['name'] as String? ?? '',
      path: json['path'] as String? ?? '',
      uprojectFile: json['uproject_file'] as String? ?? '',
      engineVersion: ver,
    );
  }
}
