.pragma library

function modules(raw) {
  var source = raw && typeof raw === "object" ? raw : ({})
  var blades = source.blades && typeof source.blades === "object" ? source.blades : source
  var found = []
  var edges = ["left", "right"]
  for (var e = 0; e < edges.length; e++) {
    var blade = blades[edges[e]]
    if (!blade || typeof blade !== "object") continue
    var slots = Array.isArray(blade.slots) ? blade.slots : []
    for (var s = 0; s < slots.length; s++) {
      var slot = slots[s] && typeof slots[s] === "object" ? slots[s] : { module: slots[s] }
      var tabs = Array.isArray(slot.modules) ? slot.modules : [slot]
      for (var t = 0; t < tabs.length; t++) {
        var tab = tabs[t] && typeof tabs[t] === "object" ? tabs[t] : { module: tabs[t] }
        var name = String(tab.module || "")
        if (name !== "" && found.indexOf(name) < 0) found.push(name)
      }
    }
  }
  return found.sort()
}

function missing(raw, normalized, resolve) {
  var alias = typeof resolve === "function" ? resolve : function(name) { return name }
  var wanted = modules(raw)
  var kept = modules({ blades: normalized })
  var lost = []
  for (var i = 0; i < wanted.length; i++) {
    var name = wanted[i]
    if (kept.indexOf(name) < 0 && kept.indexOf(String(alias(name) || "")) < 0) lost.push(name)
  }
  return lost
}
