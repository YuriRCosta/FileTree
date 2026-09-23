import QtQuick
import "../lib/Definitions.js" as Definitions
import "../lib/PathText.js" as PathText

QtObject {
  id: registry

  property string pluginDir: ""
  property string userModulesDir: ""
  property int contractVersion: 1
  property var service: null
  property var modules: ({})
  property var disabledModules: ({})
  property var order: []
  property bool scanning: false
  property bool rescanQueued: false
  property int scanGeneration: 0
  property int revision: 0
  property var fileModules: ({})
  readonly property var coreModules: []
  readonly property var builtinProviders: []
  readonly property string onlyModule: "files"
  readonly property int maximumModules: 128
  readonly property int maximumIdLength: 128
  readonly property int maximumTextLength: 512

  signal registryChanged()

  function canonicalModule(id) {
    return String(id || "").trim()
  }

  function isSafeRelativePath(value) {
    var text = String(value || "")
    return text.length > 0 && text.length <= maximumTextLength && text.charAt(0) !== "/"
      && text.indexOf("..") < 0 && !/[\u0000-\u001f\u007f]/.test(text)
  }

  function boundedText(value, fallback, limit) { return Definitions.boundedText(value, fallback, limit) }

  function moduleId(raw, idPrefix) {
    var bare = Definitions.safeId(raw.id, maximumIdLength)
    if (!bare) return ""
    var id = idPrefix ? idPrefix + "/" + bare : bare
    return id.length <= maximumIdLength ? id : ""
  }

  function normalizedModule(raw, sourceDir, source, idPrefix) {
    if (!raw || typeof raw !== "object" || Array.isArray(raw)) return null
    var id = moduleId(raw, idPrefix)
    var entry = boundedText(raw.entry, "Module.qml", maximumTextLength).trim()
    if (!id) return null
    if (!isSafeRelativePath(entry)) return null
    var directory = String(sourceDir || "").replace(/\/$/, "")
    if (!directory) return null
    var hostContract = Math.max(1, Math.floor(Number(raw.hostContract) || 1))
    var icon = typeof raw.icon === "string" && isSafeRelativePath(raw.icon) ? String(raw.icon) : ""
    return {
      id: id,
      name: boundedText(raw.name, raw.id, maximumTextLength),
      glyph: boundedText(raw.glyph, "", 16),
      iconUrl: icon !== "" ? PathText.fileUrl(PathText.join(directory, icon)) : "",
      description: boundedText(raw.description, "", maximumTextLength),
      entryUrl: PathText.fileUrl(PathText.join(directory, entry)),
      sourceDir: directory,
      source: boundedText(source, "builtin", maximumTextLength),
      providerId: boundedText(idPrefix, "", maximumIdLength),
      singleton: raw.singleton === undefined ? true : !!raw.singleton,
      minHeight: Math.max(0, Math.min(4096, Number(raw.minHeight) || 0)),
      hostContract: hostContract,
      providerEntry: isSafeRelativePath(raw.provider) ? String(raw.provider).trim() : "",
      providerState: raw.provider === null ? "stateless"
        : (isSafeRelativePath(raw.provider) ? "owned" : (raw.provider === undefined ? "legacy" : "invalid")),
      needsUpdate: false,
      compatible: hostContract <= contractVersion,
      category: Definitions.category(raw.category, source),
      settings: Definitions.settingsSpec(raw.settings)
    }
  }

  function packageModules(packageDefinition) {
    if (!packageDefinition || !Array.isArray(packageDefinition.modules)) return [packageDefinition]
    var definitions = packageDefinition.modules
    for (var i = 0; i < definitions.length; i++) {
      if (definitions[i] && definitions[i].hostContract === undefined && packageDefinition.hostContract !== undefined)
        definitions[i].hostContract = packageDefinition.hostContract
    }
    return definitions
  }

  function parseScanOutput(parsed) {
    var found = ({})
    var rows = parsed && Array.isArray(parsed.modules) ? parsed.modules.slice(0, maximumModules) : []
    var accepted = 0
    for (var i = 0; i < rows.length && accepted < maximumModules; i++) {
      var row = rows[i]
      var definitions = packageModules(row.definition)
      for (var j = 0; j < definitions.length && accepted < maximumModules; j++) {
        var module = normalizedModule(definitions[j], row.source_dir, row.source, "")
        if (module && module.id === onlyModule && module.source === "builtin" && !found[module.id]) {
          found[module.id] = module
          accepted++
        }
      }
    }
    fileModules = found
    scanning = false
    rebuild()
    if (rescanQueued) {
      rescanQueued = false
      Qt.callLater(rescan)
    }
  }

  function rebuild() {
    var merged = ({})
    var fromFiles = fileModules
    var fileIds = Object.keys(fromFiles)
    for (var i = 0; i < fileIds.length; i++) merged[fileIds[i]] = fromFiles[fileIds[i]]
    disabledModules = ({})
    var ids = Object.keys(merged)
    ids.sort(function(left, right) {
      var byCategory = Definitions.categoryOrder(merged[left].category, merged[right].category)
      if (byCategory !== 0) return byCategory
      var leftBuiltin = merged[left].source === "builtin" ? 0 : 1
      var rightBuiltin = merged[right].source === "builtin" ? 0 : 1
      if (leftBuiltin !== rightBuiltin) return leftBuiltin - rightBuiltin
      return String(merged[left].name).localeCompare(String(merged[right].name))
    })
    modules = merged
    order = ids
    revision++
    registryChanged()
  }

  function module(id) {
    return modules[canonicalModule(id)] || null
  }

  function disabledModule(id) {
    return disabledModules[canonicalModule(id)] || null
  }

  function entryUrl(id) {
    var found = module(id)
    return found ? String(found.entryUrl) : ""
  }

  function settingKeys(found) {
    var keys = []
    var rows = found.settings && Array.isArray(found.settings.schema) ? found.settings.schema : []
    for (var i = 0; i < rows.length; i++) keys.push(String(rows[i].key))
    return keys
  }

  function ipcDocument(placed) {
    var rows = []
    for (var i = 0; i < order.length; i++) {
      var found = module(order[i])
      if (!found) continue
      rows.push({
        id: found.id,
        name: found.name,
        glyph: found.glyph,
        icon: found.iconUrl !== "" ? found.iconUrl : null,
        description: found.description,
        category: found.category,
        source: found.source,
        singleton: found.singleton,
        entry: found.entryUrl,
        settings: { keys: settingKeys(found) },
        placed: placed(found.id) || null
      })
    }
    return JSON.stringify({ count: rows.length, modules: rows })
  }

  function rescan() {
    if (!pluginDir || !service) return
    if (scanning) {
      rescanQueued = true
      return
    }
    scanning = true
    scanGeneration++
    var requestGeneration = scanGeneration
    service.backendRequest("blade-modules", ["--plugin", pluginDir, "--user", userModulesDir], requestGeneration, function(response) {
      if (requestGeneration !== registry.scanGeneration) return
      registry.parseScanOutput(response)
    })
  }

  onPluginDirChanged: rescan()
  onServiceChanged: rescan()
}
