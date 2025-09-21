import 'dart:async';

import 'package:flutter/material.dart';
import 'package:window_manager/window_manager.dart';

import '../../services/api_service.dart';

/// Shows a modal dialog displaying live job progress received via WebSocket.
///
/// This is a reusable component used for download/import/create flows.
/// It subscribes to ApiService.progressEvents(jobId) and renders:
/// - LinearProgressIndicator with percentage when known
/// - Latest status message
/// - Optional filesDone / filesTotal when available
/// - Updates OS taskbar/dock progress via window_manager
Future<void> showJobProgressOverlayDialog({
  required BuildContext context,
  required ApiService api,
  required String jobId,
  required String title,
}) async {
  double? percent;
  String message = 'Starting...';
  String countsText = '';
  String speedText = '';
  bool cancelling = false;
  StreamSubscription? sub;

  // Track if an error occurred to redirect user after closing dialog
  bool hadError = false;
  String? errorMessage;

  // For computing speed when backend provides byte counters
  int? _lastBytes;
  DateTime? _lastTs;
  double? _speedBps; // bytes per second (smoothed)
  final List<Map<String, dynamic>> _hist = <Map<String, dynamic>>[]; // [{t: DateTime, b: int}]

  String _fmtSpeed(double bps) {
    if (bps.isNaN || !bps.isFinite) return '';
    const kb = 1024.0;
    const mb = kb * 1024.0;
    const gb = mb * 1024.0;
    if (bps >= gb) return (bps / gb).toStringAsFixed(2) + ' GB/s';
    if (bps >= mb) return (bps / mb).toStringAsFixed(2) + ' MB/s';
    if (bps >= kb) return (bps / kb).toStringAsFixed(1) + ' KB/s';
    return bps.toStringAsFixed(0) + ' B/s';
  }

  try {
    await showDialog<void>(
      context: context,
      barrierDismissible: false,
      builder: (ctx) {
        return StatefulBuilder(
          builder: (ctx, setStateSB) {
            sub ??= api.progressEvents(jobId).listen((ev) async {
              // Debug: log event as interpreted by UI
              final ptxtRaw = ev.progress == null ? 'null' : ev.progress!.toStringAsFixed(3);
              // ignore: avoid_print
              print('[UI][progress] job=$jobId phase=${ev.phase} message="${ev.message}" progress(raw)=$ptxtRaw');

              // Normalize progress to 0..100 regardless of backend scale (0..1 or 0..100)
              double? normalized;
              final raw = ev.progress;
              if (raw != null) {
                if (raw.isNaN) {
                  normalized = null; // treat as unknown
                } else if (raw <= 1.01) {
                  normalized = (raw * 100).clamp(0, 100);
                } else {
                  normalized = raw.clamp(0, 100);
                }
              }

              // Extract counts (downloaded/total files) from details if available
              int? filesDone;
              int? filesTotal;
              final d = ev.details;

              // Attempt to extract byte counters and/or speed from details
              int? bytesDone;
              int? bytesTotal;
              double? speedBps;

              if (d != null) {
                dynamic pick(List<String> keys) {
                  for (final k in keys) {
                    if (d.containsKey(k)) return d[k];
                  }
                  return null;
                }
                int? toInt(dynamic v) {
                  if (v == null) return null;
                  if (v is int) return v;
                  if (v is num) return v.toInt();
                  if (v is String) return int.tryParse(v);
                  return null;
                }
                double? toDouble(dynamic v) {
                  if (v == null) return null;
                  if (v is num) return v.toDouble();
                  if (v is String) return double.tryParse(v);
                  return null;
                }
                filesDone = toInt(pick(['downloaded_files', 'files_done', 'completed', 'current']));
                filesTotal = toInt(pick(['total_files', 'files_total', 'total']));

                bytesDone = toInt(pick(['bytes_done', 'downloaded_bytes', 'bytes_downloaded', 'current_bytes']));
                bytesTotal = toInt(pick(['bytes_total', 'total_bytes', 'total_size']));
                // Prefer explicit speed if present
                speedBps = toDouble(pick(['bytes_per_sec', 'bps', 'speed_bps', 'speed']));
              }

              // Compute speed from deltas when byte counters available; then smooth using a short moving average window
              if (bytesDone != null) {
                final now = DateTime.now();
                if (_lastBytes != null && _lastTs != null) {
                  final dtMs = now.difference(_lastTs!).inMilliseconds;
                  if (dtMs > 0) {
                    final db = bytesDone - _lastBytes!;
                    if (db >= 0) {
                      final inst = (db * 1000) / dtMs; // instantaneous bytes per second
                      if (speedBps == null) {
                        speedBps = inst;
                      }
                    }
                  }
                }
                _lastBytes = bytesDone;
                _lastTs = now;

                // Update history and compute moving average over the last ~5 seconds
                // Keep samples compact to avoid growth; prune old entries by time window
                const int windowMs = 5000;
                _hist.add({'t': now, 'b': bytesDone});
                // Prune by time
                while (_hist.isNotEmpty && now.difference(_hist.first['t'] as DateTime).inMilliseconds > windowMs) {
                  _hist.removeAt(0);
                }
                if (_hist.length >= 2) {
                  final DateTime t0 = _hist.first['t'] as DateTime;
                  final int b0 = _hist.first['b'] as int;
                  final DateTime t1 = _hist.last['t'] as DateTime;
                  final int b1 = _hist.last['b'] as int;
                  final int dt = t1.difference(t0).inMilliseconds;
                  if (dt > 250 && b1 >= b0) {
                    final avg = ((b1 - b0) * 1000) / dt; // bytes per second across window
                    speedBps = avg;
                  }
                }
              }

              // Fallback/override: derive progress from messages like "123 / 5851"
              double? fromCounts;
              try {
                final m = RegExp(r'\b(\d+)\s*/\s*(\d+)\b').firstMatch(ev.message);
                if (m != null) {
                  final cur = double.tryParse(m.group(1) ?? '');
                  final tot = double.tryParse(m.group(2) ?? '');
                  if (cur != null && tot != null && tot > 0 && cur >= 0 && cur <= tot) {
                    fromCounts = ((cur / tot) * 100).clamp(0, 100);
                    filesDone ??= cur.toInt();
                    filesTotal ??= tot.toInt();
                  }
                }
              } catch (_) {}
              // Prefer count-derived progress when available (more reliable for downloading phases)
              final effective = fromCounts ?? normalized;

              setStateSB(() {
                // Update in-dialog progress state
                percent = effective; // 0..100 scale
                message = ev.message.isNotEmpty ? ev.message : ev.phase;
                print("ev message: " + ev.message);
                print("ev phase: " + ev.phase);
                countsText = (filesDone != null && filesTotal != null) ? '${filesDone} / ${filesTotal} files' : '';
                _speedBps = speedBps ?? _speedBps; // keep last known if null
                // Only show speed during download-like phases and when known
                final isDownloading = ev.phase.toLowerCase().contains('download');
                speedText = (isDownloading && _speedBps != null && _speedBps! > 0)
                    ? _fmtSpeed(_speedBps!)
                    : '';
              });
              // Update OS-level window/taskbar progress if available
              if (effective != null) {
                final norm01 = (effective / 100.0);
                try { await windowManager.setProgressBar(norm01); } catch (_) {}
              }
              // Auto-close on error or when we clearly reach 100% or receive a done/cancel phase
              final ph = ev.phase.toLowerCase();
              final msgLower = ev.message.toLowerCase();
              final isErrorPhase = ph.contains('error') || ph.contains('fail');
              final isExplicitDownloadError = msgLower.contains('unable to download asset');
              if (isErrorPhase || isExplicitDownloadError) {
                hadError = true;
                errorMessage = ev.message.isNotEmpty ? ev.message : 'An error occurred';
                try { await windowManager.setProgressBar(-1); } catch (_) {}
                if (Navigator.of(ctx).canPop()) {
                  Navigator.of(ctx).pop();
                }
                return; // stop handling further for this event
              }
              if ((effective != null && effective >= 100.0) || ph == 'done' || ph == 'completed' || ph == 'cancel' || ph == 'cancelled') {
                try { await windowManager.setProgressBar(-1); } catch (_) {}
                if (Navigator.of(ctx).canPop()) {
                  Navigator.of(ctx).pop();
                }
              }
            });

            final p = (percent ?? 0).clamp(0, 100);
            return AlertDialog(
              title: Text(title),
              content: SizedBox(
                width: 420,
                child: Column(
                  mainAxisSize: MainAxisSize.min,
                  crossAxisAlignment: CrossAxisAlignment.stretch,
                  children: [
                    LinearProgressIndicator(value: percent != null ? (p / 100.0) : null),
                    const SizedBox(height: 12),
                    Row(
                      children: [
                        if (countsText.isNotEmpty) ...[
                          const SizedBox(width: 12),
                          Expanded(child: Text(countsText, style: Theme.of(context).textTheme.bodySmall?.copyWith(color: Theme.of(context).colorScheme.onSurfaceVariant))),
                          // Text(countsText, style: Theme.of(context).textTheme.bodySmall?.copyWith(color: Theme.of(context).colorScheme.onSurfaceVariant)),
                        ],
                        if (speedText.isNotEmpty) ...[
                          const SizedBox(width: 8),
                          Text(speedText, style: Theme.of(context).textTheme.bodySmall?.copyWith(color: Theme.of(context).colorScheme.onSurfaceVariant)),
                        ],
                        const SizedBox(width: 8),
                        if (percent != null) Text('${p.floor().toString()}%'),
                      ],
                    ),
                  ],
                ),
              ),
              actions: [
                TextButton.icon(
                  onPressed: cancelling ? null : () async {
                    setStateSB(() { cancelling = true; message = 'Cancelling…'; });
                    try { await api.cancelJob(jobId); } catch (_) {}
                    try { await windowManager.setProgressBar(-1); } catch (_) {}
                    if (Navigator.of(ctx).canPop()) { Navigator.of(ctx).pop(); }
                  },
                  icon: const Icon(Icons.cancel),
                  label: Text(cancelling ? 'Cancelling…' : 'Cancel'),
                ),
              ],
            );
          },
        );
      },
    );
  } finally {
    await sub?.cancel();
    try { await windowManager.setProgressBar(-1); } catch (_) {}
    // If an error occurred, inform the user and navigate back to main screen
    if (hadError) {
      try {
        final msg = (errorMessage == null || errorMessage!.isEmpty) ? 'An error occurred during the operation.' : errorMessage!;
        // Prefer SnackBar if a Scaffold is available; otherwise, show a dialog
        ScaffoldMessenger.maybeOf(context)?.showSnackBar(
          SnackBar(content: Text(msg), backgroundColor: Colors.redAccent),
        );
        // Pop to the first route (main screen)
        Navigator.of(context).popUntil((route) => route.isFirst);
      } catch (_) {}
    }
  }
}
