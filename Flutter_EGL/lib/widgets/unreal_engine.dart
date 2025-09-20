import 'package:flutter/material.dart';
import 'library_tab_widget.dart';
import '../theme/app_colors.dart';
// import 'package:test_app_ui/widgets/library_tab.dart';

class UnrealEngine extends StatelessWidget {
  const UnrealEngine({super.key});

  @override
  Widget build(BuildContext context) {
    final cs = Theme.of(context).colorScheme;
    return DefaultTabController(
      length: 1,
      child: Column(
        children: [
          Material(
            color: cs.surface,
            child: const SizedBox(
              height: 48,
              child: TabBar(
                isScrollable: false,
                indicatorSize: TabBarIndicatorSize.tab,
                tabs: [
                  Tab(text: 'Library'),
                ],
              ),
            ),
          ),
          Expanded(
            child: Container(
              color: AppColors.background,
              child: const TabBarView(
                physics: NeverScrollableScrollPhysics(),
                children: [
                  LibraryTab(),
                ],
              ),
            ),
          ),
        ],
      ),
    );
  }
}
