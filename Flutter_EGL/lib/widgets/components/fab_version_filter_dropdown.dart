import 'package:flutter/material.dart';
import '../../models/fab.dart';

class FabVersionFilterDropdown extends StatelessWidget {
  final Future<List<FabAsset>> fabFuture;
  final String value; // empty string means "All versions"
  final ValueChanged<String?> onChanged;

  const FabVersionFilterDropdown({
    super.key,
    required this.fabFuture,
    required this.value,
    required this.onChanged,
  });

  @override
  Widget build(BuildContext context) {
    return FutureBuilder<List<FabAsset>>(
      future: fabFuture,
      builder: (context, snapshot) {
        final assets = snapshot.data ?? const <FabAsset>[];
        final versions = <String>{};
        for (final a in assets) {
          for (final pv in a.projectVersions) {
            for (final ev in pv.engineVersions) {
              final parts = ev.split('_');
              if (parts.length > 1) {
                versions.add(parts[1]);
              }
            }
          }
        }
        int cmp(String a, String b) {
          int parseOrZero(String s) => int.tryParse(s) ?? 0;
          final as = a.split('.');
          final bs = b.split('.');
          final amaj = parseOrZero(as.isNotEmpty ? as[0] : '0');
          final amin = parseOrZero(as.length > 1 ? as[1] : '0');
          final bmaj = parseOrZero(bs.isNotEmpty ? bs[0] : '0');
          final bmin = parseOrZero(bs.length > 1 ? bs[1] : '0');
          if (amaj != bmaj) return bmaj.compareTo(amaj);
          return bmin.compareTo(amin);
        }
        final sorted = versions.toList()..sort(cmp);
        final items = <DropdownMenuItem<String>>[
          const DropdownMenuItem<String>(
            value: '',
            child: Text('All versions'),
          ),
          ...sorted.map((v) => DropdownMenuItem<String>(
                value: v,
                child: Text('UE $v'),
              )),
        ];

        return ConstrainedBox(
          constraints: const BoxConstraints(maxWidth: 200),
          child: DropdownButtonFormField<String>(
            initialValue: value.isEmpty ? '' : value,
            items: items,
            onChanged: onChanged,
            decoration: const InputDecoration(
              isDense: true,
              labelText: 'Filter by version',
              border: OutlineInputBorder(),
            ),
          ),
        );
      },
    );
  }
}
