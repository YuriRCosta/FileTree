import QtQuick
import Quickshell
import Quickshell.Io
import qs.Commons

ShellRoot {
  id: root

  readonly property var loadedService: service.item
  onLoadedServiceChanged: Style.service = loadedService
  readonly property string sourceDir: Quickshell.env("FILEBLADE_SOURCE_DIR")
  property var shellConfig: ({})
  readonly property var barConfig: shellConfig.bar || ({ position: "top" })
  readonly property var bar: BarVisibility {
    service: root.loadedService
    barConfig: root.barConfig
  }

  FileView {
    path: Quickshell.env("HOME") + "/.config/omarchy/shell.json"
    watchChanges: true
    printErrors: false
    onFileChanged: reload()
    onLoaded: {
      try { root.shellConfig = JSON.parse(text()) }
      catch (error) { console.error("FileBlade: invalid shell configuration: " + error) }
    }
  }

  FileView {
    path: root.sourceDir + "/manifest.json"
    printErrors: false
    onLoadFailed: {
      console.error("FileBlade: native runtime manifest is unavailable: " + path)
      Qt.quit()
    }
    onLoaded: {
      try {
        var manifest = JSON.parse(text())
        manifest.__sourceDir = root.sourceDir
        service.setSource(Util.fileUrl(root.sourceDir + "/Service.qml"), {
          shell: root,
          manifest: manifest,
          pluginRegistry: null
        })
      } catch (error) {
        console.error("FileBlade: invalid native runtime manifest: " + error)
        Qt.quit()
      }
    }
  }

  Loader {
    id: service
    onStatusChanged: {
      if (status === Loader.Error) {
        console.error("FileBlade: native browser could not load")
        Qt.quit()
      }
    }
  }

  Loader {
    active: service.status === Loader.Ready && Quickshell.env("FILEBLADE_QUALIFICATION") === "1"
    sourceComponent: Component { Qualification { service: root.loadedService } }
  }

  IpcHandler {
    target: "fileblade.native"
    function status(): string {
      return JSON.stringify({
        loaded: service.status === Loader.Ready,
        sourceDir: root.sourceDir,
        theme: Color.currentThemePath,
        background: String(Color.background),
        foreground: String(Color.foreground),
        accent: String(Color.accent),
        barSize: root.bar.barSize,
        barPosition: root.barConfig.position,
        fontSize: Style.font.body,
        rounding: Style.cornerRadius,
        gapsOut: Style.gapsOut,
        resolvedFontFamily: Style.resolvedFontFamily
      })
    }
  }
}
