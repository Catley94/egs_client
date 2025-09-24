// dart
import 'dart:io' show Platform;
import 'package:flutter/foundation.dart' show kIsWeb, kDebugMode;
import 'package:flutter/material.dart';
import 'services/image_cache.dart';
import 'package:window_size/window_size.dart' as window_size;
import 'package:window_manager/window_manager.dart';
import 'package:test_app_ui/widgets/unreal_engine.dart';
import 'theme/app_theme.dart';
import 'theme/theme_controller.dart';
import 'widgets/components/window_chrome_bar.dart';

Future<void> main() async {
  WidgetsFlutterBinding.ensureInitialized();
  // Initialize the image cache so CachedNetworkImage uses a deterministic directory
  await AppImageCache.init();

  if (!kIsWeb && Platform.isLinux) {

    const fixedSize = Size(1400, 800);

    window_size.setWindowMinSize(fixedSize);
    window_size.setWindowMaxSize(fixedSize);

    // Initialize custom window management with hidden system title bar
    await windowManager.ensureInitialized();
    const options = WindowOptions(
      size: fixedSize,
      center: true,
      titleBarStyle: TitleBarStyle.hidden,
      windowButtonVisibility: false,
    );
    await windowManager.waitUntilReadyToShow(options, () async {
      if (Platform.isLinux) {
        await windowManager.setAsFrameless();
      }
      await windowManager.show();
      await windowManager.focus();
    });
  }

  runApp(const NavigationRailExampleApp());
}

class NavigationRailExampleApp extends StatelessWidget {
  const NavigationRailExampleApp({super.key});

  @override
  Widget build(BuildContext context) {
    return ListenableBuilder(
      listenable: ThemeController.instance.mode,
      builder: (context, _) {
        return MaterialApp(
          debugShowCheckedModeBanner: false,
          builder: (context, child) {
            if (child == null) return const SizedBox.shrink();
            return Stack(
              children: [
                child,
                // Global top-edge drag overlay to move the native window even when overlays are shown.
                if (!kIsWeb && Platform.isLinux)
                  const TopEdgeWindowDragOverlay(),
                if (kDebugMode)
                  Positioned(
                    left: 0,
                    bottom: 0,
                    child: IgnorePointer(
                      child: Banner(
                        message: 'DEBUG',
                        location: BannerLocation.bottomStart,
                      ),
                    ),
                  ),
              ],
            );
          },
          theme: AppTheme.light(),
          darkTheme: AppTheme.dark(),
          themeMode: ThemeController.instance.mode.value,
          home: const UNavRail(),
        );
      },
    );
  }
}

class UNavRail extends StatefulWidget {
  const UNavRail({super.key});

  @override
  State<UNavRail> createState() => _UNavRailState();
}

class _UNavRailState extends State<UNavRail> {
  int _selectedIndex = 0;
  double groupAlignment = -1.0;

  bool _railExpanded = false;

  // Add a list of widgets for the main content
  final List<Widget> _mainContents = const [
    // Unreal Engine only
    UnrealEngine(),
  ];

  @override
  Widget build(BuildContext context) {
    final colorScheme = Theme.of(context).colorScheme;
    return Scaffold(
      body: Column(
        children: [
          if (!kIsWeb && Platform.isLinux)
            const WindowChromeBar(),
          Expanded(
            child: SafeArea(
              child: Row(
                children: <Widget>[
                  NavigationRail(
                    selectedIndex: _selectedIndex,
                    groupAlignment: groupAlignment,
                    onDestinationSelected: (int index) {
                      setState(() {
                        _selectedIndex = 0; // only one tab (Unreal Engine)
                      });
                    },
                    // Ensure labelType is none when extended to satisfy assertion
                    labelType: _railExpanded
                        ? NavigationRailLabelType.none
                        : NavigationRailLabelType.all,
                    backgroundColor: colorScheme.surface,
                    extended: _railExpanded,
                    minExtendedWidth: 220,
                    indicatorColor: colorScheme.primary.withOpacity(0.18),
                    selectedIconTheme: IconThemeData(color: colorScheme.primary, size: 24),
                    unselectedIconTheme:
                        IconThemeData(color: colorScheme.onSurfaceVariant, size: 24),
                    selectedLabelTextStyle: TextStyle(
                      color: colorScheme.primary,
                      fontWeight: FontWeight.w600,
                    ),
                    unselectedLabelTextStyle: TextStyle(
                      color: colorScheme.onSurfaceVariant,
                    ),
                    leading: Padding(
                      padding: const EdgeInsets.only(top: 8.0),
                      child: Column(
                        children: [
                          SizedBox(
                            height: 40,
                            child: Row(
                              mainAxisAlignment: _railExpanded
                                  ? MainAxisAlignment.spaceBetween
                                  : MainAxisAlignment.center,
                              children: [
                                if (_railExpanded)
                                  Container(
                                    padding:
                                        const EdgeInsets.symmetric(horizontal: 12),
                                    alignment: Alignment.centerLeft,
                                    child: const Text(
                                      'EPIC',
                                      style: TextStyle(
                                        fontWeight: FontWeight.w800,
                                        letterSpacing: 1.2,
                                      ),
                                    ),
                                  ),
                              ],
                            ),
                          ),
                          const SizedBox(height: 8),
                        ],
                      ),
                    ),
                    trailing: const SizedBox.shrink(),
                    destinations: const <NavigationRailDestination>[
                      NavigationRailDestination(
                        icon: Icon(Icons.format_underline),
                        selectedIcon: Icon(Icons.format_underline),
                        label: Text('Unreal Engine'),
                      ),
                    ],
                  ),
                  const VerticalDivider(thickness: 1, width: 1),
                  // This is the main content.
                  Expanded(
                    child: Column(
                      children: [
                        // Content area
                        Expanded(
                          child: Container(
                            width: double.infinity,
                            padding: const EdgeInsets.all(16),
                            color: colorScheme.surface,
                            child: ClipRRect(
                              borderRadius: BorderRadius.circular(12),
                              child: Container(
                                color: colorScheme.surface,
                                child: _mainContents[_selectedIndex],
                              ),
                            ),
                          ),
                        ),
                      ],
                    ),
                  ),
                ],
              ),
            ),
          ),
        ],
      ),
    );
  }
}


