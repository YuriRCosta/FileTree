import QtQuick
import QtTest
import "../../lib/FileIcons.js" as FileIcons

TestCase {
  name: "FileIconsSecurity"

  function test_theme_names_are_preserved() {
    compare(FileIcons.safeThemeIconName("org.gnome.Nautilus"), "org.gnome.Nautilus")
    compare(FileIcons.safeThemeIconName("nvim-nightly_2"), "nvim-nightly_2")
  }

  function test_paths_and_urls_are_rejected() {
    compare(FileIcons.safeThemeIconName("/tmp/untrusted.png"), "")
    compare(FileIcons.safeThemeIconName("file:///tmp/untrusted.png"), "")
    compare(FileIcons.safeThemeIconName("image://provider/untrusted"), "")
    compare(FileIcons.safeThemeIconName("../untrusted"), "")
    compare(FileIcons.safeThemeIconName("folder/untrusted"), "")
  }

  function test_names_are_bounded() {
    compare(FileIcons.safeThemeIconName("a".repeat(256)), "a".repeat(256))
    compare(FileIcons.safeThemeIconName("a".repeat(257)), "")
  }

  function test_symlink_icon_wins_over_file_and_directory_type() {
    compare(FileIcons.fileIcon("SKILL.md", true), "")
    compare(FileIcons.entryIcon("SKILL.md", false, true, false, false), "")
    compare(FileIcons.entryIcon("skill", true, true, false, false), "")
    compare(FileIcons.entryIcon("skill", true, false, false, false), FileIcons.folderIcon(false))
  }
}
