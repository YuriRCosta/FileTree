<p align="center"><img src="assets/fileblade-logo.svg" alt="FileBlade" width="640"></p>

---

**FileBlade** is an IDE-style file manager sidebar for Omarchy:
- One docked blade on the left or right edge of the screen, opened and closed from a shortcut
- A file tree with Git status, fuzzy search, quick navigation, favorites and folder colors
- A properties pane under the tree with metadata and previews for the selection
- Drag files straight into other applications, or hold space for a quick-action wheel
- Keyboard- or mouse-first, and a `fileblade` CLI that scripts and coding agents can drive

<p align="center"><img src="assets/fileblade-overview.gif" alt="FileBlade highlights" width="720"></p>

> [!NOTE]
> This is a simplified fork of [data-goblin/fileblade](https://github.com/data-goblin/fileblade)
> by Kurt Buhler. It keeps only the file manager: no extension modules, no agent blades, no
> native app, no second blade and no window mode.

## Installation

FileBlade ships a bundled static x86-64 Linux backend, so no Rust toolchain is
needed. It requires Omarchy 4.0.2 or later.

```bash
OMARCHY_SHELL_IPC_TIMEOUT=10s omarchy plugin add https://github.com/YuriRCosta/fileblade.git --enable
omarchy restart shell
```

Then add the contents of [`examples/fileblade-bindings.lua`](examples/fileblade-bindings.lua)
to `~/.config/hypr/bindings.lua` and run `hyprctl reload`. `Super+B` opens and
closes the blade.

To remove it:

```bash
omarchy plugin remove data-goblin.fileblade
omarchy restart shell
```

Your layout, settings and history stay in `~/.config/omarchy/fileblade/` and
`~/.local/state/omarchy/fileblade/`. Remove the bindings you added if you no
longer want them.

## Using it

- **Side:** open Settings (the gear in the blade footer) and pick Left or Right,
  or run `fileblade blade side left|right`. There is only ever one blade.
- **Width:** drag the blade's inner edge, hold `Super` and drag with the right
  mouse button, or use `Super+Minus` / `Super+Equals` while the blade has focus.
- **Properties pane:** drag the divider under the tree to resize it; turn it
  off with "Properties panel" in the Files settings. `Tab` moves focus between
  the tree and the pane.
- **Dragging out:** a plain left-button drag hands the files to the application
  you drop them on. Hold Shift or Ctrl to paste the absolute or relative path
  instead, or drag with the right button to get the drop wheel.
- **Search and navigation:** `/` searches, `Super+Z` opens quick navigation,
  and `?` in the blade lists every shortcut.

## Features

- File tree with Git status markers and a repository summary
- Search syntax: fuzzy, `'substring`, `^anchors$`, `"exact"`, `-exclude`,
  `type:`, `format:`, `in:`, `content:` and regex
- `zoxide`-like quick navigation and recent files
- Copy, cut, paste, rename, create, trash and undo/redo for every operation
- Folder and file colors, favorites, drives and remote locations
- Media view for folders of images and videos
- Script actions in the right-click menu
- A drive usage bar under the toolbar
- The `fileblade` CLI: selection, navigation, file operations and blade control,
  with JSON output for scripts and agents

<details>
<summary><b>File tree</b></summary>
<p align="center"><img src="assets/readme/git-status.png" alt="The file tree with git status markers and the repository summary tooltip" width="640"></p>
</details>

<details>
<summary><b>Right-click actions and folder colors</b></summary>
<p align="center"><img src="assets/readme/folder-colors.png" alt="The row context menu with file actions and the folder color swatches" width="440"></p>
</details>

<details>
<summary><b>Search</b></summary>
<p align="center"><img src="assets/readme/deep-search.png" alt="A whole-root fzf search narrowed with type: and format: filters" width="300"></p>
</details>

<details>
<summary><b>Quick navigation</b></summary>
<p align="center"><img src="assets/readme/quick-nav.png" alt="The Super+Z quick nav list of recently visited folders" width="300"></p>
</details>

<details>
<summary><b>Drop wheel</b></summary>
<p align="center"><img src="assets/fileblade-drop-wheel.gif" alt="Dragging a file from FileBlade onto a terminal and picking a new pane from the selection wheel" width="720"></p>
</details>

## What it changes on your system

- **Keybindings:** FileBlade never writes your Hyprland config. The example
  bindings ask FileBlade first with a short timeout and fall back to the normal
  dispatcher, so nothing breaks when the shell is down. `Super+W`, `Super+Arrows`
  and `Super+Shift+Arrows` act on the blade when it has focus.
- **Screen space:** the open blade reserves its strip of the screen, so tiled
  windows move aside. While it has keyboard focus, other windows' active border
  is dimmed.
- **Trash:** trashed files go to `~/.local/share/Trash`, shared with other file
  managers. Automatic cleanup is off until you choose a retention period.
- **Audit log:** every file operation is recorded in
  `~/.local/state/omarchy/fileblade/audit.jsonl`.

## More

- [ARCHITECTURE.md](ARCHITECTURE.md): how the QML service and Rust backend fit together
- [SECURITY.md](SECURITY.md): trust boundaries and known limits
- [CONTRIBUTING.md](CONTRIBUTING.md): layout, tests and conventions
- [docs/agent-written](docs/agent-written/README.md): keybindings, drag and drop, Git status and search ranking

## License

[MIT](LICENSE). Original work by Kurt Buhler.
