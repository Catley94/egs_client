import 'package:flutter/material.dart';

class FabSearchBar extends StatelessWidget {
  final TextEditingController controller;
  final String query;
  final ValueChanged<String> onChanged;
  final VoidCallback onClear;

  const FabSearchBar({
    super.key,
    required this.controller,
    required this.query,
    required this.onChanged,
    required this.onClear,
  });

  @override
  Widget build(BuildContext context) {
    return ConstrainedBox(
      constraints: const BoxConstraints(maxWidth: 420),
      child: TextField(
        controller: controller,
        onChanged: onChanged,
        decoration: InputDecoration(
          prefixIcon: const Icon(Icons.search),
          hintText: 'Search assets...',
          isDense: true,
          border: const OutlineInputBorder(),
          suffixIcon: query.isNotEmpty
              ? IconButton(
                  tooltip: 'Clear',
                  icon: const Icon(Icons.clear),
                  onPressed: onClear,
                )
              : null,
        ),
      ),
    );
  }
}
