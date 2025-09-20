import 'package:flutter/material.dart';
import 'package:cached_network_image/cached_network_image.dart';
import 'package:test_app_ui/services/image_cache.dart';

class FabLibraryItem extends StatelessWidget {
  final String title;
  final String sizeLabel;
  final bool isCompleteProject;
  final VoidCallback? onPrimaryPressed;
  final bool isBusy;
  final bool downloaded;
  final String? thumbnailUrl;
  final VoidCallback? onTap;
  final bool useWarningStyle; // when true, style primary button in warning (yellow)

  const FabLibraryItem({
    required this.title,
    required this.sizeLabel,
    required this.isCompleteProject,
    this.onPrimaryPressed,
    this.isBusy = false,
    this.downloaded = false,
    this.thumbnailUrl,
    this.onTap,
    this.useWarningStyle = false,
  });

  @override
  Widget build(BuildContext context) {
    final cs = Theme.of(context).colorScheme;
    return InkWell(
      onTap: onTap,
      borderRadius: BorderRadius.circular(12),
      child: Container(
        decoration: BoxDecoration(
          color: cs.surface,
          borderRadius: BorderRadius.circular(12),
          border: Border.all(color: Theme.of(context).colorScheme.outlineVariant),
        ),
        padding: const EdgeInsets.all(12),
        child: Row(
          crossAxisAlignment: CrossAxisAlignment.start,
          children: [
          // Left: image placeholder with optional downloaded badge
          Stack(
            clipBehavior: Clip.none,
            children: [
              ClipRRect(
                borderRadius: BorderRadius.circular(10),
                child: Container(
                  width: 112,
                  height: 112,
                  decoration: BoxDecoration(
                    color: Theme.of(context).colorScheme.surfaceVariant,
                    border: Border.all(color: Theme.of(context).colorScheme.outlineVariant),
                  ),
                  child: (thumbnailUrl != null && thumbnailUrl!.isNotEmpty)
                      ? CachedNetworkImage(
                          imageUrl: thumbnailUrl!,
                          cacheManager: AppImageCache.manager,
                          fit: BoxFit.cover,
                          placeholder: (context, url) => const Center(child: SizedBox(width: 24, height: 24, child: CircularProgressIndicator(strokeWidth: 2))),
                          errorWidget: (context, url, error) => Icon(Icons.broken_image, size: 40, color: cs.onSurfaceVariant),
                        )
                      : Icon(Icons.image, size: 40, color: cs.onSurfaceVariant),
                ),
              ),
              if (downloaded)
                Positioned(
                  top: -6,
                  left: -6,
                  child: Container(
                    padding: const EdgeInsets.all(4),
                    decoration: BoxDecoration(
                      color: Colors.green.shade600,
                      shape: BoxShape.circle,
                      border: Border.all(color: cs.surface, width: 2),
                    ),
                    child: const Icon(Icons.check, size: 14, color: Colors.white),
                  ),
                ),
            ],
          ),
          const SizedBox(width: 16),
          // Right: title, button, size stacked vertically
          Expanded(
            child: Column(
              crossAxisAlignment: CrossAxisAlignment.start,
              children: [
                Text(
                  title,
                  maxLines: 2,
                  overflow: TextOverflow.ellipsis,
                  style: Theme.of(context).textTheme.titleSmall?.copyWith(
                    fontWeight: FontWeight.w700,
                  ),
                ),
                const SizedBox(height: 12),
                SizedBox(
                  height: 36,
                  child: FilledButton.icon(
                    style: useWarningStyle
                        ? FilledButton.styleFrom(
                            backgroundColor: cs.secondaryContainer,
                            foregroundColor: cs.onSecondaryContainer,
                          )
                        : null,
                    onPressed: isBusy
                        ? null
                        : (onPrimaryPressed ?? () {
                            if (isCompleteProject) {
                              // No-op default
                              debugPrint("Create Project clicked (no handler)");
                            } else {
                              debugPrint("Import Asset clicked (no handler)");
                            }
                          }),
                    icon: isBusy
                        ? SizedBox(
                            width: 18,
                            height: 18,
                            child: const CircularProgressIndicator(strokeWidth: 2),
                          )
                        : Icon(isCompleteProject ? Icons.add : Icons.download, size: 18),
                    label: Text(isCompleteProject ? 'Create Project' : 'Import Asset'),
                  ),
                ),
                const SizedBox(height: 8),
                Text(
                  sizeLabel,
                  style: Theme.of(context).textTheme.bodySmall?.copyWith(
                    color: cs.onSurfaceVariant,
                  ),
                ),
              ],
            ),
          ),
        ],
      ),
    ),
  );
  }
}