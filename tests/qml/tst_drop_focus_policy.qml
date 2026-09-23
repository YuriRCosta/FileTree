import QtQuick
import QtTest
import "../../lib/DropFocusPolicy.js" as DropFocusPolicy

TestCase {
  name: "DropFocusPolicy"

  function test_folder_only_open_stays_in_filetree() {
    verify(!DropFocusPolicy.transfersFocus("open", 0))
  }

  function test_file_or_mixed_open_yields_to_the_launched_app() {
    verify(DropFocusPolicy.transfersFocus("open", 1))
    verify(DropFocusPolicy.transfersFocus("open", 3))
  }

  function test_external_and_target_actions_data() {
    return [
      { tag: "application", action: "application" },
      { tag: "app", action: "app-open" },
      { tag: "nvim", action: "nvim-open" },
      { tag: "review", action: "review" },
      { tag: "terminal", action: "terminal" },
      { tag: "mux-open", action: "mux-open" }
    ]
  }

  function test_external_and_target_actions(data) {
    verify(DropFocusPolicy.transfersFocus(data.action, 0))
  }

  function test_non_launching_actions_keep_filetree_focus() {
    verify(!DropFocusPolicy.transfersFocus("copy-paths", 2))
    verify(!DropFocusPolicy.transfersFocus("open-with", 2))
    verify(!DropFocusPolicy.transfersFocus("", 2))
  }

  function test_future_backend_actions_yield_by_default() {
    verify(DropFocusPolicy.transfersFocus("future-action", 0))
  }
}
