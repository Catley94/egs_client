import 'package:flutter/material.dart';
import 'my_projects_header.dart';
import 'project_tile.dart';
import 'version_prompt.dart';

class ProjectsList<TProject, TEngine> extends StatefulWidget {
  final Future<List<TProject>> projectsFuture;
  final List<TEngine> engines; // used to pick latest version
  final String Function(TProject) nameOf;
  final String Function(TProject) projectPathOf;
  final String Function(TProject) engineVersionOf;
  final String Function(TEngine) engineVersionOfEngine;
  final Future<({bool launched, String message})> Function({required String project, required String version}) openProject;
  final Future<({bool ok, String message})> Function({required String project, required String version}) setProjectVersion;
  final VoidCallback refreshProjects;
  final Color Function(int index) tileColorBuilder;

  const ProjectsList({
    super.key,
    required this.projectsFuture,
    required this.engines,
    required this.nameOf,
    required this.projectPathOf,
    required this.engineVersionOf,
    required this.engineVersionOfEngine,
    required this.openProject,
    required this.setProjectVersion,
    required this.refreshProjects,
    required this.tileColorBuilder,
  });

  @override
  State<ProjectsList<TProject, TEngine>> createState() => _ProjectsListState<TProject, TEngine>();
}

class _ProjectsListState<TProject, TEngine> extends State<ProjectsList<TProject, TEngine>> {
  bool _opening = false;


  @override
  Widget build(BuildContext context) {
    return Column(
      crossAxisAlignment: CrossAxisAlignment.start,
      children: [
        const MyProjectsHeader("My Projects"),
        const SizedBox(height: 10),
        LayoutBuilder(
          builder: (context, constraints) {
            const tileMinWidth = 95.0;
            const spacing = 8.0;
            final count = (constraints.maxWidth / (tileMinWidth + spacing)).floor().clamp(1, 8);

            return FutureBuilder<List<TProject>>(
              future: widget.projectsFuture,
              builder: (context, snapshot) {
                if (snapshot.connectionState == ConnectionState.waiting) {
                  return const Center(child: Padding(padding: EdgeInsets.all(24), child: CircularProgressIndicator()));
                }
                if (snapshot.hasError) {
                  return Padding(
                    padding: const EdgeInsets.all(8.0),
                    child: Text('Failed to load projects: ${snapshot.error}', style: const TextStyle(color: Colors.redAccent)),
                  );
                }
                final projects = snapshot.data ?? <TProject>[];
                if (projects.isEmpty) {
                  return const Padding(
                    padding: EdgeInsets.all(8.0),
                    child: Text('No projects found'),
                  );
                }
                return GridView.builder(
                  shrinkWrap: true,
                  physics: const NeverScrollableScrollPhysics(),
                  itemCount: projects.length,
                  gridDelegate: SliverGridDelegateWithFixedCrossAxisCount(
                    crossAxisCount: count,
                    mainAxisSpacing: spacing,
                    crossAxisSpacing: spacing,
                    childAspectRatio: 0.78,
                  ),
                  itemBuilder: (context, index) {
                    final p = projects[index];
                    final displayName = widget.nameOf(p);
                    final engineVersion = widget.engineVersionOf(p);
                    // Show Unreal Engine version badge on project tiles; keep help icon only when unknown
                    final versionLabel = engineVersion.isNotEmpty ? 'UE $engineVersion' : '';
                    final projPath = widget.projectPathOf(p);
                    final tile = ProjectTile(
                      name: displayName,
                      version: versionLabel,
                      color: widget.tileColorBuilder(index),
                      showName: true,
                      onHelpTap: engineVersion.isEmpty
                          ? () => showSetUnrealVersionDialog(
                                context: context,
                                projectPath: projPath,
                                setProjectVersion: widget.setProjectVersion,
                                refreshProjects: widget.refreshProjects,
                              )
                          : null,
                      onTap: () async {
                        if (_opening) return;
                        setState(() => _opening = true);
                        try {
                          String? version;
                          if (widget.engines.isNotEmpty) {
                            // choose last (assumed highest version)
                            final last = widget.engines.last;
                            final v = widget.engineVersionOfEngine(last);
                            version = v.isNotEmpty ? v : null;
                          }
                          if (version == null) {
                            if (!mounted) return;
                            ScaffoldMessenger.of(context).showSnackBar(
                              const SnackBar(content: Text('No installed Unreal Engine version found')),
                            );
                          } else {
                            final path = widget.projectPathOf(p);
                            final result = await widget.openProject(project: path, version: version);
                            if (!mounted) return;
                            final msg = result.message.isNotEmpty
                                ? result.message
                                : (result.launched ? 'Launched project' : 'Failed to launch project');
                            ScaffoldMessenger.of(context).showSnackBar(SnackBar(content: Text(msg)));
                          }
                        } catch (err) {
                          if (!mounted) return;
                          ScaffoldMessenger.of(context).showSnackBar(
                            SnackBar(content: Text('Error opening project: $err')),
                          );
                        } finally {
                          if (mounted) setState(() => _opening = false);
                        }
                      },
                    );
                    return tile;
                  },
                );
              },
            );
          },
        ),
      ],
    );
  }
}
