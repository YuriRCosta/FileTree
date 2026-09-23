import QtQuick
import Quickshell

Item {
  id: screens

  required property var host

  visible: false

  Variants {
    model: Quickshell.screens

    delegate: Component {
      BladeSurface {
        required property var modelData
        host: screens.host
        edge: "left"
        screen: modelData
      }
    }
  }

  Variants {
    model: Quickshell.screens

    delegate: Component {
      BladeSurface {
        required property var modelData
        host: screens.host
        edge: "right"
        screen: modelData
      }
    }
  }

  Variants {
    model: Quickshell.screens

    delegate: Component {
      BladeDragOverlay {
        required property var modelData
        host: screens.host
        screen: modelData
      }
    }
  }
}
