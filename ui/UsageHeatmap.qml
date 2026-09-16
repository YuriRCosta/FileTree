import QtQuick
import qs.Commons
import "../lib/MediaDates.js" as Dates

Item {
  id: heatmap

  property var payload: null
  property var calendarRule: Dates.localeRule(Qt.locale().name, Qt.locale().firstDayOfWeek)
  property string unitLabel: "uses"
  property int cursor: -1
  readonly property int cell: Style.space(8)
  readonly property int gap: Style.space(2)
  readonly property int pitch: cell + gap
  readonly property var usage: parse(payload)
  readonly property int today: usage ? usage.today : 0
  readonly property int weeks: usage ? Math.max(0, Math.min(160, Math.floor((width + gap) / pitch))) : 0
  readonly property int firstDay: Dates.weekStart(today, calendarRule) - 7 * (weeks - 1)
  readonly property real offset: width - weeks * pitch + gap
  readonly property int focusDay: cursor >= firstDay && cursor <= today ? cursor : today
  readonly property int hoverDay: pointer.containsMouse ? dayAt(pointer.mouseX, pointer.mouseY) : -1
  readonly property int tipDay: hoverDay >= 0 ? hoverDay : (activeFocus ? focusDay : -1)

  signal dismissed()

  visible: weeks > 0
  implicitHeight: 7 * pitch - gap
  activeFocusOnTab: true
  Accessible.role: Accessible.Pane
  Accessible.name: "Daily " + unitLabel
  Accessible.description: weeks > 0 ? describe(focusDay) : ""

  function ordinal(text) {
    var match = /^(\d{4})-(\d{2})-(\d{2})$/.exec(String(text))
    if (!match || !Dates.valid(Number(match[1]), Number(match[2]), Number(match[3]))) return null
    return Dates.ordinal(Number(match[1]), Number(match[2]), Number(match[3]))
  }

  function parse(value) {
    var until = value && Array.isArray(value.days) ? ordinal(value.until) : null
    if (until === null) return null
    var start = ordinal(value.coverageStart)
    var days = ({}), counts = []
    for (var row of value.days) {
      var day = Array.isArray(row) ? ordinal(row[0]) : null
      if (day === null) continue
      days[day] = row.slice(1, 6).map(function(count) { return Math.max(0, Number(count) || 0) })
      if (days[day][0] > 0) counts.push(days[day][0])
    }
    counts.sort(function(a, b) { return a - b })
    var quartile = function(share) { return counts.length ? counts[Math.ceil(share * counts.length) - 1] : 0 }
    return { today: until, coverage: start === null ? until + 1 : start, days: days,
             thresholds: [quartile(0.25), quartile(0.5), quartile(0.75)] }
  }

  function countsFor(day) {
    return usage.days[day] || [0, 0, 0, 0, 0]
  }

  function fill(day) {
    if (!usage || day < usage.coverage) return "transparent"
    var uses = countsFor(day)[0], limits = usage.thresholds
    var level = uses <= 0 ? 0 : (uses <= limits[0] ? 1 : (uses <= limits[1] ? 2 : (uses <= limits[2] ? 3 : 4)))
    return level === 0 ? Util.alpha(Color.bar.text, 0.06) : Util.alpha(Color.accent, [0.30, 0.50, 0.75, 1.0][level - 1])
  }

  function describe(day) {
    if (!usage) return ""
    var date = Dates.calendar(day)
    var label = Qt.locale().toString(new Date(date.year, date.month - 1, date.day), "ddd d MMM yyyy") + ": "
    if (day < usage.coverage) return label + "no history yet"
    var entry = countsFor(day)
    var parts = [[entry[1], "agent"], [entry[2], "you"], [entry[3], "scheduled"], [entry[4], "failed"]]
      .filter(function(part) { return part[0] > 0 }).map(function(part) { return part[0] + " " + part[1] })
    var total = entry[0] > 0 ? entry[0] + " " + (entry[0] === 1 ? unitLabel.replace(/s$/, "") : unitLabel) : "no " + unitLabel
    return label + total + (parts.length ? " (" + parts.join(", ") + ")" : "")
  }

  function cellX(day) { return offset + Math.floor((day - firstDay) / 7) * pitch }
  function cellY(day) { return (day - firstDay) % 7 * pitch }

  function dayAt(x, y) {
    var column = Math.floor((x - offset) / pitch), row = Math.floor(y / pitch)
    var day = firstDay + column * 7 + row
    return column >= 0 && column < weeks && row >= 0 && row < 7 && day <= today ? day : -1
  }

  Keys.onPressed: function(event) {
    if (event.modifiers & (Qt.ControlModifier | Qt.AltModifier | Qt.MetaModifier)) return
    if (event.key === Qt.Key_Escape) {
      heatmap.dismissed()
      event.accepted = true
      return
    }
    var target = ({ [Qt.Key_Left]: focusDay - 7, [Qt.Key_Right]: focusDay + 7, [Qt.Key_Up]: focusDay - 1,
                    [Qt.Key_Down]: focusDay + 1, [Qt.Key_Home]: firstDay, [Qt.Key_End]: today })[event.key]
    if (target === undefined) return
    if (target >= firstDay && target <= today) cursor = target
    event.accepted = true
  }

  Repeater {
    model: heatmap.weeks * 7
    delegate: Rectangle {
      required property int index
      readonly property int day: heatmap.firstDay + index
      objectName: "usage-day-" + Dates.dayKey(day)
      x: heatmap.cellX(day)
      y: heatmap.cellY(day)
      width: heatmap.cell
      height: heatmap.cell
      visible: day <= heatmap.today
      color: heatmap.fill(day)
    }
  }

  Rectangle {
    objectName: "usage-cursor"
    x: heatmap.cellX(heatmap.focusDay) - 1
    y: heatmap.cellY(heatmap.focusDay) - 1
    width: heatmap.cell + 2
    height: heatmap.cell + 2
    visible: heatmap.activeFocus
    color: "transparent"
    border.width: 1
    border.color: Color.accent
  }

  Item {
    id: tipAnchor
    x: heatmap.cellX(heatmap.tipDay)
    y: heatmap.cellY(heatmap.tipDay)
    width: heatmap.cell
    height: heatmap.cell
    onXChanged: tip.place()
    onYChanged: tip.place()
  }

  MouseArea {
    id: pointer
    anchors.fill: parent
    acceptedButtons: Qt.NoButton
    hoverEnabled: true
  }

  HintTip {
    id: tip
    objectName: "usage-tip"
    visible: heatmap.tipDay >= 0
    anchorItem: tipAnchor
    title: heatmap.tipDay >= 0 ? heatmap.describe(heatmap.tipDay) : ""
  }
}
