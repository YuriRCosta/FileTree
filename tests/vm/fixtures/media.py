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
      return JSON.stringify({ mode: root.mediaMode, active: root.mediaActive, recursive: root.mediaRecursive,
        query: root.mediaQuery, size: root.mediaSizeStep, count: mediaView.count, busy: mediaProvider.busy,
        error: mediaProvider.error, paths: mediaView.rows.slice(0, 1000).map(function(row) { return row.path }),
        selected: controller.selectedPaths, current: mediaView.currentIndex, y: mediaView.contentY,
        root: controller.rootPath, ordinaryQuery: controller.searchQuery, pending: mediaProvider.requestId,
        viewHeight: mediaView.height, cell: mediaView.cell, focused: mediaView.activeFocus,
        firstVisible: mediaView.flickable.indexAt(1, mediaView.contentY + 1) })
    }
    function toggle(): void { root.toggleMedia() }
    function recursive(): void { root.mediaRecursive = !root.mediaRecursive }
    function query(value: string): void { root.mediaQuery = value }
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
