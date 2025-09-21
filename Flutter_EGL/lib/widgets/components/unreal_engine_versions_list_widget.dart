import 'package:flutter/material.dart';
import 'project_tile.dart';

class UnrealEngineVersionsList<T> extends StatefulWidget {
  final Future<List<T>> enginesFuture;
  final String Function(T item) nameOf;
  final String Function(T item) versionOf;
  final Future<({bool launched, String message})> Function(String version) openEngine;
  final Color Function(int index) tileColorBuilder;

  const UnrealEngineVersionsList({
    super.key,
    required this.enginesFuture,
    required this.nameOf,
    required this.versionOf,
    required this.openEngine,
    required this.tileColorBuilder,
  });

  @override
  State<UnrealEngineVersionsList<T>> createState() => _UnrealEngineVersionsListState<T>();
}

class _UnrealEngineVersionsListState<T> extends State<UnrealEngineVersionsList<T>> {
  bool _opening = false;

  @override
  Widget build(BuildContext context) {
    return LayoutBuilder(
      builder: (context, constraints) {
        const tileMinWidth = 95.0;
        const spacing = 8.0;
        final count = (constraints.maxWidth / (tileMinWidth + spacing)).floor().clamp(1, 8);

        return FutureBuilder<List<T>>(
          future: widget.enginesFuture,
          builder: (context, snapshot) {
            if (snapshot.connectionState == ConnectionState.waiting) {
              return const Center(
                child: Padding(
                  padding: EdgeInsets.all(24),
                  child: CircularProgressIndicator(),
                ),
              );
            }
            if (snapshot.hasError) {
              return Padding(
                padding: const EdgeInsets.all(8.0),
                child: Text(
                  'Failed to load engines: ${snapshot.error}',
                  style: const TextStyle(color: Colors.redAccent),
                ),
              );
            }

            final engines = snapshot.data ?? <T>[];
            if (engines.isEmpty) {
              return const Padding(
                padding: EdgeInsets.all(8.0),
                child: Text('No engines found'),
              );
            }

            return GridView.builder(
              shrinkWrap: true,
              physics: const NeverScrollableScrollPhysics(),
              itemCount: engines.length,
              gridDelegate: SliverGridDelegateWithFixedCrossAxisCount(
                crossAxisCount: count,
                mainAxisSpacing: spacing,
                crossAxisSpacing: spacing,
                childAspectRatio: 0.78,
              ),
              itemBuilder: (context, index) {
                final item = engines[index];
                final name = widget.nameOf(item);
                final versionRaw = widget.versionOf(item);
                final versionLabel = versionRaw.isEmpty ? 'unknown' : 'UE $versionRaw';

                return ProjectTile(
                  name: name,
                  version: versionLabel,
                  color: widget.tileColorBuilder(index),
                  onTap: () async {
                    if (_opening) return;
                    if (versionRaw.isEmpty) {
                      ScaffoldMessenger.of(context).showSnackBar(
                        const SnackBar(content: Text('Cannot open Unreal Engine: version is unknown')),
                      );
                      return;
                    }
                    setState(() => _opening = true);
                    try {
                      final result = await widget.openEngine(versionRaw);
                      if (!mounted) return;
                      final msg = result.message.isNotEmpty
                          ? result.message
                          : (result.launched ? 'Launched Unreal Engine' : 'Failed to launch Unreal Engine');
                      ScaffoldMessenger.of(context).showSnackBar(SnackBar(content: Text(msg)));
                    } catch (err) {
                      if (!mounted) return;
                      ScaffoldMessenger.of(context).showSnackBar(
                        SnackBar(content: Text('Error opening Unreal Engine: $err')),
                      );
                    } finally {
                      if (mounted) setState(() => _opening = false);
                    }
                  },
                );
              },
            );
          },
        );
      },
    );
  }
}