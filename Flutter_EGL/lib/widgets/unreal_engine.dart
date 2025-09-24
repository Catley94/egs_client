import 'package:flutter/material.dart';
import 'library_tab_widget.dart';

class UnrealEngine extends StatelessWidget {
  const UnrealEngine({super.key});

  @override
  Widget build(BuildContext context) {
    final colorScheme = Theme.of(context).colorScheme;
    return Column(
      children: [
        Material(
          color: colorScheme.surface,
        ),
        Expanded(
          child: Container(
            color: Theme.of(context).colorScheme.surface,
            child: LibraryTab(),
          ),
        ),
      ],
    );
  }
}
