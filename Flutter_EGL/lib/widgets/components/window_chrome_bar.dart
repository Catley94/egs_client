
import 'package:flutter/foundation.dart';
import 'package:flutter/material.dart';
import 'package:window_manager/window_manager.dart' show WindowListener, windowManager, DragToMoveArea;

class WindowChromeBar extends StatefulWidget {
  const WindowChromeBar();

  @override
  State<WindowChromeBar> createState() => _WindowChromeBarState();
}

class _WindowChromeBarState extends State<WindowChromeBar> with WindowListener {
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

  @override
  Widget build(BuildContext context) {
    final colorScheme = Theme.of(context).colorScheme;
    return Container(
      height: 36,
      padding: const EdgeInsets.symmetric(horizontal: 8),
      decoration: BoxDecoration(
        color: colorScheme.surface,
        border: const Border(bottom: BorderSide(color: Color(0xFF1A2027))),
      ),
      child: Row(
        children: [
          // Draggable area: left/title region only
          Expanded(
            child: DragToMoveArea(
              child: Row(
                children: const [
                  SizedBox(width: 8),
                  Icon(Icons.format_underline, size: 16),
                  SizedBox(width: 8),
                  Expanded(
                    child: Align(
                      alignment: Alignment.centerLeft,
                      child: Text(
                        'Unreal Asset Manager',
                        overflow: TextOverflow.ellipsis,
                      ),
                    ),
                  ),
                ],
              ),
            ),
          ),
          // Control buttons (not draggable)
          _WinButton(
            tooltip: 'Close',
            icon: Icons.close,
            isClose: true,
            onPressed: () => windowManager.close(),
          ),
        ],
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
        child: Material(
          color: Colors.transparent,
          child: Material(
            color: Colors.transparent, // provides a Material ancestor for ink
            child: InkResponse(
              onTap: onPressed,
              radius: 18,
              containedInkWell: true,
              highlightShape: BoxShape.rectangle,
              highlightColor: Colors.red,
              hoverColor: Colors.blue,
              splashFactory: InkSplash.splashFactory,
              customBorder: RoundedRectangleBorder(
                borderRadius: BorderRadius.circular(4),
              ),
              child: Ink(
                width: 46,
                height: 28,
                child: Center(child: Icon(icon, size: 20)),
              ),
            ),
          ),
        )
    );
  }
}

class TopEdgeWindowDragOverlay extends StatelessWidget {
  const TopEdgeWindowDragOverlay({super.key});

  // Match the height of WindowChromeBar to avoid overlapping into content area
  static const double _dragHeight = 36.0; // Equals WindowChromeBar height
  static const double _dragWidth = 1345.0; // Left/title safe zone, away from Close button

  @override
  Widget build(BuildContext context) {
    return Positioned.fill(
      child: IgnorePointer(
        ignoring: false, // we want to receive gestures but let others through when not dragging
        child: TopDragGestureRegion(height: _dragHeight, width: _dragWidth),
      ),
    );
  }
}

class TopDragGestureRegion extends StatelessWidget {
  final double height;
  final double width;
  const TopDragGestureRegion({required this.height, required this.width});

  // bool _inDragZone(Offset pos) => ((pos.dy >= 0 && pos.dy <= height) && (pos.dx >= 0 && pos.dx <= width));

  bool _inDragZone(Offset pos) {
    final inY = pos.dy >= 0 && pos.dy <= height;
    final inX = pos.dx >= 0 && pos.dx <= width;
    final inZone = inX && inY;

    if (kDebugMode) {
      debugPrint(
        '[inDragZone] pos=(${pos.dx.toStringAsFixed(1)}, ${pos.dy.toStringAsFixed(1)}) '
            'bounds=(0..$width, 0..$height) '
            'inX=$inX inY=$inY -> $inZone',
      );
    }

    return inZone;

  }

    @override
  Widget build(BuildContext context) {
    // Use a Listener to inspect pointer positions before gestures resolve.
    return Listener(
      behavior: HitTestBehavior.translucent,
      onPointerDown: (event) async {
        // Only primary button and within top region.
        if ((event.buttons & 0x01) != 0) {
          if (kDebugMode) {
            debugPrint('pointerDown y=${event.position.dy}, x=${event.position.dx}');
          }
          if (_inDragZone(event.position)) {
            if (kDebugMode) debugPrint('inDragZone -> startDragging');
            try {
              await windowManager.startDragging();
            } catch (_) {
              // Ignore, not supported in this context
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