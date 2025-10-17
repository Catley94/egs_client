part of '../api_service.dart';

class RefreshFabAssetResult {
  final bool success;
  final String message;
  final bool anyDownloaded;
  RefreshFabAssetResult({required this.success, required this.message, required this.anyDownloaded});
}

extension RefreshFabAssetApi on ApiService {
  Future<RefreshFabAssetResult> refreshFabAsset({required String assetNamespace, required String assetId}) async {
    try {
      // Refreshes the whole list currently
      final list = await refreshFabList();
      final asset = list.firstWhere(
        (e) => e.assetNamespace == assetNamespace && e.assetId == assetId,
        orElse: () => FabAsset(
          title: '',
          description: '',
          assetId: assetId,
          assetNamespace: assetNamespace,
          source: 'fab',
          url: null,
          distributionMethod: '',
          images: const [],
          projectVersions: const [],
          downloadedVersions: const [],
        ),
      );
      final anyDownloaded = asset.anyDownloaded;
      return RefreshFabAssetResult(success: true, message: '', anyDownloaded: anyDownloaded);
    } catch (e) {
      return RefreshFabAssetResult(success: false, message: e.toString(), anyDownloaded: false);
    }
  }
}
