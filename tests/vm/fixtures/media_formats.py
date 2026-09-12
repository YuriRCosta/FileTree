#!/usr/bin/env python3
import argparse
import json
import os
from pathlib import Path
import subprocess


def run(args, env=None):
    result = subprocess.run(args, capture_output=True, text=True, timeout=20, env=env)
    if result.returncode:
        raise RuntimeError(result.stderr[-1500:])
    return result.stdout


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
        ('animated', 'gif'), ('multipage', 'tiff'), ('HEIF', 'heic'),
        ('AVIF', 'avif'), ('JPEG XL', 'jxl'), ('HDR', 'hdr'), ('EXR', 'exr'),
        ('layered', 'psd'), ('vector', 'svg'), ('video H.264', 'mp4'),
        ('video VP9', 'webm'), ('video FFV1', 'mkv'),
    ]
    for family, suffix in formats:
        path = args.root / ('fixture.' + suffix)
        row = {'family': family, 'fixture': str(path), 'recognition': 'extension-covered; live pending',
               'properties': 'not exercised', 'external_open': 'not exercised'}
        try:
            if suffix == 'svg':
                path.write_text('<svg xmlns="http://www.w3.org/2000/svg" width="32" height="16"><rect width="32" height="16" fill="red"/></svg>')
            elif suffix in ('mp4', 'webm', 'mkv'):
                codec = {'mp4': 'libx264', 'webm': 'libvpx-vp9', 'mkv': 'ffv1'}[suffix]
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
                '--key', 'format-matrix', '--width', '64', '--height', '64'], env))
        except (RuntimeError, subprocess.TimeoutExpired, ValueError, OSError) as error:
            row['failure'] = str(error)
        rows.append(row)
        print(json.dumps(row), flush=True)
    rows.append({'family': 'RAW', 'generated': False, 'thumbnail': 'incomplete: authentic camera fixture required',
                 'properties': 'not exercised', 'external_open': 'not exercised'})
    (args.root / 'matrix.json').write_text(json.dumps(rows, indent=2) + '\n')


if __name__ == '__main__':
    main()
