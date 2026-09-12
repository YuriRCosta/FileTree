import QtQuick
import qs.Commons
import "../lib/MediaBins.js" as Bins
import "../lib/MediaDates.js" as Dates

FocusScope {
  id: timeline

  property var records: []
  property var calendarRule: ({ firstDay: 1, minimumDays: 4 })
  property int columns: 1
  property real pitch: 124
  property real tileHeight: 120
  property real contentY: 0
  property real contentHeight: 0
  property real viewportHeight: 0
  property real headerWidth: width
  property var parents: []
  property string navigatedKey: ""
  property real dragOffset: 0
  readonly property int headerHeight: Style.space(26)
  readonly property real axisTop: headerHeight + Style.space(6)
  readonly property real axisHeight: Math.max(0, height - axisTop - Style.space(6))
  readonly property int capacity: Math.max(2, Math.floor(axisHeight / Style.space(18)))
  readonly property var detail: parents.length ? Bins.child(records, parents[parents.length - 1], capacity, calendarRule) : Bins.overview(records, capacity, calendarRule)
  readonly property var bounds: Bins.geometry(detail.bins, Math.max(1, columns), pitch, tileHeight)
  readonly property var viewport: Bins.viewport(bounds, contentY, viewportHeight)
  readonly property real rowHeight: axisHeight / Math.max(1, detail.bins.length)
  readonly property int activePeriod: {
    for (var i = 0; i < detail.bins.length; i++) if (detail.bins[i].key === navigatedKey) return i
    return viewport.first
  }
  readonly property var period: activePeriod >= 0 ? detail.bins[activePeriod] : null
  readonly property var nextDetail: period ? Bins.child(records, period, capacity, calendarRule) : null
  readonly property bool canDrill: !!nextDetail && period.count > 0 && nextDetail.bins.length <= capacity
  readonly property bool canGoUp: parents.length > 0
  readonly property string periodLabel: period ? label(period, false) : (parents.length ? label(parents[parents.length - 1], false) : "All dates")
  readonly property string levelLabel: ({ ranges: "Years", years: "Years", months: "Months", weeks: "Weeks", days: "Days" })[detail.level] || "Dates"
  readonly property color lightBlue: "#89b4fa"
  readonly property color darkBlue: Qt.darker(lightBlue, 2.25)
  readonly property real outlineTop: viewport.start === null ? 0 : axisTop + viewport.start * rowHeight
  readonly property real outlineHeight: viewport.start === null ? 0 : Math.max(1, (viewport.end - viewport.start) * rowHeight)

  signal seekRequested(real position)

  width: Style.space(92)
  activeFocusOnTab: true
  Accessible.role: Accessible.Pane
  Accessible.name: "Media date timeline"
  Accessible.description: "Up and Down seek periods. Left shows coarser dates. Right shows finer dates."

  function label(entry, compact) {
    if (entry.level === "undated") return "Undated"
    if (entry.level === "unknown") return compact ? "Unknown" : "Unknown precision"
    if (entry.level === "ranges" || entry.level === "years") return entry.key
    if (entry.level === "weeks") return (compact ? "W" : entry.weekYear + " · Week ") + Dates.pad(entry.week)
    var date = Dates.calendar(entry.start)
    if (entry.level === "days") return compact ? Dates.pad(date.month) + "-" + Dates.pad(date.day) : Dates.dayKey(entry.start)
    if (compact) return String(date.year).slice(-2) + "·" + Dates.pad(date.month)
    return Qt.locale().standaloneMonthName(date.month - 1, Locale.ShortFormat) + " " + date.year
  }

  function reset() { parents = []; navigatedKey = "" }

  function drill() {
    if (!canDrill) return
    parents = parents.concat([period])
    navigatedKey = ""
  }

  function up() {
    if (!canGoUp) return
    var previous = parents[parents.length - 1]
    parents = parents.slice(0, -1)
    navigatedKey = previous.key
  }

  function seek(index, fraction) {
    var position = Bins.seek(bounds, index, fraction, contentHeight, viewportHeight)
    if (position === null) return false
    navigatedKey = detail.bins[index].key
    seekRequested(position)
    return true
  }

  function seekAt(y) {
    var position = Math.max(0, Math.min(detail.bins.length - 0.00001, (y - axisTop) / Math.max(1, rowHeight)))
    seek(Math.floor(position), position - Math.floor(position))
  }

  function movePeriod(direction, edge) {
    var index = edge ? (direction > 0 ? 0 : detail.bins.length - 1) : activePeriod + direction
    for (; index >= 0 && index < detail.bins.length; index += direction) if (seek(index, 0)) return
  }

  onRecordsChanged: if (parents.length || navigatedKey !== "") reset()
  onCalendarRuleChanged: if (parents.length || navigatedKey !== "") reset()
  onCapacityChanged: Qt.callLater(function() { if (timeline.parents.length && timeline.detail.bins.length > timeline.capacity) timeline.reset() })

  Keys.onPressed: function(event) {
    if (event.modifiers & (Qt.ControlModifier | Qt.AltModifier | Qt.MetaModifier)) return
    if (event.key === Qt.Key_Left) up()
    else if (event.key === Qt.Key_Right) drill()
    else if (event.key === Qt.Key_Up) movePeriod(-1, false)
    else if (event.key === Qt.Key_Down) movePeriod(1, false)
    else if (event.key === Qt.Key_Home) movePeriod(1, true)
    else if (event.key === Qt.Key_End) movePeriod(-1, true)
    else return
    event.accepted = true
  }

  Item {
    x: timeline.width - timeline.headerWidth
    width: timeline.headerWidth
    height: timeline.headerHeight

    Text {
      anchors.left: parent.left
      anchors.right: buttons.left
      anchors.rightMargin: Style.space(6)
      anchors.verticalCenter: parent.verticalCenter
      textFormat: Text.PlainText
      text: timeline.periodLabel + " · " + timeline.levelLabel
      color: Color.muted
      elide: Text.ElideRight
      font.family: Style.font.family
      font.pixelSize: Style.font.caption
    }

    Row {
      id: buttons
      anchors.right: parent.right
      height: parent.height
      Repeater {
        model: 2
        delegate: Rectangle {
          id: button
          required property int index
          objectName: index === 0 ? "timeline-up" : "timeline-down"
          width: Style.space(26)
          height: parent.height
          enabled: index === 0 ? timeline.canGoUp : timeline.canDrill
          activeFocusOnTab: enabled
          color: "transparent"
          border.width: activeFocus ? 1 : 0
          border.color: Color.accent
          Accessible.role: Accessible.Button
          Accessible.name: index === 0 ? "Show coarser dates" : "Show finer dates for " + timeline.periodLabel
          Accessible.onPressAction: activate()
          function activate() { if (enabled) { if (index === 0) timeline.up(); else timeline.drill() } }
          Keys.onPressed: function(event) {
            if (event.key !== Qt.Key_Return && event.key !== Qt.Key_Enter && event.key !== Qt.Key_Space) return
            activate()
            event.accepted = true
          }
          Text {
            anchors.centerIn: parent
            text: button.index === 0 ? "▴" : "▾"
            color: button.enabled ? Color.bar.text : Color.muted
            opacity: button.enabled ? 1 : 0.45
            font.pixelSize: Style.font.body
          }
          MouseArea {
            anchors.fill: parent
            cursorShape: parent.enabled ? Qt.PointingHandCursor : Qt.ArrowCursor
            onClicked: { button.forceActiveFocus(); button.activate() }
          }
        }
      }
    }
  }

  Repeater {
    model: timeline.detail.bins
    delegate: Item {
      id: mark
      required property int index
      required property var modelData
      readonly property bool inViewport: timeline.viewport.active[index] === true
      x: 0
      y: timeline.axisTop + index * timeline.rowHeight
      width: timeline.width
      height: timeline.rowHeight
      Accessible.role: modelData.count > 0 ? Accessible.Button : Accessible.StaticText
      Accessible.name: timeline.label(modelData, false) + ": " + modelData.count + " media"
      Accessible.onPressAction: timeline.seek(index, 0)

      Text {
        anchors.left: parent.left
        anchors.verticalCenter: parent.verticalCenter
        width: Style.space(43)
        textFormat: Text.PlainText
        text: timeline.label(mark.modelData, true)
        color: Color.muted
        font.family: Style.font.family
        font.pixelSize: Style.font.caption
        elide: Text.ElideRight
      }
      Rectangle {
        id: countMark
        anchors.right: countText.left
        anchors.rightMargin: Style.space(4)
        anchors.verticalCenter: parent.verticalCenter
        width: timeline.detail.maximum ? Style.space(23) * mark.modelData.count / timeline.detail.maximum : 0
        height: Style.space(5)
        color: mark.inViewport ? timeline.lightBlue : timeline.darkBlue
        Repeater {
          model: mark.modelData.level === "undated" ? Math.floor(countMark.width / Style.space(4)) : 0
          delegate: Rectangle {
            required property int index
            x: index * Style.space(4)
            width: 1
            height: countMark.height
            color: Color.bar.background
          }
        }
      }
      Text {
        id: countText
        anchors.right: parent.right
        anchors.verticalCenter: parent.verticalCenter
        width: Style.space(22)
        textFormat: Text.PlainText
        text: mark.modelData.count >= 1000 ? (mark.modelData.count / 1000).toFixed(1) + "k" : mark.modelData.count
        color: mark.inViewport ? timeline.lightBlue : Color.muted
        horizontalAlignment: Text.AlignRight
        font.family: Style.font.family
        font.pixelSize: Style.font.caption
        fontSizeMode: Text.HorizontalFit
        minimumPixelSize: Style.font.caption * 0.8
      }
    }
  }

  Rectangle {
    objectName: "timeline-viewport"
    x: 0
    y: timeline.outlineTop
    width: timeline.width
    height: timeline.outlineHeight
    visible: timeline.viewport.start !== null
    color: "transparent"
    border.width: 1
    border.color: timeline.lightBlue
  }

  MouseArea {
    anchors.fill: parent
    anchors.topMargin: timeline.axisTop
    anchors.bottomMargin: Style.space(6)
    cursorShape: Qt.PointingHandCursor
    onPressed: function(mouse) {
      timeline.forceActiveFocus()
      var y = mouse.y + timeline.axisTop
      var onOutline = timeline.viewport.start !== null && y >= timeline.outlineTop && y <= timeline.outlineTop + timeline.outlineHeight
      timeline.dragOffset = onOutline ? y - timeline.outlineTop : 0
      if (!onOutline) timeline.seekAt(y)
    }
    onPositionChanged: function(mouse) { if (pressed) timeline.seekAt(mouse.y + timeline.axisTop - timeline.dragOffset) }
  }
}
