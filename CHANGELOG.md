This file was written by an agent.

# Changelog

## Unreleased

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
