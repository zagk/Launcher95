# Launcher95

A portable, single-executable application launcher with a Windows 95-style interface.
Drop it in any folder, run it, and keep your favourite programs one click (or one
global shortcut) away.

## Features

- **Authentic Windows 95 look**: navy title bar, beveled buttons, Tahoma UI font,
  classic menu bar, a flat white icon panel and a status bar with a resize grip,
  drawn entirely in the app.
- **Real Windows icons**: icons come from the actual shell resources.
  - A dropped `.lnk` shortcut is resolved to its target, so the tile shows the
    icon of the program, not a generic shortcut glyph.
  - Icon locations (`IconFile=`/`IconIndex=`) are honoured, and `%SystemRoot%`
    style variables are expanded.
  - `.url` shortcuts read `IconFile` / `IconIndex` from the INI file.
  - Executables are read with `PrivateExtractIconsW` so every size stays sharp;
    Store apps and documents fall back to the shell image lists
    (`SHGetImageList`, Jumbo/Extra large).
  - Results are cached in `Data/icons` and reused on later starts.
- **Add programs** with File > Add programs, `Ctrl+O`, or by dragging EXE / LNK /
  URL files onto the window.
- **Search** by name or path in a dedicated Find window (`Ctrl+F` or File > Find
  program), with live match counts; press Enter to launch the first match.
- **Groups**: drag one tile onto another to put both into a named group. You can also
  use Edit > New group…, or right-click a tile and choose Add to group > New group…
  to pick any set of programs at once. Filter the grid by group from View > Group;
  the status bar shows the active group.
- **Rename / remove** entries, or reveal them in Explorer. Removing never deletes
  the original file.
- **Missing-file cleanup** (Edit > Remove missing files) to prune shortcuts whose
  target disappeared.
- **Global hotkey** (default `Alt+Space`) that restores/focuses the launcher from
  anywhere, and minimizes it when it is already in front. The hotkey runs on a
  dedicated thread, so it works while the window is minimized.
- **Click-outside to minimize**: clicking another window sends the launcher to the
  taskbar automatically (configurable in Settings).
- **Single instance**: launching `launcher95.exe` again restores and focuses the
  running copy instead of opening a second window.
- **Opens at the mouse pointer**: on startup the window is centered on the cursor
  and clamped to the monitor's work area, so the launcher appears where you are
  working (configurable in Settings; falls back to the remembered position when
  disabled).
- **Settings**: icon size (32/40/64/96), name visibility, name sorting,
  always-on-top, minimize-on-blur, open-at-mouse-pointer, remembered window
  position, and a configurable hotkey.
- **Portable data**: everything lives in a `Data` folder next to the executable.
  If that folder is not writable (e.g. Program Files), it falls back to
  `%LOCALAPPDATA%\Launcher95`.

## Keyboard and mouse

| Action | Input |
| --- | --- |
| Show / hide launcher | `Alt+Space` (configurable) |
| Find a program | `Ctrl+F`, or File > Find program |
| Add programs | `Ctrl+O`, or File > Add programs |
| Launch first search match | `Enter` in the Find window |
| Clear search / close dialog | `Esc` |
| Launch an app | Click a tile |
| Tile context menu | Right-click a tile |
| Put two apps in a group | Drag one tile onto another |
| Create a group of several apps | Edit > New group…, or tile menu > Add to group |
| Filter by group | View > Group |
| Open at the mouse pointer | Enabled by default; toggle in Settings |
| Move window | Drag the title bar |
| Maximize / restore | Double-click the title bar, or the middle title button |
| Minimize | Click outside the window, or the left title button |

## Build on Windows

1. Install Rust from <https://rustup.rs/> (MSVC toolchain).
2. Open PowerShell in this folder.
3. Build:

   ```powershell
   cargo build --release
   ```

4. Run the tests:

   ```powershell
   cargo test --release
   ```

5. The executable is `target\release\launcher95.exe`. Copy it anywhere; on first
   run it creates a `Data` folder beside itself.

The icon and version resources are compiled in by `build.rs` from
`assets\launcher95.ico`; `tools\make_icon.ps1` regenerates that icon.

## Data files

- `Data/apps.json` – registered entries (`id`, `name`, `path`, `group`, `source`)
- `Data/settings.json` – window geometry, icon size, hotkey and toggles
- `Data/icons/` – extracted icon cache (safe to delete; it is rebuilt)

Both JSON files are plain, human-editable UTF-8. A UTF-8 BOM is tolerated on load.

## Notes and limitations

- Global hotkey registration depends on Windows accepting the key combination.
  If another application already owns it, registration fails and the status bar
  reports the error — pick a different combination in Settings.
- Shortcuts whose target no longer exists fall back to the shell's shortcut icon;
  use Edit > Remove missing files to clean them up.
- The UI text is English; Korean shortcut names render correctly through a
  Malgun Gothic fallback font.

## License

MIT
