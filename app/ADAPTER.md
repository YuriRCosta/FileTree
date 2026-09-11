This file was written by an agent.

# Bounded native adapter spike

This directory is an unfinished development spike. M1 is blocked: the first
native shutdown left Quickshell alive and unresponsive, preventing relaunch.
It is not an installable runtime or a qualified native lifecycle.

`shell.qml` loads the existing `Service.qml` through an absolute file URL.
Existing QML imports and browser sources are unchanged. The shell facade reads
the real shell configuration for bar placement; the private kit reads the
current Omarchy theme. The facade cannot observe runtime bar autohide.

`launch` requires an explicit isolated `FILEBLADE_SPIKE_HOME` and the locally
built release binary. Its XDG environment isolates development state from old
plugin writers. That environment also reaches child applications, so it is
not the production state-isolation design. It establishes no write lease.

## Source accounting

The kit derives from Omarchy v4.0.2, commit
`346e69e1cec6c4e8924531874af6ba010a1bc99e`.
`upstream.json` records the original source paths and SHA-256 values of all
15 copied files. Each matched the installed Omarchy 4.0.2-1 package before
adaptation. `licenses/omarchy-MIT.txt` retains the upstream copyright and
permission notice. No upstream UI path is symlinked into this directory.

The inventory found five direct FileBlade UI dependencies: BorderOverlay,
BorderSurface, Button, PanelToolTip and TextField. Goblins additionally imports
BarWidget, BarIconButton and KeyboardPanel; the latter components bring
WidgetButton and OpticalGlyph into the closure. Commons supplies Color, Style,
Util, Border and BorderGeometry. Supported companion QML also consumes this
theme surface. A runtime test of every companion importer remains pending.

Changes from upstream:

- Explanatory code comments are removed; the license is retained separately.
- Util retains only clamp, clampAlpha, alpha and fileUrl.
- Color omits notification, polkit, lock and image-picker surface objects.
- Theme colors and shell values use file-change reloads, because the native
  instance does not receive Omarchy shell theme IPC.
- BarWidget and KeyboardPanel accept a dynamic bar facade, matching their
  existing accesses to host-provided members.
- Module registration files expose only this dependency closure.

## Evidence limits

Harness A, SSH 2422: Qt base 6.11.2-2, Qt declarative 6.11.2-1,
Quickshell 0.3.1-1, Omarchy 4.0.2-1, Hyprland 0.56.2-1, software rendering.
Cold native loading, fixture listing/selection and live Tokyo Night to
Catppuccin repaint were observed with the FileBlade plugin unavailable.
The ordinary-window request never completed after the shutdown failure.

Matched Catppuccin plugin/native blade captures differed at three of 400,520
pixels, each by one RGB channel level. Exact pixel equality was not obtained;
the origin of those differences has not been established. The strict visual
gate is therefore not declared passed.

The recorded live shell root precedes a later static-only change of its bar
property from QtObject to var. The same static-only adjustment was made to
BarWidget and KeyboardPanel. Those final adjustments have not been rerun in
the VM. Final qmllint exited zero with three KeyboardPanel warnings inherited
from its upstream PanelWindow/contentItem declarations. Shell syntax passed.

Detailed source identities, screenshots, commands, shutdown diagnostics and
contract requests are in Sootscale's ignored aim note and evidence directory.
Docking transitions, hotplug, complete focus parity, single-writer authority,
accepted-operation survival, retained expectation suites, chooser and ARM
execution remain unproven.
