# Instructions for agents

Communicate concisely in plain language. Preserve unrelated changes and the
project's design intent.

## About this project

- FileTree is an IDE-style file manager sidebar for Omarchy. This fork keeps it
  deliberately small: one docked blade on a configurable side (left or right),
  holding only the built-in Files module with its properties pane.
- Do not reintroduce removed scope without the owner asking for it: extension or
  companion modules, agent blades, a second blade, window or undocked mode, blade
  animations, the native app, or drag-out modes other than the system drag.
- Support keyboard and mouse equally; the drop wheel favors mouse interaction.
  Keep navigation consistent, responsive, and usable without extra setup.
- Expose workflows through the `fileblade` CLI as well as the UI.

## Versions

- Keep `manifest.json`, `Cargo.toml`, and `Cargo.lock` versions aligned.
- The plugin id stays `data-goblin.fileblade` so existing layouts, state and
  bindings keep working.

## Testing

- Follow [CONTRIBUTING.md](CONTRIBUTING.md#tests). `tests/run` is the complete
  local code and bundle gate; there is no hosted CI.
- No unit tests. Cover behaviour through regression, integration and end-to-end
  tests, the UI expectations and the VM scenarios. Do not add mocked,
  implementation-detail unit suites.
- Any change under `src/`, `crates/`, `Cargo.toml` or `Cargo.lock` needs
  `tools/bundle build` and the rebuilt `fileblade-bin*` in the same commit; the
  plugin runs the committed binary.
- Validate Rust and QML before live UI tests.
- Update [UI expectations](tests/EXPECTATIONS.md) for user-visible changes,
  describing behavior from the user's point of view.
- Report skipped or pending checks honestly.

## Documentation

- Begin agent-written Markdown with `This file was written by an agent.` from
  the point where you wrote (not at the top of the doc).
- `README.md` is maintained by the owner. Edit it only when asked.
- Keep task-related technical documentation current and register new public
  technical documents in the [documentation catalogue](docs/agent-written/README.md).

## Cleaning up

- No comments in code; explain in the docs or the commit message instead.
- Keep Cargo targets and bundle staging on disk, not a memory-backed `/tmp`.
  See the build settings in [CONTRIBUTING.md](CONTRIBUTING.md#tests).
- Remove your temporary files, screenshots, staging directories, and unused
  worktrees when finished. Check ownership and active use before deletion;
  never remove another session's work or a user's captures.
- Kill stale processes when you finish.
- Leave only task deliverables untracked. Do not commit scratch files or agent plans.

## PR / Issues

- Ensure the owner verifies PR and issue body content.
- Ideally include an image, diagram, or gif to show rather than tell.
