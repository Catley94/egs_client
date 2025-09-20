import 'dart:io';

import 'package:cached_network_image/cached_network_image.dart';
import 'package:flutter/foundation.dart';
import 'package:flutter/foundation.dart' show kDebugMode, kReleaseMode;
import 'package:flutter_cache_manager/flutter_cache_manager.dart';
import 'package:path/path.dart' as p;
import 'package:path_provider/path_provider.dart';

/// Centralized image cache configuration for the app.
///
/// This creates a deterministic on-disk cache location so we can answer
/// exactly where thumbnails and gallery images are stored. By default, the
/// cache is placed under the platform Application Support directory:
/// - Linux:   ~/.local/share/test_app_ui/image_cache
/// - Windows: %APPDATA%/test_app_ui/image_cache
/// - macOS:   ~/Library/Application Support/test_app_ui/image_cache
/// (On mobile it uses the OS-specific app support directory as well.)
class AppImageCache {
  static const String _cacheKey = 'egs_image_cache';
  static CacheManager? _manager;
  static String? _dirPath;

  static CacheManager get manager {
    final m = _manager;
    if (m == null) {
      throw StateError('AppImageCache not initialized. Call AppImageCache.init() before runApp().');
    }
    return m;
  }

  static String get directoryPath {
    final d = _dirPath;
    if (d == null) {
      throw StateError('AppImageCache not initialized. Call AppImageCache.init() before runApp().');
    }
    return d;
  }

  /// Initialize the cache manager and choose a stable base directory.
  /// Must be awaited before using [manager].
  static Future<void> init({Directory? overrideBaseDir}) async {
    if (_manager != null) return; // already initialized

    Directory baseDir;
    if (overrideBaseDir != null) {
      baseDir = overrideBaseDir;
    } else {
      if (!kReleaseMode) {
        // Development: keep cache inside project for visibility
        baseDir = Directory(p.join('.', 'cache'));
      } else {
        // Production: Prefer XDG cache on Linux; otherwise Application Support.
        // Fallback to temporary directory if support directory fails.
        try {
          if (!kIsWeb && Platform.isLinux) {
            final xdg = Platform.environment['XDG_CACHE_HOME'];
            final home = Platform.environment['HOME'];
            final basePath = (xdg != null && xdg.isNotEmpty)
                ? xdg
                : (home != null && home.isNotEmpty)
                    ? p.join(home, '.cache')
                    : '.cache';
            baseDir = Directory(p.join(basePath, 'egs_client'));
          } else {
            baseDir = await getApplicationSupportDirectory();
          }
        } catch (_) {
          baseDir = await getTemporaryDirectory();
        }
      }
    }

    final cacheDir = Directory(p.join(baseDir.path, 'image_cache'));
    if (!await cacheDir.exists()) {
      try { await cacheDir.create(recursive: true); } catch (_) {}
    }
    _dirPath = cacheDir.path;

    // Configure cache with large TTL and reasonable object count limit.
    final config = Config(
      _cacheKey,
      stalePeriod: const Duration(days: 365),
      maxNrOfCacheObjects: 2000,
      fileSystem: IOFileSystem(cacheDir.path),
      // Default HTTP file service is fine.
    );

    _manager = CacheManager(config);

    if (kDebugMode) {
      // Print so users can see where images are cached.
      // This shows up in the console where the Flutter binary is launched from Rust.
      // Example (Linux): /home/you/.local/share/test_app_ui/image_cache
      // Example (Windows): C:\\Users\\You\\AppData\\Roaming\\test_app_ui\\image_cache
      // Example (macOS): /Users/you/Library/Application Support/test_app_ui/image_cache
      // ignore: avoid_print
      print('Image cache directory: ${cacheDir.path}');
    }
  }
}
