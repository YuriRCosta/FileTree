.pragma library

function mergeFields(original, current) {
  var merged = original && typeof original === "object" && !Array.isArray(original)
    ? JSON.parse(JSON.stringify(original)) : ({})
  var keys = Object.keys(current)
  for (var i = 0; i < keys.length; i++) {
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
