This file was written by an agent.

# Changelog

## Unreleased

- The tree header no longer lists volumes, which the Drives view already lists. Turn "Volumes in the tree" back on in Files settings to restore them, which also restores the one-click route to a connected tailnet peer that lived in that panel.
- Choose which file toolbar buttons appear, in a Toolbar group in Files settings. Every hidden button except Drives keeps a keyboard route.
- Row density returns to five stops named XS to XL. A saved percentage that is not a stop resolves to the nearest one.
- Delete acts on Skills, Memory, Hooks and MCP rows, or says why it cannot. It previously did nothing at all: the consent refusal never reached a module, and Trash was offered for definitions that have no file of their own.
- A hook modification is refused when the resulting configuration file would not parse, or would lose a top-level key it did not mean to remove. The original is left byte-identical.

- Image gallery primitives for modules: `ImageGrid` with month sections and a cursor, `ThumbnailCache` over the backend thumbnail request, a right-hand `TimelineScrubber` with years, month dots and a scrub pill, and a five-step `ImageSizeControl`.
- Module definitions accept an `icon` image; the picker, the settings sheet and `PaneHeader` draw it tinted through `ModuleIcon`.
- `BladePopout` hosts any module under a bar icon through a `BladeContext` popout seam.
- The plugin catalog treats a companion that also declares `bar-widget` as enabled when `shell.json` lists it, not only when a bar entry names it.

## 0.1.0-beta.1 (unreleased)

- Install the Welcome extensions at reviewed commits and refuse installation if a pin is unavailable.
- Keep recent trash-test fixtures relative to the test run so retention checks do not fail as dates pass.
- Synchronize trash confirmations across monitors and bind answers to their requests.
- Super+B and Super+Shift+B close open blades with one press.
- Choose Active, mirrored All, or a named monitor lock in Settings.
- Clamp blade widths to each monitor without changing saved preferences.
- Cancel close-tab confirmations when their slot or tabs change.
- Keep delayed navigation focus on its original monitor.
- Cancel pending trash requests when their confirming pane hides or unloads.
- Cancel menus, wheels, drags, and focus when their monitor disconnects.
- Resolve hover, drops, and explicit window focus across visible monitors.
- Keep directional focus routing on each blade's assigned monitor.
- Open undocked blades on their monitor, preserving ordinary window movement.
- Recognize Foot server windows as terminal targets.
- Prevent terminal actions from reaching another shared window.
