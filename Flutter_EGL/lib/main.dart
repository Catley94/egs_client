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

Future<void> main() async {
  WidgetsFlutterBinding.ensureInitialized();
  // Initialize the image cache so CachedNetworkImage uses a deterministic directory
  await AppImageCache.init();

  if (!kIsWeb && (Platform.isWindows || Platform.isLinux || Platform.isMacOS)) {
    const fixedSize = Size(1400, 800); // pick your fixed size
    // Optional: set initial position/size before locking
    // window_size.setWindowFrame(const Rect.fromLTWH(100, 100, fixedSize.width, fixedSize.height));
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
      if (Platform.isWindows || Platform.isLinux) {
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
                if (!kIsWeb && (Platform.isWindows || Platform.isLinux || Platform.isMacOS))
                  const _TopEdgeWindowDragOverlay(),
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
          home: const NavRailExample(),
        );
      },
    );
  }
}

class _TopEdgeWindowDragOverlay extends StatelessWidget {
  const _TopEdgeWindowDragOverlay({super.key});

  static const double _dragHeight = 56.0; // Top region (including app title area)

  @override
  Widget build(BuildContext context) {
    return Positioned.fill(
      child: IgnorePointer(
        ignoring: false, // we want to receive gestures but let others through when not dragging
        child: _TopDragGestureRegion(height: _dragHeight),
      ),
    );
  }
}

class _TopDragGestureRegion extends StatelessWidget {
  final double height;
  const _TopDragGestureRegion({required this.height});

  bool _inDragZone(Offset pos) => pos.dy >= 0 && pos.dy <= height;

  @override
  Widget build(BuildContext context) {
    // Use a Listener to inspect pointer positions before gestures resolve.
    return Listener(
      behavior: HitTestBehavior.translucent,
      onPointerDown: (event) async {
        // Only primary button and within top region.
        // Primary mouse button pressed
        if ((event.buttons & 0x01) != 0) {
          if (_inDragZone(event.position)) {
            try {
              await windowManager.startDragging();
            } catch (_) {
              // ignore; not supported in this context
            }
          }
        }
      },
      onPointerMove: (event) async {
        // If user presses then moves quickly into the zone, still allow starting drag.
        // If mouse is down and pointer is within the top zone, initiate dragging.
        if ((event.buttons & 0x01) != 0) {
          if (_inDragZone(event.position)) {
            try {
              await windowManager.startDragging();
            } catch (_) {}
          }
        }
      },
      child: const SizedBox.expand(),
    );
  }
}

class NavRailExample extends StatefulWidget {
  const NavRailExample({super.key});

  @override
  State<NavRailExample> createState() => _NavRailExampleState();
}

class _NavRailExampleState extends State<NavRailExample> {
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
    final cs = Theme.of(context).colorScheme;
    return Scaffold(
      body: Column(
        children: [
          if (!kIsWeb && (Platform.isWindows || Platform.isLinux || Platform.isMacOS))
            const _WindowChromeBar(),
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
                    backgroundColor: cs.surface,
                    extended: _railExpanded,
                    minExtendedWidth: 220,
                    indicatorColor: cs.primary.withOpacity(0.18),
                    selectedIconTheme: IconThemeData(color: cs.primary, size: 24),
                    unselectedIconTheme:
                        IconThemeData(color: cs.onSurfaceVariant, size: 24),
                    selectedLabelTextStyle: TextStyle(
                      color: cs.primary,
                      fontWeight: FontWeight.w600,
                    ),
                    unselectedLabelTextStyle: TextStyle(
                      color: cs.onSurfaceVariant,
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
                        icon: Icon(Icons.bookmark_border),
                        selectedIcon: Icon(Icons.bookmark),
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
                            color: cs.background,
                            // In dark mode we used to show a subtle gradient; for clarity and
                            // theme consistency we use plain background from ColorScheme.
                            // If a gradient is desired in dark mode only, we can re-introduce it
                            // conditionally.
                            // decoration: Theme.of(context).brightness == Brightness.dark
                            //     ? const BoxDecoration(
                            //         gradient: LinearGradient(
                            //           begin: Alignment.topCenter,
                            //           end: Alignment.bottomCenter,
                            //           colors: [
                            //             Color(0x11182532),
                            //             Color(0x000F1115),
                            //           ],
                            //         ),
                            //       )
                            //     : null,
                            child: ClipRRect(
                              borderRadius: BorderRadius.circular(12),
                              child: Container(
                                color: cs.surface,
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

class _WindowChromeBar extends StatefulWidget {
  const _WindowChromeBar();

  @override
  State<_WindowChromeBar> createState() => _WindowChromeBarState();
}

class _WindowChromeBarState extends State<_WindowChromeBar> with WindowListener {
  bool _isMaximized = false;

  @override
  void initState() {
    super.initState();
    windowManager.addListener(this);
    _refreshState();
  }

  @override
  void dispose() {
    windowManager.removeListener(this);
    super.dispose();
  }

  Future<void> _refreshState() async {
    if (!mounted) return;
    final maximized = await windowManager.isMaximized();
    setState(() => _isMaximized = maximized);
  }

  // WindowListener override
  @override
  void onWindowMaximize() => _refreshState();
  @override
  void onWindowUnmaximize() => _refreshState();

  Future<void> _toggleMaxRestore() async {
    if (_isMaximized) {
      await windowManager.unmaximize();
    } else {
      await windowManager.maximize();
    }
    _refreshState();
  }

  @override
  Widget build(BuildContext context) {
    final cs = Theme.of(context).colorScheme;
    return GestureDetector(
      behavior: HitTestBehavior.opaque,
      onPanStart: (_) => windowManager.startDragging(),
      child: Container(
        height: 36,
        padding: const EdgeInsets.symmetric(horizontal: 8),
        decoration: BoxDecoration(
          color: cs.surface,
          border: const Border(bottom: BorderSide(color: Color(0xFF1A2027))),
        ),
        child: Row(
          children: [
            const SizedBox(width: 8),
            const Icon(Icons.apps, size: 16),
            const SizedBox(width: 8),
            const Expanded(
              child: Align(
                alignment: Alignment.centerLeft,
                child: Text(
                  'Unreal Asset Manager',
                  overflow: TextOverflow.ellipsis,
                ),
              ),
            ),
            // Control buttons
            _WinButton(
              tooltip: 'Close',
              icon: Icons.close,
              isClose: true,
              onPressed: () => windowManager.close(),
            ),
          ],
        ),
      ),
    );
  }
}

class _WinButton extends StatelessWidget {
  final String tooltip;
  final IconData icon;
  final VoidCallback onPressed;
  final bool isClose;
  const _WinButton({required this.tooltip, required this.icon, required this.onPressed, this.isClose = false});

  @override
  Widget build(BuildContext context) {
    return Tooltip(
      message: tooltip,
      child: InkResponse(
        onTap: onPressed,
        radius: 18,
        highlightShape: BoxShape.rectangle,
        child: Container(
          width: 46,
          height: 28,
          alignment: Alignment.center,
          child: Icon(icon, size: 16),
        ),
        onHover: (_) {},
        containedInkWell: true,
        splashFactory: InkSplash.splashFactory,
      ),
    );
  }
}
