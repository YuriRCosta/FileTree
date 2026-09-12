#!/usr/bin/env python3
import argparse
import ctypes
import hashlib
import json
import os
from pathlib import Path
import subprocess


def run(*args, text=False, env=None):
    return subprocess.check_output(args, stderr=subprocess.PIPE, timeout=15, text=text, env=env)


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('root', type=Path)
    parser.add_argument('--binary', required=True)
    args = parser.parse_args()
    if os.environ.get('USER') != 'omarchy':
        raise SystemExit('Run inside harness B')
    args.root.mkdir(parents=True, exist_ok=True)
    cms = ctypes.CDLL('liblcms2.so.2')
    cms.cmsCreate_sRGBProfile.restype = ctypes.c_void_p
    cms.cmsSaveProfileToMem.argtypes = [ctypes.c_void_p, ctypes.c_void_p, ctypes.POINTER(ctypes.c_uint32)]
    cms.cmsCloseProfile.argtypes = [ctypes.c_void_p]
    profile = cms.cmsCreate_sRGBProfile()
    assert profile
    try:
        size = ctypes.c_uint32()
        assert cms.cmsSaveProfileToMem(profile, None, ctypes.byref(size))
        data = ctypes.create_string_buffer(size.value)
        assert cms.cmsSaveProfileToMem(profile, data, ctypes.byref(size))
    finally:
        cms.cmsCloseProfile(profile)
    icc = args.root / 'source.icc'
    icc.write_bytes(data.raw)
    results = []
    for suffix in ('png', 'jpg', 'webp'):
        source = args.root / ('profiled.' + suffix)
        run('magick', '-limit', 'thread', '1', '-size', '100x50', 'gradient:red-blue',
            '-profile', str(icc), str(source))
        original = run('magick', '-limit', 'thread', '1', str(source), 'icc:-')
        result = json.loads(run(args.binary, '_backend', 'thumbnail', '--path', str(source),
            '--key', 'colour-profile', '--width', '64', '--height', '64', text=True,
            env=dict(os.environ, XDG_CACHE_HOME=str(args.root / 'cache'))))
        assert result['ok'] and (result['width'], result['height']) == (64, 32), result
        retained = run('magick', '-limit', 'thread', '1', result['path'], 'icc:-')
        assert retained == original == data.raw, suffix
        results.append({'format': suffix, 'profile_sha256': hashlib.sha256(retained).hexdigest(), 'result': result})
    print(json.dumps({'check': 'native previews retain real RGB ICC profiles after scaling', 'passed': True, 'results': results}))


if __name__ == '__main__':
    main()
