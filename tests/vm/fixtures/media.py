#!/usr/bin/env python3
import argparse
import datetime
import json
import os
from pathlib import Path
import shutil
import struct
import zlib


def png(width, height, color):
    def chunk(kind, data):
        return struct.pack('!I', len(data)) + kind + data + struct.pack('!I', zlib.crc32(kind + data))
    pixels = (b'\0' + bytes(color) * width) * height
    return b'\x89PNG\r\n\x1a\n' + chunk(b'IHDR', struct.pack('!2I5B', width, height, 8, 6, 0, 0, 0)) + chunk(b'IDAT', zlib.compress(pixels)) + chunk(b'IEND', b'')


def prepare(state, plugin):
    state.mkdir(parents=True, exist_ok=True)
    pane = plugin / 'panes/TreePane.qml'
    source = pane.read_text()
    marker = '\n  IpcHandler {\n    target: "brindle-media-'
    if marker in source:
        source = source[:source.index(marker)] + '}\n'
        source = source.removeprefix('import Quickshell.Io\n')
    (state / 'TreePane.qml').write_text(source)
    source = 'import Quickshell.Io\n' + source
    probe = '''
  IpcHandler {
    target: "brindle-media-" + (root.context ? root.context.edge : "left")
    function state(): string {
      return JSON.stringify({ loaded: !!mediaView, providerLoaded: !!mediaProvider, mode: root.mediaMode, active: root.mediaActive, recursive: root.mediaRecursive,
        query: root.mediaQuery, size: root.mediaSizeStep, count: (mediaView ? mediaView.count : 0), busy: (mediaProvider ? mediaProvider.busy : false),
        error: (mediaProvider ? mediaProvider.error : ""), paths: (mediaView ? mediaView.rows : []).slice(0, 1000).map(function(row) { return row.path }),
        selected: controller.selectedPaths, current: (mediaView ? mediaView.currentIndex : -1), y: (mediaView ? mediaView.contentY : 0),
        root: controller.rootPath, ordinaryQuery: controller.searchQuery, pending: (mediaProvider ? mediaProvider.requestId : ""),
        viewHeight: (mediaView ? mediaView.height : 0), cell: (mediaView ? mediaView.cell : 0), focused: (mediaView ? mediaView.activeFocus : false),
        columns: (mediaView ? mediaView.columns : 0), anchor: (mediaView ? mediaView.anchorPath : ""), sorts: controller.treeSort, visual: root.visualMode,
        values: (mediaView ? mediaView.rows : []).slice(0, 1000).map(function(row) { return { name: row.name, size: row.size, modified: row.modified } }),
        ordinary: { step: root.ordinaryDensityStep, density: root.ordinaryDensity,
          count: root.activeList.count, footerCount: footerCount.text, footerDetail: footerDetail.text,
          first: root.activeList.indexAt(1, root.activeList.contentY + 1),
          current: root.activeList.currentIndex,
          rows: !root.mediaActive && !controller.trashMode && !controller.drivesMode ? Array.from({length: Math.min(1000, root.activeList.count)}, function(_, i) { var row = root.activeList.model.get(i); return {path: row.path, depth: row.depth, kind: row.kind} }) : [],
          rowHeight: root.activeList.currentItem ? root.activeList.currentItem.height : 0,
          y: root.activeList.contentY },
        slider: { x: mediaSize.mapToItem(null, 0, 0).x + root.originX(),
          y: mediaSize.mapToItem(null, 0, 0).y + (root.context ? Number(root.context.surfaceOriginY) || 0 : 0),
          width: mediaSize.width, height: mediaSize.height, focused: mediaSize.activeFocus, pressed: mediaSize.pressed },
        timeline: mediaView ? { level: mediaView.timeline.detail.level, count: mediaView.timeline.detail.count,
          bins: mediaView.timeline.detail.bins.map(function(bin) { return { key: bin.key, count: bin.count, level: bin.level } }),
          maximum: mediaView.timeline.detail.maximum, active: mediaView.timeline.viewport.active,
          outlineTop: mediaView.timeline.outlineTop, outlineHeight: mediaView.timeline.outlineHeight,
          axisTop: mediaView.timeline.axisTop, rowHeight: mediaView.timeline.rowHeight,
          width: mediaView.timeline.width, period: mediaView.timeline.periodLabel,
          parentKeys: mediaView.timeline.parents.map(function(bin) { return bin.key }),
          up: mediaView.timeline.canGoUp, down: mediaView.timeline.canDrill,
          x: mediaView.timeline.mapToItem(null, 0, 0).x + root.originX(),
          y: mediaView.timeline.mapToItem(null, 0, 0).y + (root.context ? Number(root.context.surfaceOriginY) || 0 : 0) } : {},
        firstVisible: mediaView ? mediaView.flickable.indexAt(1, mediaView.contentY + 1) : -1 })
    }
    property var independentView: null
    function independent(opened: bool): string {
      if (independentView) { independentView.destroy(); independentView = null }
      if (!opened) return "closed"
      var component = Qt.createComponent(Qt.resolvedUrl("../blades/BladePopout.qml"))
      if (component.status !== Component.Ready) return component.errorString()
      independentView = component.createObject(root, { host: root.context.host, shell: root.context.shell,
        services: root.context.services, screen: root.targetScreen(), moduleId: "files", opened: true,
        width: 300, height: 400, visible: false })
      return independentView ? "created" : component.errorString()
    }
    function sort(value: string): void { filesView.setSorts(JSON.parse(Qt.atob(value))) }
    function scroll(value: int): void { mediaView.contentY = value }
    function timelineFocus(): void { mediaView.timeline.forceActiveFocus() }
    function dates(value: string): void {
      var dates = JSON.parse(Qt.atob(value))
      mediaView.items = mediaProvider.rows.slice(0, dates.length).map(function(row, index) {
        var result = Object.assign({}, row)
        result.date = dates[index]
        result.datePrecision = ""
        return result
      })
    }
    function toggle(): void { root.toggleMedia() }
    function recursive(): void { root.mediaRecursive = !root.mediaRecursive }
    function query(value: string): void { root.mediaQuery = value }
    function density(value: int): void { root.changeDensity(value) }
    function ordinaryChoose(index: int): void { root.selectIndex(root.activeList, root.activeList === treeList, index, "replace"); root.activeList.forceActiveFocus() }
    function size(value: int): void { mediaView.rememberAnchor(); root.mediaSizeStep = value }
    function choose(index: int, mode: string): void { root.selectIndex(mediaView, false, index, mode); mediaView.forceActiveFocus() }
    function action(value: string): void { root.runListAction(value, { modifiers: 0 }, mediaView, false) }
  }
'''
    pane.write_text(source[:source.rfind('}')] + probe + '}\n')


def fixtures(state):
    state.mkdir(parents=True, exist_ok=True)
    folder = state / 'library'
    folder.mkdir(exist_ok=True)
    (folder / 'nested').mkdir(exist_ok=True)
    for i in range(424):
        item = folder / f'image-{i:03d} #%.png'
        item.write_bytes(png(63 + i % 3 * 40, 81, (40 + i % 100, 110, 210, 160 if i % 2 else 255)))
        date = datetime.datetime(2024 + i % 3, 2 if i % 2 else 12, 29 if i % 3 == 0 and i % 2 else 20, tzinfo=datetime.timezone.utc)
        os.utime(item, (date.timestamp(), date.timestamp()))
    (folder / 'notes.txt').write_text('ordinary file\n')
    (folder / 'unsupported.mkv').write_bytes(b'undecodable video fixture')
    (folder / 'nested/deep.png').write_bytes(png(41, 203, (100, 200, 80, 255)))
    (state / 'empty').mkdir(exist_ok=True)
    print(json.dumps({'root': str(folder), 'media': 425, 'recursive_media': 426, 'video_preview': 'intentionally invalid; no decode-support claim'}))


parser = argparse.ArgumentParser()
parser.add_argument('action', choices=['prepare', 'generate', 'restore'])
parser.add_argument('state', type=Path)
parser.add_argument('--plugin', type=Path, default=Path.home() / '.config/omarchy/plugins/data-goblin.fileblade')
args = parser.parse_args()
if args.action == 'prepare':
    prepare(args.state, args.plugin)
    fixtures(args.state)
elif args.action == 'generate':
    fixtures(args.state)
else:
    shutil.copy2(args.state / 'TreePane.qml', args.plugin / 'panes/TreePane.qml')
