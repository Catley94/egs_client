Unreal Engine on Linux: How to obtain and install

Short answer
- Epic does not distribute official, prebuilt Linux binaries of the Unreal Editor via the Epic Games Launcher or a public download page. The supported way is to build from source on Linux via GitHub after linking your Epic Games and GitHub accounts.
- You can try third‑party/workaround approaches (e.g., running the Windows editor under Wine/Proton via Heroic/Lutris), but these are unofficial and often fragile. Use at your own risk.

Option A — Build from source via GitHub (official for Linux)
1) Link your Epic Games account with GitHub
   - Sign in to Epic Games (https://www.epicgames.com/)
   - Go to Account settings → Connections → Apps → Link your GitHub account
   - Accept the Unreal Engine EULA when prompted.
   - After linking, you’ll gain access to the private EpicGames/UnrealEngine repository on GitHub.

2) Clone the UnrealEngine repository
   - Ensure you have Git and required build tools (see docs below).
   - Example (choose a version branch):
     git clone -b 5.4 https://github.com/EpicGames/UnrealEngine.git
     # or via SSH if your GitHub is configured:
     # git clone -b 5.4 git@github.com:EpicGames/UnrealEngine.git

3) Read the official Linux build docs
   - See the repo’s README.md and Engine/Build/BatchFiles/Linux for up‑to‑date instructions.
   - Typical steps from the repo root:
     ./Setup.sh
     ./GenerateProjectFiles.sh
     # Build the editor (Development):
     make -j"$(nproc)"
     # or explicitly via build script:
     ./Engine/Build/BatchFiles/Linux/Build.sh UnrealEditor Linux Development

4) Launch the editor
   - Binary usually ends up here:
     ./Engine/Binaries/Linux/UnrealEditor
   - Run it directly:
     ./Engine/Binaries/Linux/UnrealEditor

5) Optional: Place/organize engines for this app (egs_client)
   - By default, this project looks for engines under:
     $HOME/UnrealEngines
   - You can move/clone your built engine to something like:
     $HOME/UnrealEngines/UE_5.4
   - Or configure a custom path via either:
     • Environment variable: EGS_UNREAL_ENGINES_DIR=/path/to/engines
     • Paths config API: POST /set-paths-config with { "engines_dir": "/path/to/engines" }

Option B — Epic Games Launcher (Windows) under Wine/Proton
- The Epic Games Launcher is not available natively on Linux. Some users install the Windows launcher through Proton/Wine (e.g., Heroic Games Launcher or Lutris), and then attempt to install the Windows Unreal Editor.
- Caveats:
  • Not officially supported by Epic for the editor on Linux.
  • Large downloads, heavy disk I/O, and various compatibility issues are common.
  • Even if installation succeeds, the Windows build targets Windows and won’t provide native Linux editor binaries.

What about downloading from Epic’s website directly?
- Epic provides source access and official binary installers for Windows/macOS via the Launcher.
- For Linux, Epic’s supported path is the GitHub source build. There is no official public Linux binary download page for the editor.

How egs_client detects your engines
- The HTTP API GET /list-unreal-engines scans the configured engines directory for folders that look like engine roots and tries to read:
  • Engine/Build/Build.version for the version string
  • Engine/Binaries/Linux/UnrealEditor for the editor binary path
- If you organize engines as described above, they will appear in the API and the Flutter UI.

Quick checklist for Linux
- [ ] Link Epic ↔ GitHub and accept EULA
- [ ] Clone EpicGames/UnrealEngine (pick desired branch: 5.3, 5.4, etc.)
- [ ] Run Setup.sh, GenerateProjectFiles.sh, Build
- [ ] Launch ./Engine/Binaries/Linux/UnrealEditor
- [ ] Optionally: move the built engine under $HOME/UnrealEngines and/or set EGS_UNREAL_ENGINES_DIR

References
- Official repo (access after linking): https://github.com/EpicGames/UnrealEngine
- Community wiki (historical but useful): https://github.com/UE4Linux/UE4Linux (unofficial)
- Heroic Games Launcher (for Windows launcher under Wine/Proton; unofficial): https://heroicgameslauncher.com/

Notes
- Commands and version numbers above are examples; always consult the branch’s README in Epic’s repo for the most current steps and dependencies.

Automate via egs_client API (experimental)
- You can ask the local server to clone and set up Unreal Engine for you (it will use your system Git credentials/token/SSH):
  curl -X POST http://127.0.0.1:8080/clone-unreal-engine \
       -H 'Content-Type: application/json' \
       -d '{
             "accept_eula": true,
             "branch": "5.4",
             "shallow": true,
             "build": false
           }'
- Optional fields: repo_url, engines_dir, dest_name, job_id. Progress can be observed via the WebSocket at /ws?job_id=YOUR_ID used in the request.
- This endpoint runs: git lfs install, git clone, ./Setup.sh, ./GenerateProjectFiles.sh, and optionally make. It assumes you have already linked Epic↔GitHub and have repository access.
