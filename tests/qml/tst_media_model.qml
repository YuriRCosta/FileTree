import QtQuick
import QtTest
import "../../modules/files/MediaModel.js" as MediaModel

TestCase {
  name: "MediaModel"

  function row(path, mime, extra) {
    return MediaModel.record(Object.assign({ path: path, name: path.split("/").pop(), mime: mime, modified: "2024-02-29 12:00:00", size: 20 }, extra || {}))
  }

  function test_recognition_keeps_unsupported_decoders_visible() {
    var families = ["png", "jpg", "webp", "bmp", "ico", "pbm", "pgm", "ppm", "pam", "pfm", "gif", "svg", "tiff", "heic", "avif", "jxl", "cr3", "hdr", "exr", "psd", "mkv", "mp4", "webm", "avi", "ogv", "wmv", "flv"]
    families.forEach(function(ext) { verify(MediaModel.kind(row("/a." + ext, "application/octet-stream")) !== "") })
    compare(MediaModel.kind(row("/no-suffix", "video/mp4")), "video")
    compare(MediaModel.kind(row("/misleading.txt", "image/png")), "image")
    compare(MediaModel.kind(row("/text.txt", "text/plain")), "")
    compare(MediaModel.kind(row("/folder.png", "inode/directory", { isDir: true })), "")
    compare(MediaModel.kind(row("/gone.png", "image/png", { gitDeleted: true })), "")
  }

  function test_query_and_metadata_filters() {
    var rows = [row("/a.png", "image/png"), row("/b.mp4", "video/mp4"), row("/c.png", "image/png", { size: 100 })]
    compare(MediaModel.matching(rows, "type:video", {}, {}).rows.length, 1)
    var nested = row("/root/nested/deep/a.png", "image/png", { relative: "nested/deep/a.png" })
    compare(MediaModel.matching([nested], "in:nested/deep mime:image/png", {}, {}).rows.length, 1)
    compare(MediaModel.matching([nested], "in:other", {}, {}).rows.length, 0)
    compare(MediaModel.matching(rows, "format:png -c", {}, {}).rows[0].name, "a.png")
    compare(MediaModel.matching(rows, "", {}, { size: { min: 50 } }).rows.length, 1)
    compare(MediaModel.matching(rows, "[", { regex: true }, {}).rows.length, 0)
    verify(MediaModel.matching(rows, "[", { regex: true }, {}).invalid !== "")
  }

  function test_selection_uses_exact_path_identity() {
    var a = row("/dir/a #%.png", "image/png")
    var b = row("/else/a #%.png", "image/png")
    compare(MediaModel.retained([a, b], [b]).map(function(item) { return item.path }), [b.path])
    compare(MediaModel.retained([a], []).length, 0)
  }

  function test_capabilities_are_explicit_for_descriptors() {
    verify(MediaModel.allows(null, "read"))
    verify(!MediaModel.allows({}, "read"))
    verify(!MediaModel.allows({ capabilities: ["read"] }, "trash"))
    verify(MediaModel.allows({ capabilities: ["read"] }, "read"))
    verify(!MediaModel.allows({ capabilities: { read: false } }, "read"))
  }
}
