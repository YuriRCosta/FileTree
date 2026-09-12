# Configure the drop wheel

This file was written by an agent.

FileBlade reads the optional `dropWheel` member of its own `settings.json`
each time it opens the wheel and each time it dispatches a custom command.
The containing settings document keeps its existing version. The wheel member
has its own `version: 1`. An absent member uses the standard wheel.

The current plugin profile stores this document at
`$XDG_CONFIG_HOME/omarchy/fileblade/settings.json` (normally
`~/.config/omarchy/fileblade/settings.json`). The native profile uses the
preferences path selected by the native runtime. Edit the existing document,
preserving its other members. Ordinary preference saves preserve unknown
members, including unknown fields inside `dropWheel`.

The core settings form entry is an integration dependency. This schema is
usable by editing the settings document now; it does not claim a completed
visual wheel editor.

## Order, visibility and appearance

`actions` is an ordered array of overrides. Listed available actions appear
first, followed by unlisted available actions in their original order.
Custom actions can be included in this order by their `custom:` ids. Their
definitions belong in `customActions`. Unlisted custom actions follow the
standard actions. `hidden: true` removes an action. To restore it, remove
that override or set `hidden: false`.

Built-in ids are `open`, `open-with`, `terminal`, `review`, `mux-open`,
`nvim-open`, and `app-open`. Availability still depends on the target:
configuration cannot make an unavailable multiplexer or editor work.
`application` and `copy-paths` are also reserved dispatch ids.

An override can set `label`, `key`, `glyph`, `icon`, `description`, `group`,
and `placements`. `key` is one ASCII letter or digit, or empty for automatic
assignment. Keys are unique within each ring; a duplicate receives an
available key. Groups are descriptive metadata; array order determines the
wheel order.

`icon` is a theme icon name such as `utilities-terminal`, or a bundled mark
(`herdr`, `tmux`, `pane-horizontal`, `pane-vertical`). The name may contain
ASCII letters, digits, hyphens, underscores and dots. Paths and URLs are
not accepted. An explicit icon clears the inherited icon source and mask,
so it wins over an application-resolved icon. An explicit `glyph` without
`icon` clears the inherited icon and uses the glyph instead. The renderer
bounds the icon size. Existing application icon fields and bundled marks
remain usable without configuration.

`placements` works the same way for each action's children: listed children
come first, unlisted children remain, and `hidden` removes a child. Hiding
all children removes the containing action. Multiplexer placement ids are:

| Target | Placement ids |
| --- | --- |
| herdr | `right`, `down`, `tab`, `workspace` |
| tmux | `right`, `down`, `window`, `session` |
| nvim | `tab`, `buffer`, `split` |
| hunk review over a multiplexer | `right`, `down`, `tab`, `workspace`, `window` |

Open-with application placements share the internal id `application`.
Identify an override for one application by `desktop_id`, for example
`{"desktop_id":"org.gnome.Loupe.desktop","hidden":true}`.

## Custom actions and two placement layers

A custom action has a unique `id` starting with `custom:`, a nonempty
`label`, optional display fields listed above, and either a `command`,
a `builtin` reference, or executable descendants in `placements`.
Child ids are unique within their parent. A child added to a built-in's
placements must also start with `custom:`. Descendants defined inside a
custom action may use short ids such as `numbered`.

There are at most three rings: action, placement, sub-placement. Each level
can have its own icon, key, label and command. An entry with children opens
its next ring; its own command is not executed until it is a leaf. There is
no implicit command inheritance: each executable child declares its own
command or built-in reference.

To reuse an available built-in as a custom leaf, use:

```json
{
  "id": "custom:split",
  "label": "Split here",
  "builtin": {"action": "mux-open", "placement": "right"}
}
```

A reference inherits the built-in glyph and icon unless explicitly overridden.
It must identify an executable built-in leaf. Omit `placement` for
an action without children, such as `terminal`. For an Open-with reference,
use `action: "open-with"` and the desktop id as `placement`.
Built-in implementations cannot be replaced by adding `command` to an
override. Add a separate custom action instead.

`targetKinds` optionally restricts a custom action to a nonempty array drawn
from `desktop`, `terminal`, `editor`, `window`, `blade`. Applications and
browsers are normalized to `window`. The `blade` value applies when the
runtime supplies a blade target; the legacy desktop drop resolver currently
excludes its own blade windows. Omission allows every target, subject to
run-mode capability checks.

`conditions` optionally contains `mime` and `path` arrays of patterns. Within
an array, any pattern may match; across both arrays, every condition must
match every selected path. `*` matches any sequence; other characters are
literal. Examples: `"mime":["image/*"]`, `"path":["/home/me/project/*.rs"]`.
Path matching is case-sensitive against the full path representation. The
existing resolver probes at most 12 MIME values. An unprobed or unknown MIME
never satisfies a MIME condition.

## Commands

`command` is an argument array with a literal executable first. FileBlade
does not interpret it as a shell command. For example:

```json
["my-inspector", "--", "{paths}"]
```

Substitutions occupy whole arguments:

| Argument | Expansion |
| --- | --- |
| `{paths}` | Every selected path as a separate argument |
| `{path}` | The sole selected path; multiple selections are refused |
| `{cwd}` | The selected folder, or the first selected file's parent |
| `{git_root}` | The common detected repository root; absence is refused |

Path bytes are retained through native argument construction, including
FileBlade's encoded representation for non-UTF-8 paths. Quotes, spaces,
`$()` and shell operators in substituted paths stay data. An embedded
substitution such as `--file={path}` is rejected; use two arguments.
Unrecognized brace expressions are rejected. Include `--` where the chosen
program needs an end-of-options delimiter; FileBlade does not infer the
option syntax of arbitrary programs.

`runMode` defaults to `detached`:

| Mode | Behavior |
| --- | --- |
| `detached` | Launch the argv with `{cwd}` as working directory |
| `terminal` | Use the desktop's terminal launcher with that directory and argv |
| `multiplexer` | Use the resolved herdr/tmux target, with required `placement` from its table above |

Multiplexer transport uses the existing argument-quoting helpers. Its
transport may require a quoted command string internally; the configuration
itself remains argv. Ambiguous or stale targets are refused. A custom command
is resolved from current settings and current file facts again on dispatch.
Removing or hiding it invalidates an old wheel's route.

## Worked example

Merge this member into the existing settings document. It puts Inspect first,
renames the terminal action, removes Open with, and supplies three executable
leaves across two placement layers:

```json
{
  "dropWheel": {
    "version": 1,
    "actions": [
      {"id": "custom:inspect"},
      {"id": "terminal", "label": "Shell", "key": "s", "icon": "utilities-terminal"},
      {"id": "open-with", "hidden": true}
    ],
    "customActions": [
      {
        "id": "custom:inspect",
        "label": "Inspect",
        "key": "i",
        "icon": "document-properties",
        "targetKinds": ["desktop", "terminal", "editor", "window"],
        "conditions": {"mime": ["text/*"]},
        "placements": [
          {
            "id": "text",
            "label": "Text",
            "key": "t",
            "placements": [
              {"id": "plain", "label": "Read", "key": "r", "command": ["less", "--", "{paths}"], "runMode": "terminal"},
              {"id": "numbered", "label": "Line numbers", "key": "n", "command": ["less", "-N", "--", "{paths}"], "runMode": "terminal"}
            ]
          },
          {"id": "custom:shell", "label": "Shell here", "key": "s", "builtin": {"action": "terminal"}}
        ]
      }
    ]
  }
}
```

Letters choose the displayed action, then placement, then sub-placement.
Arrows and Tab move within the active ring; Enter accepts. Escape or Backspace
returns one layer, then closes the wheel. Pointer hover exposes children;
clicking an executable wedge dispatches it.

## Limits and diagnostics

The existing preferences document limit is 64 KiB. Each ring displays at most
12 entries, custom definitions consume a budget of 96 nodes, and a fourth
ring is rejected. Commands have at most 128 arguments, each at most 4096
bytes. Display text is bounded to 512 bytes and cannot contain control
characters. Identifiers are at most 96 bytes.

A malformed custom entry is skipped; a malformed override is ignored and its
built-in remains available. The wheel footer shows the diagnosis. Full
messages are returned in the drop-context response's `diagnostics` array.
An unsupported wheel schema version uses defaults and reports the problem.
No wheel read rewrites the settings document or drops unknown fields.

A drag released while the wheel is loading is remembered for up to 800 ms.
Rows arriving within that interval activate at the remembered point once.
After the interval, the wheel stays open for an explicit choice. Closing the
wheel discards the pending activation.
