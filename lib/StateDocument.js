.pragma library

var preferenceKeys = ["showHidden", "priorityProperty", "priorityColumns", "gitEnabled", "gitSummaryFields",
  "projectContext", "gitStatusDetails", "propertyIcons", "confirmTrash", "scrollMarks", "autoHideSearch",
  "showSystemVolumes", "modeBadge", "folderColorScope", "searchCaseSensitive", "searchRegex",
  "searchTreeLayout", "searchDeep", "treeSort", "treeFilter"]

function mergeFields(original, current, inheritedKeys) {
  var merged = original && typeof original === "object" && !Array.isArray(original)
    ? JSON.parse(JSON.stringify(original)) : ({})
  var keys = Object.keys(current)
  for (var i = 0; i < keys.length; i++) {
    if ((inheritedKeys || []).indexOf(keys[i]) >= 0 && !Object.prototype.hasOwnProperty.call(merged, keys[i])) continue
    Object.defineProperty(merged, keys[i], { value: current[keys[i]], enumerable: true, writable: true, configurable: true })
  }
  return merged
}

function hydrationPlan(response) {
  if (!response || typeof response !== "object") {
    return { apply: false, writable: false, text: "", retry: true, warning: "state read returned nothing; keeping state.json read-only" }
  }
  if (response.ok) {
    var quarantined = response.quarantined ? String(response.quarantined) : ""
    return {
      apply: true,
      writable: true,
      text: String(response.text || ""),
      retry: false,
      warning: quarantined ? "unreadable state.json moved to " + quarantined + " (" + String(response.error || "") + ")" : ""
    }
  }
  return {
    apply: false,
    writable: false,
    text: "",
    retry: !response.cancelled,
    warning: "preserving unreadable state.json: " + String(response.error || "read failed")
  }
}
