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
  StreamSubscription? sub;
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
                filesDone = toInt(pick(['downloaded_files', 'files_done', 'completed', 'current']));
                filesTotal = toInt(pick(['total_files', 'files_total', 'total']));
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
              });
              // Update OS-level window/taskbar progress if available
              if (effective != null) {
                final norm01 = (effective / 100.0);
                try { await windowManager.setProgressBar(norm01); } catch (_) {}
              }
              // Auto-close when we clearly reach 100% or receive a done phase
              if ((effective != null && effective >= 100.0) || ev.phase.toLowerCase() == 'done' || ev.phase.toLowerCase() == 'completed') {
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
                        Expanded(child: Text(message, overflow: TextOverflow.ellipsis)),
                        if (countsText.isNotEmpty) ...[
                          const SizedBox(width: 12),
                          Text(countsText, style: Theme.of(context).textTheme.bodySmall?.copyWith(color: Theme.of(context).colorScheme.onSurfaceVariant)),
                        ],
                        const SizedBox(width: 8),
                        if (percent != null) Text('${p.floor().toString()}%'),
                      ],
                    ),
                  ],
                ),
              ),
            );
          },
        );
      },
    );
  } finally {
    await sub?.cancel();
    try { await windowManager.setProgressBar(-1); } catch (_) {}
  }
}
