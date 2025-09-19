//! egs_client — minimal Actix Web service to browse and download Epic Fab assets.
//!
//! What this binary does:
//! - Boots an HTTP server on 127.0.0.1:8080 (override with BIND_ADDR or PORT)
//! - Exposes routes implemented in the api module:
//!   - GET /get-fab-list: Returns cached Fab library or refreshes it.
//!   - GET /refresh-fab-list: Forces refresh from Epic Games Services (EGS).
//!   - GET /download-asset/{namespace}/{asset_id}/{artifact_id}: Downloads a specific asset.
//!
//! How to run:
//! - cargo run
//! - Visit http://127.0.0.1:8080/get-fab-list
//! - Use curl examples provided in api.rs for downloads.
//!
//! Environment and logs:
//! - Uses env_logger. To increase verbosity, run:
//!   RUST_LOG=info cargo run
//! - The server binds to 127.0.0.1:8080 by default. Override with env vars: BIND_ADDR or PORT.
//!
//! Minimal architecture diagram:
//!   main.rs (this file) -> constructs Actix App -> registers api services -> runs HttpServer
//!                              |
//!                              v
//!                         api.rs routes -> call into utils/mod.rs (auth, cache, download) -> egs_api crate

mod api;
mod utils;
mod models;

// Configure where the Flutter desktop binary resides in development vs production builds.
// These can be overridden at runtime with the FLUTTER_APP_PATH environment variable.
// Dev (debug build): typically points to a debug bundle output from `flutter build linux --debug`.
pub const DEV_FLUTTER_APP_PATH: &str = "Flutter_EGL/build/linux/x64/debug/bundle/test_app_ui";
// Prod (release build): typically points to a release bundle output from `flutter build linux --release`.
pub const PROD_FLUTTER_APP_PATH: &str = "Flutter_EGL/build/linux/x64/release/bundle/test_app_ui";

use actix_web::{App, HttpServer};
use std::env;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::sync::broadcast;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RunMode {
    Backend,
    Frontend,
    Both,
}

fn parse_mode() -> RunMode {
    // Priority: CLI arg --mode=..., then positional arg, then env EGS_MODE,
    // else auto-detect: if a Flutter binary is present, default to Both; otherwise Backend
    let mut mode_str: Option<String> = None;
    let args: Vec<String> = env::args().collect();
    for a in &args {
        if let Some(rest) = a.strip_prefix("--mode=") {
            mode_str = Some(rest.to_string());
            break;
        }
    }
    if mode_str.is_none() {
        if args.len() > 1 {
            // Allow: `egs_client both` as a shorthand
            let p = args[1].to_lowercase();
            if ["backend", "frontend", "both"].contains(&p.as_str()) {
                mode_str = Some(p);
            }
        }
    }
    if mode_str.is_none() {
        if let Ok(env_mode) = env::var("EGS_MODE") {
            mode_str = Some(env_mode);
        }
    }
    if let Some(s) = mode_str.as_deref() {
        return match s {
            "frontend" => RunMode::Frontend,
            "both" => RunMode::Both,
            _ => RunMode::Backend,
        };
    }
    // No explicit mode provided — auto-detect Flutter binary for a single-binary experience
    if resolve_flutter_binary().is_some() {
        RunMode::Both
    } else {
        RunMode::Backend
    }
}

fn resolve_flutter_binary() -> Option<PathBuf> {
    // Highest priority: explicit env override.
    if let Ok(p) = env::var("FLUTTER_APP_PATH") {
        let pb = PathBuf::from(p);
        if pb.exists() {
            println!("Flutter binary: using FLUTTER_APP_PATH override: {}", pb.display());
            return Some(pb);
        } else {
            eprintln!("FLUTTER_APP_PATH is set but path does not exist: {}", pb.display());
        }
    }

    // Next: build-mode specific constant paths defined at the top of this file.
    // If compiled in debug (dev) mode, prefer the dev path; otherwise prefer the prod path.
    let debug_build = cfg!(debug_assertions);
    println!(
        "Rust build mode detected: {} (path preference: {} first)",
        if debug_build { "debug" } else { "release" },
        if debug_build { "DEV_FLUTTER_APP_PATH" } else { "PROD_FLUTTER_APP_PATH" }
    );
    let mode_pref: [&str; 2] = if debug_build {
        [DEV_FLUTTER_APP_PATH, PROD_FLUTTER_APP_PATH]
    } else {
        [PROD_FLUTTER_APP_PATH, DEV_FLUTTER_APP_PATH]
    };
    for c in mode_pref {
        let p = Path::new(c);
        if p.exists() {
            println!(
                "Flutter binary: selected {} (exists)",
                p.display()
            );
            return Some(p.to_path_buf());
        } else {
            println!("Flutter binary candidate not found: {}", p.display());
        }
    }

    // Fallbacks: project-relative defaults based on platform and common Flutter build outputs.
    // App name from pubspec.yaml: test_app_ui
    #[cfg(target_os = "linux")]
    {
        let candidates = [
            "Flutter_EGL/build/linux/x64/release/bundle/test_app_ui",
            "Flutter_EGL/build/linux/x64/debug/bundle/test_app_ui",
            // Older layout (if any)
            "Flutter_EGL/build/linux/x64/release/bundle/test_app_ui/test_app_ui",
        ];
        for c in candidates {
            let p = Path::new(c);
            if p.exists() {
                println!("Flutter binary: selected fallback candidate: {}", p.display());
                return Some(p.to_path_buf());
            } else {
                println!("Flutter binary fallback candidate not found: {}", p.display());
            }
        }
    }
    println!("Flutter binary not found via env, configured paths, or fallbacks.");
    None
}

fn spawn_flutter(ui_path: &Path, bind_addr: &str) -> std::io::Result<Child> {
    // Canonicalize to avoid issues with relative paths and ensure parent dir is valid
    let path = match std::fs::canonicalize(ui_path) {
        Ok(p) => p,
        Err(_) => ui_path.to_path_buf(),
    };
    let parent = path.parent().unwrap_or(Path::new("."));
    let program_name = path.file_name().unwrap_or_default();

    // On Unix, ensure the binary is executable (some VCS or copy ops may strip +x)
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if let Ok(meta) = std::fs::metadata(&path) {
            let mut perms = meta.permissions();
            let mode = perms.mode();
            if mode & 0o111 == 0 {
                let new_mode = mode | 0o755;
                let _ = std::fs::set_permissions(&path, std::fs::Permissions::from_mode(new_mode));
            }
        }
    }

    // Build command: run from the bundle directory and execute by local name with explicit ./ prefix
    let mut cmd = if !program_name.is_empty() {
        let mut prog = std::path::PathBuf::from("./");
        prog.push(program_name);
        Command::new(prog)
    } else {
        // Fallback to the full path
        Command::new(&path)
    };
    cmd.current_dir(parent);

    // If the Flutter app adds support for overriding API base, pass it here.
    cmd.env("EGS_BASE_URL", format!("http://{}", bind_addr))
        .stdin(Stdio::null())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit());
    cmd.spawn()
}

#[actix_web::main]
async fn main() -> std::io::Result<()> {
    // Initialize env_logger to honor RUST_LOG levels (e.g., RUST_LOG=info)
    env_logger::init();

    // Explicitly log Rust build mode early for visibility
    println!("Rust build mode: {}", if cfg!(debug_assertions) { "debug" } else { "release" });

    let mode = parse_mode();

    // Ensure runtime directories exist (non-fatal if they cannot be created)
    for dir in [api::DEFAULT_CACHE_DIR_NAME, api::DEFAULT_DOWNLOADS_DIR_NAME] {
        // Create cache and downloads directories locally in project folder
        if let Err(e) = std::fs::create_dir_all(dir) {
            eprintln!("Warning: failed to create directory '{}': {}", dir, e);
        }
    }

    // Determine bind address: prefer BIND_ADDR, else PORT, else 127.0.0.1:8080 (safe default for host)
    let bind_addr = if let Ok(addr) = env::var("BIND_ADDR") {
        addr
    } else if let Ok(port) = env::var("PORT") {
        format!("0.0.0.0:{}", port)
    } else {
        "127.0.0.1:8080".to_string()
    };

    // Frontend-only mode: run the Flutter UI without starting backend (assumes external backend)
    if mode == RunMode::Frontend {
        if let Some(ui_bin) = resolve_flutter_binary() {
            println!("Launching Flutter UI: {}", ui_bin.display());
            let mut child = spawn_flutter(&ui_bin, &bind_addr)?;
            let status = child.wait().expect("failed waiting for Flutter UI");
            println!("Flutter UI exited with status: {}", status);
            return Ok(());
        } else {
            eprintln!("Flutter UI binary not found. Build it first (see justfile tasks) or set FLUTTER_APP_PATH.");
            std::process::exit(2);
        }
    }

    println!("Starting egs_client HTTP server on {} (mode: {:?})", bind_addr, mode);

    // In BOTH mode, enable shutdown on WS close (frontend lifecycle drives backend)
    if mode == RunMode::Both {
        std::env::set_var("EGS_EXIT_ON_WS_CLOSE", "1");
    }

    // Prepare shutdown broadcast channel and register with utils so WS can request shutdown
    let (shutdown_tx, _shutdown_rx0) = broadcast::channel::<()>(4);
    crate::utils::set_shutdown_sender(shutdown_tx.clone());

    // Shared child handle for Ctrl+C handling when in BOTH mode
    let flutter_child: Arc<Mutex<Option<Child>>> = Arc::new(Mutex::new(None));

    // Retry loop on bind failure to avoid immediate exit (e.g., short-lived port conflicts)
    loop {
        match HttpServer::new(|| {
            App::new()
                // Public HTTP endpoints
                .service(api::get_fab_list)
                .service(api::refresh_fab_list)
                .service(api::download_asset)
                .service(api::list_unreal_projects)
                .service(api::list_unreal_engines)
                .service(api::open_unreal_project)
                .service(api::open_unreal_engine)
                .service(api::import_asset)
                .service(api::create_unreal_project)
                .service(api::ws_endpoint)
                .service(api::get_paths_config)
                .service(api::set_paths_config)
        })
        .bind(&bind_addr) {
            Ok(server) => {
                // Start server
                let srv = server.run();

                // If BOTH mode, launch Flutter after server is started
                if mode == RunMode::Both {
                    match resolve_flutter_binary() {
                        Some(ui_bin) => {
                            println!("Launching Flutter UI: {}", ui_bin.display());
                            match spawn_flutter(&ui_bin, &bind_addr) {
                                Ok(child) => {
                                    // Store child handle
                                    let mut guard = flutter_child.lock().unwrap();
                                    *guard = Some(child);

                                    // Watcher: when Flutter UI exits, stop the HTTP server
                                    let watcher_child = Arc::clone(&flutter_child);
                                    let srv_handle2 = srv.handle();
                                    tokio::spawn(async move {
                                        loop {
                                            tokio::time::sleep(Duration::from_millis(500)).await;
                                            if let Ok(mut g) = watcher_child.lock() {
                                                if let Some(ch) = g.as_mut() {
                                                    match ch.try_wait() {
                                                        Ok(Some(status)) => {
                                                            eprintln!("Flutter UI exited with status: {} — stopping backend...", status);
                                                            let h = srv_handle2.clone();
                                                            tokio::spawn(async move { h.stop(true).await; });
                                                            break;
                                                        }
                                                        Ok(None) => {}
                                                        Err(e) => {
                                                            eprintln!("Error monitoring Flutter UI process: {}", e);
                                                        }
                                                    }
                                                } else {
                                                    break;
                                                }
                                            }
                                        }
                                    });
                                }
                                Err(err) => {
                                    eprintln!("Failed to spawn Flutter UI: {}", err);
                                }
                            }
                        }
                        None => {
                            eprintln!("Flutter UI binary not found. Build it first (see justfile tasks) or set FLUTTER_APP_PATH.");
                        }
                    }
                }

                // Ctrl+C handling: stop server and kill Flutter child if present
                {
                    let flutter_child = Arc::clone(&flutter_child);
                    let srv_handle = srv.handle();
                    let _ = ctrlc::set_handler(move || {
                        eprintln!("\nCtrl+C received — shutting down...");
                        // Stop server gracefully (spawn async task to await)
                        let handle = srv_handle.clone();
                        tokio::spawn(async move {
                            handle.stop(true).await;
                        });
                        // Kill Flutter child if running
                        if let Ok(mut guard) = flutter_child.lock() {
                            if let Some(child) = guard.as_mut() {
                                let _ = child.kill();
                            }
                        }
                    });
                }

                // Listen for WS-close-triggered shutdown requests and stop the server
                {
                    let srv_handle3 = srv.handle();
                    let mut rx = shutdown_tx.subscribe();
                    tokio::spawn(async move {
                        if rx.recv().await.is_ok() {
                            eprintln!("Shutdown requested (WS close) — stopping backend...");
                            let h = srv_handle3.clone();
                            tokio::spawn(async move { h.stop(true).await; });
                        }
                    });
                }

                return srv.await;
            }
            Err(e) => {
                eprintln!("Failed to bind to {}: {} — retrying in 2s...", bind_addr, e);
                tokio::time::sleep(Duration::from_secs(2)).await;
            }
        }
    }
}


