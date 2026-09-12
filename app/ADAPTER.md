This file was written by an agent.

# Bounded native adapter spike

This directory is a bounded development spike, not an installable runtime.
The M1 visual gate passes under the coordinator's declared pixel tolerance.
The shutdown failure reproduces in an empty Quickshell with `-d`; foreground
launching exits cleanly with FileBlade's original backend cleanup. The
launcher rejects `-d` and `--daemonize` on the qualified spike tuple. A process
supervisor must launch Quickshell in the foreground.

`shell.qml` loads the existing `Service.qml` through an absolute file URL.
Existing QML imports and browser sources are unchanged. The shell facade reads
the real shell configuration for bar placement; the private kit reads the
current Omarchy theme. The facade cannot observe runtime bar autohide.

`launch` requires an explicit isolated `FILEBLADE_SPIKE_HOME` and the locally
built release binary. Its XDG environment isolates development state from old
plugin writers. That environment also reaches child applications, so it is
not the production state-isolation design. The launcher starts or attaches
to the native authority before loading QML. Its kernel OFD lease covers the
canonical selected state, config and recovery roots; view processes relay to
its private socket. A replaced identity permanently invalidates the authority.
Durable records open relative to pinned directory descriptors; accepted work
stops at its next barrier with an explicit authority-lost result. The isolated
launcher enables Full only after verifying the three fixture roots. Standard
native startup stays ReadOnly until the migration outcome is available.
Accepted mutations survive view shutdown and retain results in the authority
until fetched or 24 hours. Process-death recovery still uses durable journals;
the in-memory result cache does not survive authority process death.

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
Docking, conversion to an ordinary window, and docking again work through
the native authority. The Never preference was saved through the QML client
and verified on disk.

Matched Catppuccin plugin/native blade captures differed at three of 400,520
pixels, each by one RGB channel level. R12 declares a pass at no more than one
level per channel and fewer than 0.1 percent differing pixels. This passes.

The final bar property typing changes in shell.qml, BarWidget and
KeyboardPanel were rerun in the VM. Guest qmllint exited zero with three
KeyboardPanel warnings inherited from its upstream PanelWindow/contentItem
declarations and one BackendClient QProcess::ExitStatus metadata warning.

Detailed source identities, screenshots, commands, shutdown diagnostics and
contract requests are in Sootscale's ignored aim note and evidence directory.
Sixteen native authority tests pass in harness A, including the 256 MiB copy
after EOF, a multi-file move with a lost progress subscriber, explicit cancel,
result fetching, lock identity, and the Qt pipe-availability regression.
The retained server and persistence suites pass another seventeen tests.
The live native authority expectation also passes: a two-file move continues
after the QML view quits, preserves both mappings and the same authority,
and remains queryable after view relaunch until explicitly fetched.
A real QML copy interrupted by root replacement leaves the replacement
sentinel unchanged and retains an explicit authority-lost result after view
exit.
Hotplug, complete focus parity, remaining retained expectations, chooser and
ARM execution remain separate qualification work.

`ovm-spike` adapts the retained expectation scripts to harness A and the native
IPC target without changing their source. Other shell targets still address
the real Omarchy bar. Guest commands use the native XDG roots and binary;
hardcoded Trash fixture paths are mapped to that isolated data root. Restart
requests restart the foreground native view and the real shell while keeping
the authority alive. Push, install and reset require explicit staging outside
this adapter.

Retained Trash expectation E-14-11 remains blocked on R19. The private
data root receives the trashed file, but the desktop GVfs trash service uses
the desktop data root and cannot resolve that item through `trash:///`.
The VM fixture lives under `/home/omarchy/fileblade-runtime-state` to avoid
the separate cross-filesystem limitation of placing its Trash on `/tmp`.
Removing the data override also requires isolating FileBlade's artifact bin
from legacy writers; that path belongs to the operations lane. The bounded
launcher retains its isolation pending that shared contract change.
