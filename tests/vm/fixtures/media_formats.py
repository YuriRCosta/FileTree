#!/usr/bin/env python3
import argparse
import base64
import json
import os
import hashlib
import struct
import time
from http.server import BaseHTTPRequestHandler, HTTPServer
from threading import Thread
from pathlib import Path
import subprocess


def run(args, env=None, text=True):
    result = subprocess.run(args, capture_output=True, text=text, timeout=20, env=env)
    if result.returncode:
        raise RuntimeError(result.stderr[-1500:])
    return result.stdout


def inspect_png(result, width, height):
    data = Path(result['path']).read_bytes()
    assert data.startswith(b'\x89PNG\r\n\x1a\n'), result
    size = struct.unpack('>II', data[16:24])
    assert 0 < size[0] <= width and 0 < size[1] <= height, size
    assert list(size) == [result['width'], result['height']], result
    pixels = run(['magick', '-limit', 'thread', '1', result['path'], '-depth', '8', 'rgba:-'], text=False)
    assert len(pixels) == size[0] * size[1] * 4
    assert any(pixels[3::4]), 'empty/transparent output'
    return {'size': size, 'rgba_sha256': hashlib.sha256(pixels).hexdigest(),
            'first_pixel': list(pixels[:4]), 'last_pixel': list(pixels[-4:])}


def boundaries(root, binary):
    env = dict(os.environ, XDG_CACHE_HOME=str(root / 'boundary-cache'))
    checks = []

    def render(path, ok=True):
        start = time.monotonic()
        result = json.loads(run([binary, '_backend', 'thumbnail', '--path', str(path),
                                '--key', 'same-key', '--width', '64', '--height', '64'], env))
        assert result.get('ok') is ok, result
        elapsed = time.monotonic() - start
        assert elapsed < 10, elapsed
        checks.append({'file': path.name, 'ok': ok, 'elapsed_seconds': elapsed, 'result': result})
        return result

    for name, data in [('corrupt.mp4', b'not a movie'), ('corrupt.tiff', b'II*\0broken'),
                       ('huge.svg', b'<svg xmlns="http://www.w3.org/2000/svg" width="16385" height="1"/>'),
                       ('pixel-bomb.svg', b'<svg xmlns="http://www.w3.org/2000/svg" width="10000" height="10000"/>')]:
        path = root / name
        path.write_bytes(data)
        render(path, False)
    path = root / 'too-many-bytes.bmp'
    with path.open('wb') as out:
        out.truncate(16 * 1024 * 1024 + 1)
    render(path, False)

    path = root / 'scaled-alpha.tiff'
    run(['magick', '-limit', 'thread', '1', '-size', '1001x501', 'xc:none',
         '-fill', 'red', '-draw', 'rectangle 0,0 499,500', str(path)])
    result = render(path)
    decoded = inspect_png(result, 64, 64)
    assert decoded['size'] == (64, 32), decoded
    assert decoded['first_pixel'] == [255, 0, 0, 255], decoded
    assert decoded['last_pixel'][3] == 0, decoded
    path = root / 'misleading.jpg'
    path.unlink(missing_ok=True)
    path.write_bytes((root / 'fixture.tiff').read_bytes())
    result = render(path)
    inspect_png(result, 64, 64)
    assert render(path)['cached'] is True
    path.unlink()
    path.symlink_to(root / 'fixture.png')
    render(path, False)

    requests = []

    class Handler(BaseHTTPRequestHandler):
        def do_GET(self):
            requests.append(self.path)
            self.send_response(200)
            self.end_headers()
            self.wfile.write((root / 'fixture.png').read_bytes())

        def log_message(self, *args):
            pass

    server = HTTPServer(('127.0.0.1', 0), Handler)
    thread = Thread(target=server.serve_forever, daemon=True)
    thread.start()
    try:
        svg = root / 'no-external.svg'
        for href in [(root / 'fixture.png').as_uri(), 'fixture.png',
                     'http://127.0.0.1:' + str(server.server_port) + '/image.png']:
            svg.write_text('<svg xmlns="http://www.w3.org/2000/svg" width="32" height="16">'
                           '<rect width="16" height="16" fill="red"/>'
                           '<image href="' + href + '" x="16" width="16" height="16"/></svg>')
            result = render(svg)
            pixels = run(['magick', '-limit', 'thread', '1', result['path'], '-depth', '8', 'rgba:-'], text=False)
            assert list(pixels[:4]) == [255, 0, 0, 255], list(pixels[:4])
            assert all(pixels[(y * 32 + x) * 4 + 3] == 0 for y in range(16) for x in range(16, 32))
        assert requests == [], requests
        svg.write_text('<svg xmlns="http://www.w3.org/2000/svg" width="32" height="16">'
                       '<image width="32" height="16" href="data:image/png;base64,'
                       + base64.b64encode((root / 'fixture.png').read_bytes()).decode() + '"/></svg>')
        decoded = inspect_png(render(svg), 64, 64)
        assert decoded['first_pixel'][0] > 200 and decoded['last_pixel'][2] > 200, decoded
    finally:
        server.shutdown()
        server.server_close()
        thread.join()
    return checks


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('root', type=Path)
    parser.add_argument('--binary', required=True)
    args = parser.parse_args()
    if os.environ.get('USER') != 'omarchy':
        raise SystemExit('Run inside the authorized Omarchy guest')
    args.root.mkdir(parents=True, exist_ok=True)
    rows = []
    formats = [
        ('raster', 'png'), ('raster', 'jpg'), ('raster', 'webp'), ('raster', 'bmp'),
        ('icon', 'ico'), ('PNM bitmap', 'pbm'), ('PNM gray', 'pgm'), ('PNM RGB', 'ppm'),
        ('PNM alpha', 'pam'), ('PNM float', 'pfm'),
        ('animated', 'gif'), ('multipage', 'tiff'), ('HEIF', 'heic'),
        ('AVIF', 'avif'), ('JPEG XL', 'jxl'), ('HDR', 'hdr'), ('EXR', 'exr'),
        ('layered', 'psd'), ('vector', 'svg'), ('video H.264', 'mp4'),
        ('video VP9', 'webm'), ('video FFV1', 'mkv'), ('video MPEG-4', 'avi'), ('video Theora', 'ogv'),
        ('video WMV2', 'wmv'), ('video FLV1', 'flv'), ('video HEVC', 'hevc.mp4'),
        ('video AV1', 'av1.mkv'), ('video VP8', 'vp8.webm'),
    ]
    for family, suffix in formats:
        path = args.root / ('fixture.' + suffix)
        row = {'family': family, 'fixture': str(path), 'recognition': 'extension-covered; live pending',
               'properties': 'not exercised', 'external_open': 'not exercised'}
        try:
            if suffix == 'svg':
                path.write_text('<svg xmlns="http://www.w3.org/2000/svg" width="32" height="16"><rect width="32" height="16" fill="red"/></svg>')
            elif suffix in ('mp4', 'webm', 'mkv', 'avi', 'ogv', 'wmv', 'flv', 'hevc.mp4', 'av1.mkv', 'vp8.webm'):
                codec = {'mp4': 'libx264', 'webm': 'libvpx-vp9', 'mkv': 'ffv1', 'avi': 'mpeg4',
                         'ogv': 'libtheora', 'wmv': 'wmv2', 'flv': 'flv',
                         'hevc.mp4': 'libx265', 'av1.mkv': 'libaom-av1', 'vp8.webm': 'libvpx'}[suffix]
                run(['ffmpeg', '-v', 'error', '-y', '-f', 'lavfi', '-i', 'color=red:s=32x16:r=2',
                     '-t', '1', '-c:v', codec, '-threads', '1', str(path)])
            else:
                command = ['magick', '-limit', 'thread', '1', '-size', '32x16', 'gradient:red-blue']
                if suffix in ('gif', 'tiff', 'psd'):
                    command += ['-size', '32x16', 'xc:green']
                run(command + [str(path)])
            row['generated'] = True
            row['bytes'] = path.stat().st_size
            env = dict(os.environ, XDG_CACHE_HOME=str(args.root / 'cache'))
            row['thumbnail'] = json.loads(run([args.binary, '_backend', 'thumbnail', '--path', str(path),
                '--key', 'format-matrix-v2', '--width', '64', '--height', '64'], env))
            row['sha256'] = hashlib.sha256(path.read_bytes()).hexdigest()
            if row['thumbnail'].get('ok'):
                row['decoded'] = inspect_png(row['thumbnail'], 64, 64)
        except (RuntimeError, subprocess.TimeoutExpired, ValueError, OSError) as error:
            row['failure'] = str(error)
        rows.append(row)
        print(json.dumps(row), flush=True)
    checks = boundaries(args.root, args.binary)
    print(json.dumps({'boundary_checks': checks}), flush=True)
    rows.append({'family': 'RAW', 'generated': False, 'thumbnail': 'incomplete: authentic camera fixture required',
                 'properties': 'not exercised', 'external_open': 'not exercised'})
    (args.root / 'checks.json').write_text(json.dumps(checks, indent=2) + '\n')
    (args.root / 'matrix.json').write_text(json.dumps(rows, indent=2) + '\n')
    assert all(row.get('decoded') for row in rows[:-1]), 'Required generated fixture failed decoding'


if __name__ == '__main__':
    main()
