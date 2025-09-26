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
  int? lastBytes;
  DateTime? lastTs;
  double? speedBps0; // bytes per second (smoothed)
  final List<Map<String, dynamic>> hist = <Map<String, dynamic>>[]; // [{t: DateTime, b: int}]

  String fmtSpeed(double bps) {
    if (bps.isNaN || !bps.isFinite) return '';
    const kb = 1024.0;
    const mb = kb * 1024.0;
    const gb = mb * 1024.0;
    if (bps >= gb) return '${(bps / gb).toStringAsFixed(2)} GB/s';
    if (bps >= mb) return '${(bps / mb).toStringAsFixed(2)} MB/s';
    if (bps >= kb) return '${(bps / kb).toStringAsFixed(1)} KB/s';
    return '${bps.toStringAsFixed(0)} B/s';
  }

  try {
    // Use a stable root navigator to avoid looking up with a deactivated dialog context
    final NavigatorState rootNav = Navigator.of(context, rootNavigator: true);
    await showDialog<void>(
      context: context,
      barrierDismissible: false,
      builder: (ctx) {
        return StatefulBuilder(
          builder: (ctx, setStateSB) {
            sub ??= api.progressEvents(jobId).listen((ev) async {

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
                if (lastBytes != null && lastTs != null) {
                  final dtMs = now.difference(lastTs!).inMilliseconds;
                  if (dtMs > 0) {
                    final db = bytesDone - lastBytes!;
                    if (db >= 0) {
                      final inst = (db * 1000) / dtMs; // instantaneous bytes per second
                      speedBps ??= inst;
                    }
                  }
                }
                lastBytes = bytesDone;
                lastTs = now;

                // Update history and compute moving average over the last ~5 seconds
                // Keep samples compact to avoid growth; prune old entries by time window
                const int windowMs = 5000;
                hist.add({'t': now, 'b': bytesDone});
                // Prune by time
                while (hist.isNotEmpty && now.difference(hist.first['t'] as DateTime).inMilliseconds > windowMs) {
                  hist.removeAt(0);
                }
                if (hist.length >= 2) {
                  final DateTime t0 = hist.first['t'] as DateTime;
                  final int b0 = hist.first['b'] as int;
                  final DateTime t1 = hist.last['t'] as DateTime;
                  final int b1 = hist.last['b'] as int;
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
                countsText = (filesDone != null && filesTotal != null) ? '$filesDone / $filesTotal files' : '';
                speedBps0 = speedBps ?? speedBps0; // keep last known if null
                // Only show speed during download-like phases and when known
                final isDownloading = ev.phase.toLowerCase().contains('download');
                speedText = (isDownloading && speedBps0 != null && speedBps0! > 0)
                    ? fmtSpeed(speedBps0!)
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
                if (rootNav.canPop()) {
                  rootNav.pop();
                }
                return; // stop handling further for this event
              }
              if ((effective != null && effective >= 100.0) || ph == 'done' || ph == 'completed' || ph == 'cancel' || ph == 'cancelled') {
                try { await windowManager.setProgressBar(-1); } catch (_) {}
                if (rootNav.canPop()) {
                  rootNav.pop();
                }
              }
            });

            // Normalize percent to 0..1 for the progress indicator; keep null for indeterminate
            final double? p01 = percent != null ? ((percent!.clamp(0.0, 100.0)) / 100.0) : null;

            // Avoid duplicating counts like "23 / 128" in the message line when we already
            // show a nice "23 / 128 files" summary above.
            // Note: This is unrelated to RepaintBoundary. RepaintBoundary only controls how a
            // subtree repaints; it does not create duplicate widgets. We keep this suppression
            // because some backends send the numeric fraction as the status message at the same
            // time we also have explicit counts available. In that case, showing both would look
            // like a duplicate even though it’s coming from two different sources.
            final bool messageIsJustCounts = RegExp(r'^\s*\d+\s*/\s*\d+\s*$').hasMatch(message);
            final String? messageToShow = (countsText.isNotEmpty && messageIsJustCounts)
                ? null
                : (message.trim().isEmpty ? null : message.trim());

            return AlertDialog(
              title: Text(title),
              content: SizedBox(
                width: 420,
                child: RepaintBoundary(
                  child: Column(
                    mainAxisSize: MainAxisSize.min,
                    crossAxisAlignment: CrossAxisAlignment.stretch,
                    children: [
                      // RepaintBoundary note:
                      // The entire dialog content is wrapped in a RepaintBoundary so the whole progress
                      // component repaints as a unit. RepaintBoundary does not duplicate widgets; it only
                      // creates a separate layer for repaint isolation.
                      LinearProgressIndicator(
                        key: ValueKey<double?>(p01),
                        value: p01,
                        minHeight: 4,
                        backgroundColor: Theme.of(context).colorScheme.surfaceVariant,
                        valueColor: AlwaysStoppedAnimation<Color>(Theme.of(context).colorScheme.primary),
                      ),
                      const SizedBox(height: 12),
                      Row(
                        children: [
                          if (countsText.isNotEmpty) ...[
                            const SizedBox(width: 12),
                            Expanded(child: Text(countsText, style: Theme.of(context).textTheme.bodySmall?.copyWith(color: Theme.of(context).colorScheme.onSurfaceVariant))),
                          ],
                          if (speedText.isNotEmpty) ...[
                            const SizedBox(width: 8),
                            Text(speedText, style: Theme.of(context).textTheme.bodySmall?.copyWith(color: Theme.of(context).colorScheme.onSurfaceVariant)),
                          ],
                          const SizedBox(width: 8),
                          if (percent != null) Text('${(percent!.clamp(0.0, 100.0) as double).floor().toString()}%'),
                        ],
                      ),
                      if (messageToShow != null) ...[
                        const SizedBox(height: 8),
                        Text(messageToShow, maxLines: 2, overflow: TextOverflow.ellipsis, style: Theme.of(context).textTheme.bodySmall),
                      ],
                    ],
                  ),
                ),
              ),
              actions: [
                TextButton.icon(
                  onPressed: cancelling ? null : () async {
                    setStateSB(() { cancelling = true; message = 'Cancelling…'; });
                    try { await api.cancelJob(jobId); } catch (_) {}
                    try { await windowManager.setProgressBar(-1); } catch (_) {}
                    if (rootNav.canPop()) { rootNav.pop(); }
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
