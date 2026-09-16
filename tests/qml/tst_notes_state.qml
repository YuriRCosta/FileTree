import QtQuick
import QtTest
import "../../modules/notes/NotesState.js" as NotesState

TestCase {
  name: "NotesStateRegression"

  function repeated(unit, count) {
    var out = ""
    for (var i = 0; i < count; i++) out += unit
    return out
  }

  function local(text, revision, dirty, saving) {
    return { text: text, revision: revision, dirty: !!dirty, saving: !!saving }
  }

  function test_utf8_length() {
    compare(NotesState.utf8Length(""), 0)
    compare(NotesState.utf8Length("abc"), 3)
    compare(NotesState.utf8Length("é"), 2)
    compare(NotesState.utf8Length("€"), 3)
    compare(NotesState.utf8Length("😀"), 4)
    compare(NotesState.utf8Length(null), 0)
    compare(NotesState.utf8Length(undefined), 0)
  }

  function test_cap_is_bounded_for_host_layout() {
    compare(NotesState.CAP_BYTES, 65536)
    verify(NotesState.CAP_BYTES < 262144)
  }

  function test_clamp_ascii() {
    var result = NotesState.clampToBytes(repeated("a", 70000), NotesState.CAP_BYTES)
    compare(result.clamped, true)
    compare(result.bytes, NotesState.CAP_BYTES)
    compare(result.text.length, NotesState.CAP_BYTES)
  }

  function test_clamp_under_cap_is_untouched() {
    var result = NotesState.clampToBytes("hello", NotesState.CAP_BYTES)
    compare(result.clamped, false)
    compare(result.text, "hello")
    compare(result.bytes, 5)
  }

  function test_clamp_never_splits_a_surrogate_pair() {
    var text = repeated("a", NotesState.CAP_BYTES - 2) + "😀"
    var result = NotesState.clampToBytes(text, NotesState.CAP_BYTES)
    compare(result.clamped, true)
    compare(result.bytes, NotesState.CAP_BYTES - 2)
    compare(result.text.charCodeAt(result.text.length - 1), 97)
    verify(NotesState.utf8Length(result.text) <= NotesState.CAP_BYTES)
  }

  function test_clamp_multibyte_boundary() {
    var text = repeated("é", 40000)
    var result = NotesState.clampToBytes(text, NotesState.CAP_BYTES)
    compare(result.clamped, true)
    verify(result.bytes <= NotesState.CAP_BYTES)
    compare(result.bytes % 2, 0)
  }

  function test_normalize_legacy_string_state() {
    var state = NotesState.normalize("legacy note", undefined)
    compare(state.text, "legacy note")
    compare(state.revision, 0)
    compare(state.bytes, 11)
  }

  function test_normalize_rejects_junk() {
    compare(NotesState.normalize(undefined, undefined).text, "")
    compare(NotesState.normalize(42, "x").text, "")
    compare(NotesState.normalize({ text: "wrapped" }, 3).text, "wrapped")
    compare(NotesState.normalize({ text: "wrapped", revision: 8 }, 3).revision, 8)
    compare(NotesState.normalize("x", -5).revision, 0)
    compare(NotesState.normalize("x", 7.9).revision, 7)
  }

  function test_hydration_skips_while_dirty() {
    var decision = NotesState.hydration({ text: "old", revision: 9 }, local("typed", 2, true, false))
    compare(decision.action, "skip")
    compare(decision.reason, "local edit pending")
  }

  function test_hydration_skips_while_saving() {
    var decision = NotesState.hydration({ text: "old", revision: 9 }, local("typed", 2, false, true))
    compare(decision.action, "skip")
  }

  function test_hydration_skips_stale_revision() {
    var decision = NotesState.hydration({ text: "old", revision: 1 }, local("current", 4, false, false))
    compare(decision.action, "skip")
    compare(decision.reason, "stale revision")
  }

  function test_hydration_applies_newer_revision() {
    var decision = NotesState.hydration({ text: "fresh", revision: 5 }, local("current", 4, false, false))
    compare(decision.action, "apply")
  }

  function test_hydration_skips_identical_echo() {
    var decision = NotesState.hydration({ text: "same", revision: 4 }, local("same", 4, false, false))
    compare(decision.action, "skip")
    compare(decision.reason, "unchanged")
  }

  function test_persist_plan_bumps_revision() {
    var plan = NotesState.persistPlan("note", 7, false)
    compare(plan.action, "write")
    compare(plan.revision, 8)
    compare(plan.text, "note")
    compare(plan.clamped, false)
  }

  function test_persist_plan_clamps_oversize_input() {
    var plan = NotesState.persistPlan(repeated("a", 70000), 1, false)
    compare(plan.action, "write")
    compare(plan.clamped, true)
    compare(plan.bytes, NotesState.CAP_BYTES)
    verify(NotesState.utf8Length(plan.text) <= NotesState.CAP_BYTES)
  }

  function test_persist_plan_holds_when_migrating_oversize_note() {
    var plan = NotesState.persistPlan(repeated("a", 70000), 3, true)
    compare(plan.action, "hold")
    compare(plan.revision, 3)
  }

  function test_oversize_note_resumes_after_trim() {
    var plan = NotesState.persistPlan("trimmed", 3, true)
    compare(plan.action, "write")
    compare(plan.text, "trimmed")
    compare(plan.revision, 4)
  }

  function test_status_text_states() {
    compare(NotesState.statusText(12, false, false, false, false), "12 bytes")
    compare(NotesState.statusText(12, false, false, true, false), "saving")
    compare(NotesState.statusText(70000, false, true, false, false), "over 64 KiB, saving paused")
    compare(NotesState.statusText(65536, true, false, false, false), "limit reached, trimmed")
    compare(NotesState.statusText(62000, false, false, false, false), "61 KiB of 64 KiB")
    compare(NotesState.statusText(12, false, false, false, true), "save rejected by host")
  }

  function test_round_trip_is_stable() {
    var text = "line one\nline two"
    var plan = NotesState.persistPlan(text, 0, false)
    var incoming = NotesState.normalize(plan.text, plan.revision)
    compare(incoming.text, text)
    compare(NotesState.hydration(incoming, local(plan.text, plan.revision, false, false)).action, "skip")
  }

  function test_legacy_note_migrates_into_a_named_notebook() {
    var result = NotesState.normalizeNotebook({ text: "legacy note", revision: 4 }, 0, "Ideas")
    compare(result.migrated, true)
    compare(result.notebook.revision, 4)
    compare(result.notebook.items.length, 1)
    compare(result.notebook.items[0].label, "Ideas")
    compare(result.notebook.items[0].text, "legacy note")
  }

  function test_notebook_tabs_add_rename_select_and_remove() {
    var notebook = NotesState.emptyNotebook("First", "alpha", 2)
    notebook = NotesState.addNote(notebook)
    compare(notebook.items.length, 2)
    compare(NotesState.activeIndex(notebook), 1)
    notebook = NotesState.renameNote(notebook, 1, "Second")
    compare(notebook.items[1].label, "Second")
    notebook = NotesState.selectNote(notebook, 0)
    compare(NotesState.activeNote(notebook).text, "alpha")
    notebook = NotesState.removeNote(notebook, 0)
    compare(notebook.items.length, 1)
    compare(NotesState.activeNote(notebook).label, "Second")
  }

  function test_note_numbers_continue_after_deletion_and_reload() {
    var notebook = NotesState.addNote(NotesState.emptyNotebook("Note 1", "first", 0))
    notebook = NotesState.removeNote(notebook, 0)
    var saved = NotesState.persistedNotebook(notebook)
    notebook = NotesState.normalizeNotebook(JSON.parse(JSON.stringify(saved))).notebook
    notebook = NotesState.addNote(notebook)
    compare(notebook.items[0].label, "Note 2")
    compare(notebook.items[1].label, "Note 3")
    notebook = NotesState.removeNote(notebook, 1)
    notebook = NotesState.addNote(notebook)
    compare(notebook.items[1].label, "Note 4")
  }

  function test_notebook_text_cap_applies_across_all_notes() {
    var notebook = NotesState.emptyNotebook("First", repeated("a", 40000), 0)
    notebook = NotesState.addNote(notebook)
    var plan = NotesState.editPlan(notebook, repeated("b", 40000), false)
    compare(plan.action, "write")
    compare(plan.clamped, true)
    compare(plan.bytes, NotesState.CAP_BYTES)
    compare(NotesState.textBytes(plan.notebook), NotesState.CAP_BYTES)
  }

  function test_edit_plan_stamps_only_real_text_changes_and_round_trips() {
    var notebook = NotesState.emptyNotebook("First", "alpha", 0)
    compare(NotesState.activeNote(notebook).edited, undefined)
    notebook = NotesState.editPlan(notebook, "alpha", false, 1000).notebook
    compare(NotesState.activeNote(notebook).edited, undefined)
    notebook = NotesState.editPlan(notebook, "alpha beta", false, 2000).notebook
    compare(NotesState.activeNote(notebook).edited, 2000)
    notebook = NotesState.remember(notebook, 3, 3, 3)
    notebook = NotesState.renameNote(notebook, 0, "Renamed")
    compare(NotesState.activeNote(notebook).edited, 2000)
    var full = NotesState.emptyNotebook("Full", repeated("a", NotesState.CAP_BYTES), 0)
    full.items[0].edited = 3000
    compare(NotesState.activeNote(NotesState.editPlan(full, repeated("a", NotesState.CAP_BYTES) + "b", false, 4000).notebook).edited, 3000)
    var restored = NotesState.normalizeNotebook(JSON.parse(JSON.stringify(NotesState.persistedNotebook(notebook)))).notebook
    compare(NotesState.activeNote(restored).edited, 2000)
    restored.items[0].edited = "garbage"
    compare(NotesState.copyNotebook(restored).items[0].edited, undefined)
  }

  function test_word_and_character_counts() {
    compare(NotesState.wordCount(""), 0)
    compare(NotesState.wordCount("   \n\t "), 0)
    compare(NotesState.wordCount(" one  two\nthree\tfour "), 4)
    compare(NotesState.wordCount(null), 0)
    compare(NotesState.characterCount(""), 0)
    compare(NotesState.characterCount("abc"), 3)
    compare(NotesState.characterCount("a😀é"), 3)
    compare(NotesState.characterCount("\uD800x"), 2)
    compare(NotesState.characterCount(undefined), 0)
  }

  function test_notebook_round_trip_preserves_active_note() {
    var notebook = NotesState.addNote(NotesState.emptyNotebook("First", "alpha", 3))
    notebook = NotesState.renameNote(notebook, 1, "Second")
    var saved = NotesState.persistedNotebook(notebook)
    var result = NotesState.normalizeNotebook(saved, 0, "ignored")
    compare(result.migrated, false)
    compare(result.notebook.revision, 4)
    compare(NotesState.activeNote(result.notebook).label, "Second")
  }
  function test_cursor_selection_round_trip_and_unicode_boundaries() {
    var note = NotesState.emptyNotebook("First", "a😀bc", 0)
    note = NotesState.remember(note, 1, 1, 4)
    note = NotesState.persistedNotebook(note)
    var restored = NotesState.normalizeNotebook(JSON.parse(JSON.stringify(note))).notebook
    compare(restored.items[0].cursor, 1)
    compare(restored.items[0].anchor, 4)
    var second = NotesState.addNote(restored)
    compare(NotesState.selectNote(second, 0).items[0].anchor, 4)
    compare(NotesState.position(2, "a😀bc"), 1)
    compare(NotesState.position(-5, "abc"), 0)
    compare(NotesState.position(99, "abc"), 3)
    compare(NotesState.writtenNotebook("broken", "one"), null)
    compare(NotesState.writtenNotebook(JSON.stringify({blades:{right:{slots:[{id:"one",modules:[{module:"notes",state:{text:note}}]}]}}}), "one"), note)
  }

}
