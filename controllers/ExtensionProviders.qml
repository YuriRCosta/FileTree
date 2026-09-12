import QtQuick
import qs.Commons

QtObject {
  id: manager

  property var providers: []
  property var builtinProviders: []
  property var disclosed: ({})
  property var files: null
  property string inventoryUrl: ""
  property var services: ({})
  property var errors: ({})
  property var live: ({})
  readonly property int maximumProviders: 32

  function socketEntries(manifest, socketId) {
    if (!manifest || typeof manifest !== "object") return []
    var extensions = manifest.extensions
    if (!extensions || typeof extensions !== "object" || Array.isArray(extensions)) return []
    return Array.isArray(extensions[socketId]) ? extensions[socketId] : []
  }

  function providerEntry(manifest) {
    var entries = socketEntries(manifest, "data-goblin.fileblade/blade")
    for (var i = 0; i < entries.length; i++) {
      var entry = entries[i]
      if (!entry || typeof entry !== "object") continue
      if (entry.provider === null) return ""
      var declared = String(entry.provider || "").trim()
      if (!declared) continue
      if (declared.charAt(0) === "/" || declared.indexOf("..") >= 0) return ""
      if (declared.length > 512 || /[\u0000-\u001f\u007f]/.test(declared)) return ""
      return declared
    }
    return ""
  }

  function identityOf(row, entry) {
    return String(row.id) + " " + String(row.dir) + " " + entry
  }

  function builtinRows() {
    var rows = Array.isArray(builtinProviders) ? builtinProviders.slice(0, 4) : []
    return rows.filter(function(row) {
      return row && ["fileblade.core.skills", "fileblade.core.memory", "fileblade.core.hooks", "fileblade.core.mcp"].indexOf(row.id) >= 0
        && row.dir && row.entry === "Provider.qml"
    })
  }

  function absorbedProvider(id, builtins) {
    for (var i = 0; i < builtins.length; i++) {
      var module = builtins[i].id.slice("fileblade.core.".length)
      if (id === "data-goblin.fileblade-" + module || id === "kurt.agent-" + module) return true
    }
    return false
  }

  function rebuild() {
    var next = ({})
    var exposed = ({})
    var failed = ({})
    var builtins = builtinRows()
    var external = Array.isArray(providers) ? providers : []
    var rows = builtins.concat(external)
    for (var i = 0; i < rows.length && Object.keys(next).length < maximumProviders; i++) {
      var row = rows[i]
      var builtin = i < builtins.length
      if (!row || (!builtin && row.enabled !== true) || !row.id || !row.dir || next[row.id]) continue
      if (!builtin && (String(row.id).indexOf("fileblade.core.") === 0 || absorbedProvider(row.id, builtins))) continue
      if (!builtin && disclosed && disclosed[row.id]) continue
      var entry = builtin ? row.entry : providerEntry(row.manifest)
      if (!entry) continue
      var identity = identityOf(row, entry)
      var existing = live[row.id]
      if (existing && existing.identity === identity && existing.object) {
        next[row.id] = existing
        exposed[row.id] = existing.object
        continue
      }
      var object = create(row, entry)
      if (!object) {
        failed[row.id] = "The extension's provider could not be loaded"
        continue
      }
      next[row.id] = { identity: identity, object: object }
      exposed[row.id] = object
    }
    var ids = Object.keys(live)
    for (var j = 0; j < ids.length; j++) {
      if (next[ids[j]] === live[ids[j]]) continue
      retire(live[ids[j]])
    }
    live = next
    services = exposed
    errors = failed
  }

  function create(row, entry) {
    if (!files || !inventoryUrl) return null
    var source = Util.fileUrl(String(row.dir).replace(/\/$/, "") + "/" + entry)
    if (!source) return null
    var component = Qt.createComponent(source, Component.PreferSynchronous)
    if (component.status !== Component.Ready) {
      component.destroy()
      return null
    }
    var object = component.createObject(manager, {
      providerId: String(row.id),
      providerRoot: String(row.dir),
      files: manager.files,
      inventoryComponentUrl: manager.inventoryUrl
    })
    component.destroy()
    return object
  }

  function retire(record) {
    if (!record || !record.object) return
    if (typeof record.object.shutdown === "function") record.object.shutdown()
    record.object.destroy()
  }

  function shutdown() {
    var ids = Object.keys(live)
    for (var i = 0; i < ids.length; i++) retire(live[ids[i]])
    live = ({})
    services = ({})
  }

  onProvidersChanged: rebuild()
  onBuiltinProvidersChanged: rebuild()
  onDisclosedChanged: rebuild()
  onFilesChanged: rebuild()
  onInventoryUrlChanged: rebuild()
}
