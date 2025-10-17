part of '../api_service.dart';

extension ApiServiceExtras on ApiService {
  Future<String> getVersion() async {
    final uri = _uri('/version');
    final res = await http.get(uri);
    if (res.statusCode != 200) {
      throw Exception('Failed to fetch version: ${res.statusCode} ${res.body}');
    }
    try {
      final data = jsonDecode(res.body) as Map<String, dynamic>;
      final v = data['version']?.toString();
      if (v != null && v.isNotEmpty) return v;
    } catch (_) {}
    return res.body.toString();
  }
}
