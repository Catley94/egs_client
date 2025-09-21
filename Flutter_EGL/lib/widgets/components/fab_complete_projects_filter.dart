import 'package:flutter/material.dart';

class FabCompleteProjectsFilter extends StatelessWidget {
  final bool selected;
  final ValueChanged<bool> onChanged;

  const FabCompleteProjectsFilter({
    super.key,
    required this.selected,
    required this.onChanged,
  });

  @override
  Widget build(BuildContext context) {
    return FilterChip(
      label: const Text('Complete projects only'),
      selected: selected,
      onSelected: onChanged,
    );
  }
}
