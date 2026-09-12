.pragma library
.import "../../lib/SearchQuery.js" as SearchQuery
.import "../../lib/TreeOrder.js" as TreeOrder

var IMAGE_EXTENSIONS = "png jpg jpeg jpe jfif webp gif bmp dib ico cur pbm pgm ppm pnm pam pfm svg svgz tif tiff heif heic avif jxl raw arw cr2 cr3 dng nef nrw orf raf rw2 pef srw x3f hdr exr pic psd psb xcf kra ora ai eps".split(" ")
var VIDEO_EXTENSIONS = "mp4 m4v mov qt mkv webm avi mpeg mpg mpe m2v mts m2ts ts vob ogv ogm 3gp 3g2 flv wmv asf rm rmvb divx mxf".split(" ")

function kind(row) {
  if (!row || row.isDir || row.gitDeleted) return ""
  var mime = String(row.mime || "").toLowerCase()
  if (mime.indexOf("image/") === 0) return "image"
  if (mime.indexOf("video/") === 0) return "video"
  var extension = String(row.name || "").toLowerCase().split(".").pop()
  if (IMAGE_EXTENSIONS.indexOf(extension) >= 0) return "image"
  return VIDEO_EXTENSIONS.indexOf(extension) >= 0 ? "video" : ""
}

function record(row) {
  var relative = String(row.relative || row.name || "")
  var parts = relative.split("/"), folders = []
  for (var i = 1; i < parts.length; i++) folders.push(parts.slice(0, i).join("/"))
  return Object.assign({}, row, {
    date: row.modified || "", datePrecision: "second", stamp: row.statFingerprint || row.modified || "",
    text: relative, fields: {
      name: row.name, type: ["file", kind(row)].concat(row.isSymlink ? ["link"] : []),
      format: String(row.name || "").toLowerCase().split(".").pop(), in: folders, mime: String(row.mime || "").toLowerCase()
    }
  })
}

function matching(rows, query, options, filter) {
  var spec = SearchQuery.parse(query, ["type", "format", "in", "mime"], options)
  if (spec.invalid) return { rows: [], invalid: spec.invalid }
  var kept = rows.filter(function(row) {
    return TreeOrder.passes(row, TreeOrder.normalizeFilter(filter)) && SearchQuery.matches(spec, row)
  })
  return { rows: kept, invalid: "" }
}

function retained(entries, rows) {
  var paths = Object.create(null)
  rows.forEach(function(row) { paths[row.path] = row })
  return entries.filter(function(entry) { return !!paths[entry.path] }).map(function(entry) { return paths[entry.path] })
}

function allows(descriptor, capability) {
  if (!descriptor) return true
  var capabilities = descriptor.capabilities
  return Array.isArray(capabilities) ? capabilities.indexOf(capability) >= 0 : !!capabilities && capabilities[capability] === true
}
