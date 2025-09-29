import 'package:flutter/material.dart';

class ProjectTile extends StatefulWidget {
  final String name;
  final String version;
  final bool showName;
  final Color color;
  final VoidCallback? onTap;
  final VoidCallback? onHelpTap;

  const ProjectTile({
    super.key,
    required this.name,
    required this.version,
    required this.color,
    required this.showName,
    this.onTap,
    this.onHelpTap,
  });

  @override
  State<ProjectTile> createState() => _ProjectTileState();
}

class _ProjectTileState extends State<ProjectTile> {
  bool _helpHover = false;

  @override
  Widget build(BuildContext context) {
    final cs = Theme.of(context).colorScheme;
    final bgStart = Color.lerp(widget.color, Colors.black, 0.20)!;
    final bgEnd = Color.lerp(widget.color, Colors.white, 0.06)!;

    Widget unrealBadge({double size = 36, double opacity = 0.10}) {
      return Opacity(
        opacity: opacity,
        child: Container(
          width: size,
          height: size,
          decoration: BoxDecoration(
            shape: BoxShape.circle,
            color: Colors.white.withOpacity(0.04),
            border: Border.all(color: Colors.white.withOpacity(0.15), width: 1.0),
          ),
          alignment: Alignment.center,
          child: Text(
            'U',
            style: TextStyle(
              fontSize: size * 0.6,
              fontWeight: FontWeight.w800,
              color: Colors.white.withOpacity(0.8),
              height: 1.0,
            ),
          ),
        ),
      );
    }

    return Column(
      crossAxisAlignment: CrossAxisAlignment.start,
      children: [
        AspectRatio(
          aspectRatio: 1,
          child: Stack(
            children: [
              Container(
                decoration: BoxDecoration(
                  borderRadius: BorderRadius.circular(12),
                  border: Border.all(color: cs.outlineVariant),
                  gradient: LinearGradient(
                    begin: Alignment.topLeft,
                    end: Alignment.bottomRight,
                    colors: [bgStart, bgEnd],
                  ),
                ),
              ),
              Positioned(
                left: -30,
                top: -30,
                child: Container(
                  width: 120,
                  height: 120,
                  decoration: BoxDecoration(
                    shape: BoxShape.circle,
                    color: widget.color.withOpacity(0.10),
                  ),
                ),
              ),
              Center(child: unrealBadge(size: 56, opacity: 0.18)),
              Positioned(
                left: 0,
                right: 0,
                top: 0,
                bottom: 0,
                child: Material(
                  color: Colors.transparent,
                  borderRadius: BorderRadius.circular(12),
                  child: InkWell(
                    borderRadius: BorderRadius.circular(12),
                    onTap: widget.onTap,
                  ),
                ),
              ),
              if (widget.onHelpTap != null)
                Positioned(
                  right: 8,
                  top: 8,
                  child: MouseRegion(
                    cursor: SystemMouseCursors.click,
                    onEnter: (_) => setState(() => _helpHover = true),
                    onExit: (_) => setState(() => _helpHover = false),
                    child: Tooltip(
                      message: 'Set Unreal Engine version',
                      child: Material(
                        color: Colors.transparent,
                        shape: RoundedRectangleBorder(borderRadius: BorderRadius.circular(6)),
                        child: InkWell(
                          onTap: widget.onHelpTap,
                          borderRadius: BorderRadius.circular(6),
                          hoverColor: cs.primary.withOpacity(0.08),
                          splashColor: cs.primary.withOpacity(0.12),
                          child: AnimatedContainer(
                            duration: const Duration(milliseconds: 120),
                            width: 32,
                            height: 32,
                            decoration: BoxDecoration(
                              color: _helpHover ? cs.surfaceVariant.withOpacity(0.95) : cs.surface.withOpacity(0.92),
                              borderRadius: BorderRadius.circular(6),
                              border: Border.all(color: _helpHover ? cs.primary.withOpacity(0.7) : cs.outlineVariant.withOpacity(0.6)),
                              boxShadow: [
                                BoxShadow(color: Colors.black.withOpacity(_helpHover ? 0.30 : 0.25), blurRadius: _helpHover ? 6 : 4, offset: const Offset(0, 1)),
                              ],
                            ),
                            alignment: Alignment.center,
                            child: Icon(Icons.help_outline, size: 18, color: _helpHover ? cs.primary : cs.onSurface.withOpacity(0.9)),
                          ),
                        ),
                      ),
                    ),
                  ),
                ),
              if (widget.version.trim().isNotEmpty)
                Positioned(
                  right: 8,
                  bottom: 8,
                  child: Container(
                    padding: const EdgeInsets.symmetric(horizontal: 8, vertical: 4),
                    decoration: BoxDecoration(
                      color: cs.surface.withOpacity(0.85),
                      borderRadius: BorderRadius.circular(20),
                      border: Border.all(color: cs.outlineVariant.withOpacity(0.5)),
                    ),
                    child: Row(
                      mainAxisSize: MainAxisSize.min,
                      children: [
                        Padding(
                          padding: const EdgeInsets.only(right: 6),
                          child: unrealBadge(size: 16, opacity: 1.0),
                        ),
                        Text(
                          widget.version,
                          style: Theme.of(context).textTheme.labelSmall?.copyWith(
                                fontWeight: FontWeight.w800,
                              ) ?? const TextStyle(fontSize: 11, fontWeight: FontWeight.w800),
                        ),
                      ],
                    ),
                  ),
                ),
            ],
          ),
        ),
        const SizedBox(height: 8),
        if (widget.showName)
          Text(
            widget.name,
            maxLines: 1,
            overflow: TextOverflow.ellipsis,
            style: Theme.of(context).textTheme.bodyMedium?.copyWith(
                  fontWeight: FontWeight.w600,
                ),
          ),
      ],
    );
  }
}
