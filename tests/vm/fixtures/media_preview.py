#!/usr/bin/env python3
import argparse
import base64
import json
import os
import signal
from pathlib import Path
import subprocess
import struct
import time
from urllib.parse import unquote, urlsplit
from media_formats import inspect_png


def run(*args):
    return subprocess.check_output(args, text=True, stderr=subprocess.PIPE, timeout=20).strip()


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('action', choices=['prepare', 'check'])
    parser.add_argument('root', type=Path)
    parser.add_argument('--matrix', type=Path)
    parser.add_argument('--external-open', action='store_true')
    args = parser.parse_args()
    if os.environ.get('USER') != 'omarchy':
        raise SystemExit('Run inside harness B')
    plugin = Path.home() / '.config/omarchy/plugins/yuricosta.filetree'
    if args.action == 'prepare':
        args.root.mkdir(parents=True, exist_ok=True)
        run('magick', '-limit', 'thread', '1', '-size', '64x32', 'xc:#ff0000a0', str(args.root / 'image.png'))
        for suffix in ('jpg', 'webp'):
            run('magick', '-limit', 'thread', '1', str(args.root / 'image.png'), str(args.root / ('image.' + suffix)))
        (args.root / 'image.svg').write_text('<svg xmlns="http://www.w3.org/2000/svg" width="64" height="32"><rect width="64" height="32" fill="blue"/></svg>')
        run('ffmpeg', '-v', 'error', '-y', '-f', 'lavfi', '-i', 'color=green:s=64x32:r=2',
            '-t', '1', '-c:v', 'libx264', '-threads', '1', str(args.root / 'video.mp4'))
        (args.root / 'corrupt.mp4').write_bytes(b'not a movie')
        link = args.root / 'linked.png'
        link.unlink(missing_ok=True)
        link.symlink_to(args.root / 'image.png')
        pane = plugin / 'panes/PropertiesPane.qml'
        source = pane.read_text()
        (args.root / 'PropertiesPane.qml').write_text(source)
        probe = '''
  IpcHandler {
    target: "brindle-properties"
    function capabilities(value: string): void { root.mediaLocationDescriptor = JSON.parse(value) }
    function state(): string {
      return JSON.stringify({ entry: root.entry, image: root.isImage, media: root.imageDecodable,
        allowed: root.imagePreviewAllowed, key: root.imagePreviewKey, source: root.previewSource,
        loading: root.imageLoading, error: root.imageError, message: root.imagePreviewMessage(),
        text: root.isText, generation: root.thumbnailGeneration, request: root.thumbnailRequestId,
        card: filePreview.image })
    }
  }
'''
        pane.write_text('import Quickshell.Io\n' + source.rstrip()[:-1] + probe + '}\n')
        return

    def control(*values):
        return run('omarchy-shell', 'yuricosta.filetree.control', *values)

    def state():
        return json.loads(run('omarchy-shell', 'brindle-properties', 'state'))

    def wait(predicate):
        until = time.monotonic() + 15
        while time.monotonic() < until:
            value = state()
            if predicate(value):
                return value
            time.sleep(.05)
        raise AssertionError(value)

    slots = [{'id': 'files', 'fraction': .4, 'modules': [{'module': 'files'}]},
             {'id': 'properties', 'fraction': .6, 'modules': [{'module': 'properties'}]}]
    control('setBladeSlots', 'left', 'base64:' + base64.b64encode(json.dumps(slots).encode()).decode())
    control('setRoot', str(args.root))
    control('openBlade', 'left')
    control('setBladeWidth', 'left', '600')
    values = []
    if args.matrix:
        matrix = json.loads(args.matrix.read_text())
        for row in matrix:
            if not row.get('generated'):
                continue
            path = row['fixture']
            control('select', path)
            value = wait(lambda s: s.get('entry', {}).get('path') == path and not s['loading'])
            assert value['media'] and value['allowed'] and value['card'] and not value['text'], value
            assert value['source'] and not value['error'], value
            preview = unquote(urlsplit(value['source']).path)
            width, height = struct.unpack('>II', Path(preview).read_bytes()[16:24])
            pixels = inspect_png({'path': preview, 'width': width, 'height': height}, 1024, 1024)
            row['properties'] = {'passed': True, 'source': value['source'], 'decoded': pixels}
            row['recognition'] = {'mime': value['entry']['mime'], 'media': value['media']}
            if args.external_open:
                desktop = 'mpv.desktop' if row['family'].startswith('video') else 'imv.desktop'
                before = {c['address'] for c in json.loads(run('hyprctl', '-j', 'clients'))}
                control('openWithPath', path, desktop)
                until = time.monotonic() + 17
                while time.monotonic() < until:
                    launch = json.loads(run('omarchy-shell', 'yuricosta.filetree', 'status'))
                    if not launch['launchBusy'] and launch['lastLaunchedPath'] == path:
                        break
                    time.sleep(.1)
                assert not launch['launchBusy'] and launch['lastLaunchedPath'] == path, launch
                windows = [c for c in json.loads(run('hyprctl', '-j', 'clients'))
                           if c['address'] not in before and c['class'] in ('imv', 'mpv')]
                row['external_open'] = {'desktop_id': desktop, 'status': launch['launchStatus'],
                    'error': launch['launchError'], 'windows': [{k: c[k] for k in ('address', 'class', 'title', 'pid')} for c in windows],
                    'pixel_inspection': 'not measured; window and dispatch only'}
                for window in windows:
                    process = Path('/proc') / str(window['pid'])
                    if process.exists() and os.fsencode(path) in (process / 'cmdline').read_bytes().split(b'\0'):
                        try:
                            os.kill(window['pid'], signal.SIGTERM)
                        except ProcessLookupError:
                            pass
                control('openBlade', 'left')
            print(json.dumps({'family': row['family'], 'fixture': path, 'properties': row['properties'],
                              'external_open': row['external_open']}), flush=True)
        (args.root / 'properties-matrix.json').write_text(json.dumps(matrix, indent=2) + '\n')
    for name in ('image.png', 'image.jpg', 'image.webp', 'image.svg', 'video.mp4'):
        path = str(args.root / name)
        control('select', path)
        value = wait(lambda s: s.get('entry', {}).get('path') == path and not s['loading'] and bool(s['source']))
        assert value['allowed'] and value['media'] and value['card'] and not value['text'], value
        entry = value['entry']
        assert value['key'] == '\n'.join([path, entry['stat_fingerprint'], entry['mime'], str(entry['size'])]), value
        assert '?v=' in value['source'] and value['source'].startswith('file://'), value
        values.append(value)
    run('omarchy-shell', 'brindle-properties', 'capabilities', '{"capabilities":[]}')
    value = wait(lambda s: not s['allowed'] and not s['source'] and not s['request'])
    assert value['card'] and not value['text'] and value['message'] == 'Preview unavailable for this location.', value
    values.append(value)
    run('omarchy-shell', 'brindle-properties', 'capabilities', 'null')
    wait(lambda s: s['allowed'] and bool(s['source']))
    for name in ('corrupt.mp4', 'linked.png'):
        path = str(args.root / name)
        control('select', path)
        value = wait(lambda s: s.get('entry', {}).get('path') == path and not s['loading'])
        assert value['card'] and not value['source'] and not value['text'], value
        assert value['error'] if name == 'corrupt.mp4' else value['message'].startswith('Linked images'), value
        values.append(value)
    control('select', str(args.root / 'video.mp4'))
    wait(lambda s: s['allowed'] and bool(s['source']))
    print(json.dumps({'check': 'R51 Properties image compatibility, posters, capability refusal and visible failures',
                      'passed': True, 'states': values}), flush=True)


if __name__ == '__main__':
    main()
