Image cache location (Flutter UI)

The Flutter UI downloads and caches asset thumbnails and gallery images on disk so they load instantly on subsequent runs.

Where are the images stored?

- Linux:   ~/.local/share/test_app_ui/image_cache
- Windows: %APPDATA%/test_app_ui/image_cache
- macOS:   ~/Library/Application Support/test_app_ui/image_cache

Notes

- The folder is created on first run and printed to the console in debug builds as:
  Image cache directory: <path>
- The cache is implemented via flutter_cache_manager with a custom base directory so it is deterministic and easy to inspect.
- Files may be cleaned up automatically when they become very old (TTL ~365 days) or when there are too many items (cap ~2000 objects).

How to clear the image cache

- Manual: delete the image_cache folder shown above for your OS while the app is closed.
- Programmatic (not wired to UI): call DefaultCacheManager().emptyCache() or use the custom manager instance in code.

Advanced: overriding the cache directory (developers)

The cache manager is initialized in Flutter_EGL/lib/services/image_cache.dart. Developers can change the base directory by passing an overrideBaseDir to AppImageCache.init() before runApp().
