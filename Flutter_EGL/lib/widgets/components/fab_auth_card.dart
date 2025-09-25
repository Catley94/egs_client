import 'package:flutter/material.dart';
import 'package:url_launcher/url_launcher.dart';
import 'package:flutter/services.dart';

class FabAuthCard extends StatelessWidget {
  final String authUrl;
  final String? message;
  final TextEditingController controller;
  final Future<void> Function() onSubmit;
  final bool isWorking;

  const FabAuthCard({
    super.key,
    required this.authUrl,
    this.message,
    required this.controller,
    required this.onSubmit,
    this.isWorking = false,
  });

  @override
  Widget build(BuildContext context) {
    final cs = Theme.of(context).colorScheme;
    return Padding(
      padding: const EdgeInsets.all(24.0),
      child: Center(
        child: ConstrainedBox(
          constraints: const BoxConstraints(maxWidth: 720),
          child: Container(
            padding: const EdgeInsets.all(20),
            decoration: BoxDecoration(
              color: cs.surface,
              borderRadius: BorderRadius.circular(12),
              border: Border.all(color: const Color(0xFF1A2027)),
            ),
            child: Column(
              mainAxisSize: MainAxisSize.min,
              crossAxisAlignment: CrossAxisAlignment.start,
              children: [
                Row(
                  children: [
                    const Icon(Icons.lock_open, size: 28),
                    const SizedBox(width: 8),
                    Text('Sign in required', style: Theme.of(context).textTheme.titleMedium?.copyWith(fontWeight: FontWeight.bold)),
                  ],
                ),
                const SizedBox(height: 12),
                Text(
                  message ??
                      'To view your Fab Library, sign in to Epic Games in your web browser. After signing in, the page will show a JSON with an authorizationCode. Paste that code here to complete login.',
                  style: Theme.of(context).textTheme.bodyMedium,
                ),
                const SizedBox(height: 16),
                Column(
                  crossAxisAlignment: CrossAxisAlignment.start,
                  children: [
                    Row(
                      children: [
                        FilledButton.icon(
                          icon: const Icon(Icons.open_in_browser),
                          label: const Text('Open Epic login in browser'),
                          onPressed: () async {
                            final uri = Uri.parse(authUrl);
                            if (!await launchUrl(uri, mode: LaunchMode.externalApplication)) {
                              if (context.mounted) {
                                ScaffoldMessenger.of(context).showSnackBar(
                                  const SnackBar(content: Text('Failed to open browser. Copy the URL manually.')),
                                );
                              }
                            }
                          },
                        ),
                        const SizedBox(width: 12),
                        TextButton.icon(
                          onPressed: () async {
                            final uri = Uri.parse(authUrl);
                            await Clipboard.setData(ClipboardData(text: uri.toString()));
                            if (context.mounted) {
                              ScaffoldMessenger.of(context).showSnackBar(
                                const SnackBar(content: Text('Login URL copied to clipboard')),
                              );
                            }
                          },
                          icon: const Icon(Icons.copy),
                          label: const Text('Copy URL'),
                        ),
                      ],
                    ),
                    const SizedBox(height: 8),
                    SelectableText('Login URL: $authUrl', style: Theme.of(context).textTheme.bodySmall?.copyWith(color: cs.onSurfaceVariant)),
                    const SizedBox(height: 16),
                    Text('Paste authorizationCode here:', style: Theme.of(context).textTheme.bodySmall),
                    const SizedBox(height: 8),
                    Row(
                      children: [
                        Expanded(
                          child: TextField(
                            controller: controller,
                            decoration: const InputDecoration(
                              labelText: 'authorizationCode',
                              hintText: 'Paste the code here',
                              border: OutlineInputBorder(),
                            ),
                            onSubmitted: (_) async { await onSubmit(); },
                          ),
                        ),
                        const SizedBox(width: 12),
                        FilledButton.icon(
                          onPressed: isWorking ? null : () async { await onSubmit(); },
                          icon: isWorking ? const SizedBox(width: 16, height: 16, child: CircularProgressIndicator(strokeWidth: 2)) : const Icon(Icons.login),
                          label: const Text('Submit'),
                        ),
                      ],
                    ),
                  ],
                ),
              ],
            ),
          ),
        ),
      ),
    );
  }
}
