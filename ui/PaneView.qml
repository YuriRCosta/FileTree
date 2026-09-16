import QtQuick
import qs.Commons
import "../lib/Format.js" as Format

Item {
  id: view
  visible: false

  property var context: null
  property bool persist: true
  property var options: []
  property string defaultMetric: "off"
  property var columns: defaultMetric === "off" ? [] : [defaultMetric]
  property var defaultSorts: []
  property var sorts: []
  property var pinnedSortKeys: []
  property var filter: ({})
  property bool activityOption: false
  property bool activity: true
  property var navigationActions: [
    { key: "search", glyph: "󰍉", title: "Search", actions: [{ button: "left", text: "Search" }, { shortcut: "/" }] },
    { key: "filter", glyph: "󰈲", title: "Filter", active: view.filterActive, actions: [{ button: "left", text: "Filter" }, { shortcut: "f" }] }
  ].concat(activityOption ? [
    { key: "activity", glyph: "󰃭", title: "Activity", active: view.activity, actions: [{ button: "left", text: view.activity ? "Hide activity" : "Show activity" }] }
  ] : [])
  property int columnRightReserve: 0
  readonly property string metricKey: columns.length > 0 ? String(columns[0]) : "off"
  readonly property string sortKey: sorts.length > 0 ? String(sorts[0].key) : ""
  readonly property bool sortDescending: sorts.length > 0 && sorts[0].desc === true
  readonly property bool filterActive: Object.keys(filter).length > 0
  readonly property var currentOption: optionFor(metricKey)
  readonly property string metricKind: Format.metricKind(currentOption || metricKey)
  readonly property bool canAddColumn: columns.length < options.filter(function(option) {
    var key = String(option.key || option.value || "")
    return key !== "off" && key !== "none" && pinnedSortKeys.indexOf(key) < 0
  }).length
  readonly property int addColumnReserve: canAddColumn ? Style.space(22) : 0
  readonly property int metricRightMargin: Style.space(7) + columnRightReserve + addColumnReserve

  signal navigationTriggered(string key)

  onNavigationTriggered: function(key) {
    if (key !== "activity") return
    activity = !activity
    remember("activity", activity)
  }

  function optionFor(key) {
    var wanted = String(key || "")
    for (var i = 0; i < options.length; i++)
      if (String(options[i].key) === wanted) return options[i]
    return null
  }

  function kindOf(key) {
    return Format.metricKind(optionFor(key) || key)
  }

  function columnWidthFor(key) {
    var option = optionFor(key)
    var declared = option && Number(option.width)
    if (declared > 0) return Style.space(declared)
    var widths = { agents: 112, date: 108, number: 106, text: 96 }
    return Style.space(widths[kindOf(key)] || 96)
  }

  function validMetric(key) {
    return optionFor(key) ? String(key) : "off"
  }

  function remember(key, value) {
    if (persist && context && context.state) context.state.set(key, value)
  }

  function cleanColumns(list) {
    var result = []
    var source = Array.isArray(list) ? list : []
    for (var i = 0; i < source.length; i++) {
      var key = validMetric(source[i])
      if (key !== "off" && pinnedSortKeys.indexOf(key) < 0 && result.indexOf(key) < 0) result.push(key)
    }
    return result
  }

  function sortsWithinColumns(list) {
    return list.filter(function(item) {
      return item.key === "name" || columns.indexOf(item.key) >= 0 || pinnedSortKeys.indexOf(item.key) >= 0
    })
  }

  function setColumns(list) {
    columns = cleanColumns(list)
    remember("columns", columns.length > 0 ? columns : null)
    remember("metric", metricKey)
    var kept = sortsWithinColumns(sorts)
    if (kept.length !== sorts.length) setSorts(kept)
    columnsCommitted()
  }

  function setMetric(key) {
    var wanted = validMetric(key)
    setColumns(wanted === "off" ? columns.slice(1) : [wanted].concat(columns.slice(1)))
  }

  function setColumn(index, key) {
    var next = columns.slice()
    var wanted = validMetric(key)
    if (index < 0 || index >= next.length) return addColumn(wanted)
    if (wanted === "off") next.splice(index, 1)
    else next[index] = wanted
    setColumns(next)
  }

  function addColumn(key) {
    var wanted = validMetric(key)
    if (wanted === "off" || columns.indexOf(wanted) >= 0) return
    setColumns(columns.concat([wanted]))
  }

  function removeColumn(index) {
    var next = columns.slice()
    if (index < 0 || index >= next.length) return
    var removed = next.splice(index, 1)[0]
    setColumns(next)
    dropSort(removed)
  }

  function moveColumn(from, to) {
    var next = columns.slice()
    if (from < 0 || from >= next.length || to < 0 || to >= next.length || from === to) return
    var moved = next.splice(from, 1)[0]
    next.splice(to, 0, moved)
    setColumns(next)
  }

  signal columnsCommitted()

  function cleanSorts(list) {
    var result = []
    var source = Array.isArray(list) ? list : (list && typeof list === "object" && list.key !== undefined ? [list] : [])
    for (var i = 0; i < source.length; i++) {
      var entry = source[i] && typeof source[i] === "object" ? source[i] : ({})
      var key = String(entry.key || "")
      var known = key === "name" || !!optionFor(key)
      var duplicate = result.some(function(item) { return item.key === key })
      if (key === "" || !known || duplicate) continue
      result.push({ key: key, desc: entry.desc === true })
    }
    return result
  }

  function rememberSorts() {
    remember("sort", sorts)
  }

  function setSorts(list) {
    sorts = cleanSorts(list)
    rememberSorts()
  }

  function setSort(key, descending) {
    setSorts([{ key: String(key || ""), desc: !!descending }])
  }

  function sortFor(key) {
    for (var i = 0; i < sorts.length; i++)
      if (sorts[i].key === String(key)) return sorts[i]
    return null
  }

  function dropSort(key) {
    setSorts(sorts.filter(function(item) { return item.key !== String(key) }))
  }

  function toggleSort(key) {
    var wanted = String(key || metricKey)
    if (wanted === "off" || wanted === "none" || wanted === "") return
    var descendingByDefault = kindOf(wanted) !== "text"
    var current = sortFor(wanted)
    if (!current) return setSorts(sorts.concat([{ key: wanted, desc: descendingByDefault }]))
    if (current.desc === descendingByDefault)
      return setSorts(sorts.map(function(item) { return item.key === wanted ? { key: wanted, desc: !item.desc } : item }))
    dropSort(wanted)
  }

  function clearSort() {
    setSorts([])
  }

  function cleanFilter(spec) {
    var result = ({})
    var source = spec && typeof spec === "object" ? spec : ({})
    for (var key in source) {
      var clause = source[key]
      if (!clause || typeof clause !== "object" || !optionFor(key)) continue
      var kept = ({})
      var fields = ["since", "until", "min", "max"]
      for (var i = 0; i < fields.length; i++) {
        var value = clause[fields[i]]
        if (value !== undefined && value !== null && value !== "") kept[fields[i]] = value
      }
      if (Object.keys(kept).length > 0) result[key] = kept
    }
    return result
  }

  function setFilter(spec) {
    filter = cleanFilter(spec)
    remember("filter", filterActive ? filter : null)
  }

  function clearFilter() {
    setFilter({})
  }

  function restore() {
    if (!persist || !context || !context.state) return
    var saved = context.state.get("columns", null)
    if (Array.isArray(saved)) columns = cleanColumns(saved)
    else columns = cleanColumns([context.state.get("metric", defaultMetric)])
    var savedSorts = context.state.get("sort", undefined)
    sorts = sortsWithinColumns(cleanSorts(savedSorts === undefined ? defaultSorts : savedSorts))
    filter = cleanFilter(context.state.get("filter", null))
    activity = context.state.get("activity", true) !== false
  }

  onContextChanged: restore()
  onOptionsChanged: restore()
}
