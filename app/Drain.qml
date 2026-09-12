import QtQuick

Item {
  id: drain

  required property var shell
  required property var service
  property string activeToken: ""
  property double expiresAt: 0
  property int cycle: 0
  property string stateReadId: ""
  property string stateTargetText: ""
  property string stateConfirmedText: ""
  property string stateMismatchText: ""
  property int stateMismatchCount: 0
  property string stateError: ""
  property int stateReadSerial: 0
  property bool traversalExhausted: false

  function objects() {
    var result = [], queue = [shell, service], seen = []
    traversalExhausted = false
    while (queue.length && seen.length < 20000) {
      var item = queue.shift()
      if (!item || seen.indexOf(item) >= 0) continue
      seen.push(item)
      result.push(item)
      for (var key of ["children", "data", "instances"]) {
        try {
          var entries = item[key]
          if (entries && entries.length !== undefined)
            for (var i = 0; i < entries.length; i++) queue.push(entries[i])
        } catch (error) {}
      }
      for (var child of ["item", "contentItem"]) if (item[child]) queue.push(item[child])
    }
    traversalExhausted = queue.length > 0
    return result
  }

  function appendUnique(values, value) {
    if (values.indexOf(value) < 0) values.push(value)
  }

  function serialized(value) {
    try {
      var text = JSON.stringify(value, null, 2)
      return typeof text === "string" ? text + "\n" : ""
    } catch (error) {
      return ""
    }
  }

  function output(ready, dirty, pending, error) {
    return JSON.stringify({ ready: !!ready, dirty_note_ids: dirty, pending: pending, error: String(error || "") })
  }

  function invalid(error) {
    return output(false, [], ["token"], error)
  }

  function checkToken(token) {
    var wanted = token === undefined || token === null ? "" : String(token)
    if (!wanted) return { ok: false, value: invalid("drain token is required") }
    if (!activeToken || wanted !== activeToken)
      return { ok: false, value: invalid("drain token does not match") }
    if (Date.now() >= expiresAt) {
      activeToken = ""
      expiresAt = 0
      cycle++
      return { ok: false, value: invalid("drain token expired") }
    }
    return { ok: true, token: wanted }
  }

  function stateController(items) {
    for (var i = 0; i < items.length; i++) {
      var item = items[i]
      if (item && (item.service === undefined || item.service === service)
          && typeof item.document === "function" && typeof item.save === "function"
          && item.stateWriteRequestId !== undefined && item.queuedStateDocument !== undefined
          && item.stateWritable !== undefined && item.ready !== undefined) return item
    }
    return null
  }

  function notes(items) {
    var result = [], seen = []
    for (var i = 0; i < items.length; i++) {
      var item = items[i]
      if (!item) continue
      if (String(item.moduleId || "") !== "notes" || !item.moduleItem) continue
      var candidate = item.moduleItem
      if (seen.indexOf(candidate) >= 0) continue
      seen.push(candidate)
      var context = candidate.context !== undefined ? candidate.context
        : item.moduleContext !== undefined ? item.moduleContext
        : item.context !== undefined ? item.context : null
      result.push({ item: candidate, context: context })
    }
    return result
  }

  function knownNotes(item) {
    return !!item && typeof item.flush === "function"
      && item.dirty !== undefined && item.saving !== undefined
      && item.saveFailed !== undefined && item.incomingConflict !== undefined
      && item.overCap !== undefined && item.pendingNotebook !== undefined
  }

  function noteIds(entry) {
    var item = entry.item
    var context = entry.context || ({})
    var result = []
    var tab = Number(context.tabIndex)
    tab = isFinite(tab) && tab >= 0 ? Math.floor(tab) : 0
    if (context.inPopout) appendUnique(result, "popout:" + String(context.moduleId || item.moduleId || "notes") + ":" + tab)
    else if (String(context.slotId || "")) appendUnique(result, "slot:" + String(context.slotId) + ":" + tab)
    var items = item.notebook && Array.isArray(item.notebook.items) ? item.notebook.items : []
    for (var i = 0; i < items.length; i++) if (items[i] && items[i].id !== undefined)
      appendUnique(result, "note:" + String(items[i].id))
    if (result.length === 0) appendUnique(result, "notes:" + String(context.moduleId || item.moduleId || "notes"))
    return result
  }

  function noteUnsafe(item) {
    return !!item.dirty || !!item.saving || !!item.saveFailed || !!item.incomingConflict
      || !!item.overCap || !!item.pendingNotebook
  }

  function syncState(controller, pending, requestWrites) {
    if (controller.ready !== true) {
      appendUnique(pending, "state")
      return ""
    }
    var current = ""
    try { current = serialized(controller.document()) } catch (failure) { return "state document could not be read" }
    if (!current) return "state document could not be serialized"
    if (stateReadId) {
      appendUnique(pending, "state")
      return ""
    }
    if (stateTargetText !== current) {
      stateTargetText = current
      stateConfirmedText = ""
      stateError = ""
      if (stateMismatchText !== current) {
        stateMismatchText = ""
        stateMismatchCount = 0
      }
      if (!requestWrites) {
        appendUnique(pending, "state")
        return ""
      }
      try { controller.save() } catch (failure) { return "state save failed: " + String(failure) }
      appendUnique(pending, "state")
      return ""
    }
    if (controller.stateWriteRequestId || controller.queuedStateDocument) {
      appendUnique(pending, "state")
      return ""
    }
    if (stateConfirmedText === current) return ""
    if (stateError) {
      appendUnique(pending, "state")
      return stateError
    }
    if (!requestWrites) {
      appendUnique(pending, "state")
      return ""
    }
    if (!service || typeof service.backendRequest !== "function") return "state reader is unavailable"
    var token = activeToken
    var generation = cycle
    var serial = ++stateReadSerial
    stateReadId = service.backendRequest("state-read", [], "drain-state-read-" + generation + "-" + serial, function(response) {
      if (serial !== stateReadSerial) return
      stateReadId = ""
      if (generation !== cycle || token !== activeToken || Date.now() >= expiresAt) return
      if (!response || !response.ok) {
        stateError = "state read failed: " + String(response && response.error || "read failed")
        stateConfirmedText = ""
        return
      }
      var latest = ""
      try { latest = serialized(controller.document()) } catch (failure) { latest = "" }
      if (latest !== current) {
        stateTargetText = ""
        stateConfirmedText = ""
        stateMismatchText = ""
        stateMismatchCount = 0
        stateError = ""
        return
      }
      if (controller.stateWriteRequestId || controller.queuedStateDocument) {
        stateConfirmedText = ""
        stateError = ""
        return
      }
      if (String(response.text || "") !== current) {
        stateConfirmedText = ""
        if (stateMismatchText === current) stateMismatchCount++
        else {
          stateMismatchText = current
          stateMismatchCount = 1
        }
        if (stateMismatchCount <= 1) {
          stateTargetText = ""
          stateError = ""
        } else stateError = "state readback did not match requested document"
        return
      }
      stateConfirmedText = current
      stateMismatchText = ""
      stateMismatchCount = 0
      stateError = ""
    })
    appendUnique(pending, "state")
    return ""
  }

  function inspect(requestWrites) {
    var pending = [], dirty = [], error = ""
    var items = objects()
    if (traversalExhausted)
      return { ready: false, dirty: [], pending: ["objects"], error: "object traversal limit reached" }
    var entries = notes(items)
    var unknownNotes = false
    for (var i = 0; i < entries.length; i++) {
      var discovered = entries[i]
      var discoveredTemporary = !!(discovered.context && discovered.context.inPopout)
      if (!discovered.context || !knownNotes(discovered.item)) {
        unknownNotes = true
        appendUnique(pending, "notes")
        error = error || "Notes persistence API is unavailable"
        if (discoveredTemporary) {
          var unavailableIds = noteIds(discovered)
          for (var u = 0; u < unavailableIds.length; u++) appendUnique(dirty, unavailableIds[u])
        }
      }
      else if (discoveredTemporary) {
        var temporaryIds = noteIds(discovered)
        for (var t = 0; t < temporaryIds.length; t++) appendUnique(dirty, temporaryIds[t])
        appendUnique(pending, "notes")
        error = error || "temporary Notes cannot be durably drained"
      }
    }

    if (!error && !unknownNotes) {
      for (var j = 0; j < entries.length; j++) {
        var entry = entries[j]
        if (entry.context.inPopout || !requestWrites || !entry.item.dirty) continue
        try { entry.item.flush() } catch (failure) { error = error || "Notes flush failed: " + String(failure) }
      }
    }

    for (var k = 0; k < entries.length; k++) {
      var current = entries[k]
      var notesItem = current.item
      if (!current.context || !knownNotes(notesItem)) continue
      if (!noteUnsafe(notesItem)) continue
      var ids = noteIds(current)
      for (var n = 0; n < ids.length; n++) appendUnique(dirty, ids[n])
      appendUnique(pending, "notes")
    }

    var host = service && service.bladeHost ? service.bladeHost : null
    if (!host) {
      appendUnique(pending, "layout")
      error = error || "layout host is unavailable"
    } else if (host.layoutReady !== true) {
      appendUnique(pending, "layout")
    } else {
      var layout = ""
      try { layout = serialized(host.layoutDocument()) } catch (failure) { error = error || "layout document could not be read" }
      if (!layout) error = error || "layout document could not be serialized"
      else if (host.layoutWritable !== true) {
        if (layout !== String(host.lastWrittenLayoutText || "") || layout !== String(host.lastSavedLayoutText || ""))
          error = error || "layout is not writable"
      } else if (host.layoutWriteRequestId || host.queuedLayoutDocument) {
        appendUnique(pending, "layout")
      } else {
        var written = String(host.lastWrittenLayoutText || "")
        var saved = String(host.lastSavedLayoutText || "")
        if (layout !== written || layout !== saved) {
          if (requestWrites) {
            try { host.save() } catch (layoutFailure) { error = error || "layout save failed: " + String(layoutFailure) }
          }
          appendUnique(pending, "layout")
        }
      }
    }

    var controller = stateController(items)
    if (!controller) {
      appendUnique(pending, "state")
      error = error || "state controller is unavailable"
    } else if (controller.stateWritable !== true) {
      error = error || "state is not writable"
    } else {
      var stateFailure = syncState(controller, pending, requestWrites)
      error = error || stateFailure
    }
    if (stateError) error = error || stateError
    return { ready: error === "" && dirty.length === 0 && pending.length === 0, dirty: dirty, pending: pending, error: error }
  }

  function begin(token, timeoutMs) {
    var wanted = token === undefined || token === null ? "" : String(token)
    if (!wanted) return invalid("drain token is required")
    var now = Date.now()
    if (activeToken && now < expiresAt && activeToken !== wanted)
      return output(false, [], ["token"], "drain token is already owned")
    if (!activeToken || now >= expiresAt || activeToken !== wanted) {
      activeToken = wanted
      cycle++
      stateReadId = ""
      stateTargetText = ""
      stateConfirmedText = ""
      stateMismatchText = ""
      stateMismatchCount = 0
      stateError = ""
      stateReadSerial++
      var requested = Number(timeoutMs)
      requested = isFinite(requested) && requested > 0 ? Math.min(300000, Math.floor(requested)) : 30000
      expiresAt = now + requested
    }
    var state = inspect(true)
    return output(state.ready, state.dirty, state.pending, state.error)
  }

  function status(token) {
    var checked = checkToken(token)
    if (!checked.ok) return checked.value
    var state = inspect(true)
    return output(state.ready, state.dirty, state.pending, state.error)
  }

  function abort(token) {
    var checked = checkToken(token)
    if (!checked.ok) return checked.value
    activeToken = ""
    expiresAt = 0
    cycle++
    stateReadSerial++
    stateReadId = ""
    stateTargetText = ""
    stateConfirmedText = ""
    stateMismatchText = ""
    stateMismatchCount = 0
    stateError = ""
    return output(false, [], [], "drain aborted")
  }

  function commit(token) {
    var checked = checkToken(token)
    if (!checked.ok) return checked.value
    var state = inspect(false)
    var result = output(state.ready, state.dirty, state.pending, state.error)
    if (state.ready) Qt.quit()
    return result
  }
}
