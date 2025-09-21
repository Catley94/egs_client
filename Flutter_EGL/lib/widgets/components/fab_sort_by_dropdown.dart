import 'package:flutter/material.dart';

class FabSortByDropdown<T> extends StatelessWidget {
  final T value;
  final List<DropdownMenuItem<T>> items;
  final ValueChanged<T?> onChanged;

  const FabSortByDropdown({
    super.key,
    required this.value,
    required this.items,
    required this.onChanged,
  });

  @override
  Widget build(BuildContext context) {
    return ConstrainedBox(
      constraints: const BoxConstraints(maxWidth: 220),
      child: DropdownButtonFormField<T>(
        value: value,
        items: items,
        onChanged: onChanged,
        decoration: const InputDecoration(
          isDense: true,
          labelText: 'Sort by',
          border: OutlineInputBorder(),
        ),
      ),
    );
  }
}
