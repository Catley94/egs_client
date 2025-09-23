egs_client (Rust + Flutter)

Overview
- This project combines a Rust backend (Actix Web) with a Flutter desktop/web UI to manage and launch Unreal Engine projects and to access/download assets from Epic Games Fab.
- The Rust service exposes a simple HTTP+WebSocket API. The Flutter app talks to it via HTTP for commands and a WebSocket for live progress and log streaming.
- The two parts can be run together (Rust spawns Flutter) or independently.

High-level architecture
- Rust backend (src/):
  - Actix-web HTTP server with JSON endpoints in src/api.rs.
  - Business logic and integrations in src/utils/mod.rs (EGS auth, token caching, Fab library fetch, downloader, paths/config helpers, WS event bus).
  - Binary entry in src/main.rs that chooses run mode (backend only, frontend only, or both), resolves Flutter binary, and manages child process lifecycle.
- Flutter frontend (Flutter_EGL/):
  - UI built with Material 3 using NavigationRail.
  - Service layer in lib/services/api_service.dart encapsulates HTTP and WebSocket calls to the backend.
  - Models for Fab and Unreal metadata in lib/models/.
  - Widgets and components under lib/widgets/.

Data flow and connections
- Authentication
  1) UI calls GET /auth/start to obtain the Epic login URL (kEpicLoginUrl); the user signs in and copies the authorizationCode.
  2) UI posts the code to POST /auth/complete. Rust uses egs_api to exchange it and persists tokens locally (debug: cache/.egs_client_tokens.json; release: XDG config).
  3) Subsequent requests use cached tokens; no browser is required.
- Fab library and downloads
  - GET /get-fab-list returns the cached/enriched library JSON if available.
  - GET /refresh-fab-list re-fetches the library from EGS and caches it (cache/fab_list.json). The JSON is enriched with local-only downloaded flags based on the downloads/ folder.
  - GET /download-asset/{namespace}/{assetId}/{artifactId} starts an asset download; live progress is pushed over WebSocket /ws?jobId=... with phase messages and percentage.
- Unreal projects and engines
  - GET /list-unreal-projects and /list-unreal-engines scan the filesystem to enumerate projects and engine installs.
  - GET /open-unreal-project and /open-unreal-engine launch the editor with appropriate parameters.
- WebSocket progress bus
  - The UI opens a WS connection to /ws?jobId=... and receives JSON ProgressEvent messages emitted by utils::emit_event.

Rust: notable crates and their purposes
- actix-web, actix, actix-web-actors: HTTP server and WebSocket handling.
- tokio: async runtime used by Actix and async downloads.
- serde, serde_json: JSON (de)serialization for requests and responses.
- reqwest: HTTP client for fetching chunk files during downloads.
- dashmap: concurrent maps for per-job channels and buffers.
- tokio::sync::broadcast: broadcast channels for progress events per job.
- walkdir: filesystem traversal for copy and discovery operations.
- sha1: hashing verification of assembled files when hashes are available.
- anyhow/colored: error handling and coloring logs (if used).
- egs_api: Epic Games Services API (account, Fab library, manifests and downloads).

Flutter: notable packages and their purposes
- http: HTTP calls to the backend.
- web_socket_channel: WebSocket client for live progress.
- window_manager, window_size: desktop window management/frameless title bar integration.
- url_launcher: to open external URLs (e.g., Epic login page) when needed.
- cached_network_image: image thumbnails for assets (if used in UI).
- path_provider, path: local paths for caching or helpers.

How to run
- Prerequisites
  - Rust (stable), Cargo
  - Flutter SDK (channel with desktop support for your OS)

- Run backend only
  1) cargo run -- Backend
  - This starts the Actix server at http://127.0.0.1:8080. You can interact with it using curl or the Flutter app launched separately.

- Run Flutter UI only
  1) cd Flutter_EGL
  2) flutter pub get
  3) flutter run -d windows|linux|macos|chrome
  - Ensure the backend from another terminal is running at http://127.0.0.1:8080 (configure baseUrl in ApiService if different).

- Run both (Rust spawns Flutter)
  1) cargo run -- Both
  - The backend starts first, then tries to locate a Flutter binary and launch the UI pointed at the backend bind address.

Configuration and directories
- In debug/dev builds:
  - cache/: various cache files including fab_list.json and token cache (.egs_client_tokens.json)
  - downloads/: asset downloads arranged by sanitized title or namespace-id-artifactId
- In release:
  - Respect XDG base directories for cache, data, and config where appropriate (see utils::default_* helpers).

Key endpoints (short list)
- GET /get-fab-list → cached/enriched Fab library JSON
- GET /refresh-fab-list → fetch fresh library JSON from EGS and cache it
- GET /download-asset/{namespace}/{assetId}/{artifactId}?jobId=abc → start download
- GET /list-unreal-projects, GET /list-unreal-engines → enumerate
- GET /open-unreal-project, GET /open-unreal-engine → launch editor
- WS /ws?jobId=abc → receive ProgressEvent messages
- GET /config/paths, POST /config/paths → read/update directories
- POST /auth/complete, GET /auth/start → authentication helpers

Dart service surface (selected)
- ApiService.getFabList(), refreshFabList(), downloadAsset(), openUnrealProject(), importAsset(), createUnrealProject(), listUnrealProjects(), listUnrealEngines(), getPathsConfig(), setPathsConfig(), openUnrealEngine(), openProgressChannel(), progressEvents().

Security considerations
- Token cache contains sensitive access/refresh tokens; on Unix systems the file permission is set to 0600.
- Downloads are verified via SHA1 when hashes are provided in manifests; otherwise size checks are used.
- The WebSocket bus is process-local and unauthenticated; do not expose the backend to untrusted networks without additional controls.

Troubleshooting
- 401/403 on Fab list: complete the auth flow first (GET /auth/start then POST /auth/complete with code).
- No progress events in UI: ensure the UI opens /ws with the correct jobId used by the API call.
- Engine launch fails: verify the paths in the Paths Config or environment variables (EGS_UNREAL_ENGINES_DIR etc.).

License
- See repository policy or add a LICENSE file as appropriate.
