import QtQuick
import qs.Commons

Item {
  id: favoritesSection

  required property var controller
  required property var pane
  readonly property int rowHeight: Style.space(30)
  readonly property int visibleRows: Math.min(6, controller.favoritesModel.count)
  height: visibleRows * rowHeight
  visible: height > 0

  ListView {
    id: favoritesList
    anchors.fill: parent
    reuseItems: true
    clip: true
    boundsBehavior: Flickable.StopAtBounds
    model: controller.favoritesModel
    currentIndex: -1

    delegate: BrowserRow {
      controller: favoritesSection.controller
      pane: favoritesSection.pane
      treeMode: true
      favoriteMode: true
      ownerView: favoritesList
    }
  }
}
