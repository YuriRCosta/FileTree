#!/usr/bin/env python3
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import threading


def main():
    if os.environ.get('USER') != 'omarchy':
        raise SystemExit('Run inside harness B')
    evidence = len(sys.argv) == 2 and sys.argv[1] == 'evidence'
    if len(sys.argv) > 1 and not evidence:
        raise SystemExit('usage: media_icons.py [evidence]')
    plugin = Path(__file__).resolve().parents[3]
    work_root = Path(os.environ.get('MEDIA_ICONS_WORK_ROOT', '/tmp'))
    work_root.mkdir(parents=True, exist_ok=True)
    stage = Path(tempfile.mkdtemp(prefix='fileblade-media-icons-', dir=work_root))
    source = stage / 'probe.qml'
    commons = stage / 'Commons'
    wide_asset = stage / 'wide.svg'
    process = None
    try:
        for name in ('assets', 'lib', 'panes', 'ui'):
            shutil.copytree(plugin / name, stage / name)
        shutil.copytree(plugin / 'tests/imports/qs/Commons', commons)
        color = commons / 'Color.qml'
        color.write_text(color.read_text().rstrip()[:-1] + '\n  property var menu: ({ text: "white", selectedText: "white", selectedBorder: "blue", selectedBackground: "black" })\n}\n')
        environment = os.environ.copy()
        if evidence:
            data = stage / 'data'
            apps = data / 'applications'
            apps.mkdir(parents=True)
            (stage / 'bad-desktop.svg').write_text('broken SVG')
            (apps / 'brindle-icon-failures.desktop').write_text('[Desktop Entry]\nType=Application\nName=Icon failure fixture\nExec=true\nIcon=' + str(stage / 'bad-desktop.svg') + '\n')
            for mark in ('pane-horizontal', 'tmux'):
                (stage / 'assets/marks' / (mark + '.svg')).write_text('broken SVG')
            (stage / 'bad-source.svg').write_text('broken SVG')
            environment['XDG_DATA_HOME'] = str(data)
            wide_asset.write_text('<svg xmlns="http://www.w3.org/2000/svg" width="256" height="128"><rect width="256" height="128" fill="#7aa2f7"/></svg>\n')
        qml = Path(__file__).with_suffix('.qml').read_text().replace('../../../', './')
        qml = qml.replace('property string pluginRoot: ""', 'property string pluginRoot: ' + json.dumps(str(stage)))
        qml = qml.replace('property bool evidenceMode: false', 'property bool evidenceMode: ' + str(evidence).lower())
        qml = qml.replace('property string wideAsset: ""', 'property string wideAsset: ' + json.dumps(wide_asset.as_uri() if evidence else ''))
        source.write_text(qml)
        if evidence:
            print('MEDIA_ICONS_SOURCE ' + str(source), flush=True)
            process = subprocess.Popen(['quickshell', '-p', str(source)], text=True,
                                        stdout=subprocess.PIPE, stderr=subprocess.STDOUT, env=environment)
            lines = []
            deadline = threading.Timer(180, process.kill)
            deadline.start()
            try:
                for line in process.stdout:
                    lines.append(line)
                    print(line, end='', flush=True)
                returncode = process.wait(timeout=5)
            finally:
                deadline.cancel()
            log = ''.join(lines)
        else:
            result = subprocess.run(['quickshell', '-p', str(source)], text=True, capture_output=True, timeout=15)
            print(result.stdout, end='')
            print(result.stderr, end='')
            log = result.stdout + result.stderr
            returncode = result.returncode
        assert returncode == 0 and 'MEDIA_ICONS_FAIL' not in log, returncode
        records = [json.loads(line.split('MEDIA_ICONS_PASS ', 1)[1]) for line in log.splitlines() if 'MEDIA_ICONS_PASS ' in line]
        assert len(records) == 1 and len(records[0]) == 3, records
        if evidence:
            evidence_records = [json.loads(line.split('MEDIA_ICONS_EVIDENCE_DONE ', 1)[1])
                                for line in log.splitlines() if 'MEDIA_ICONS_EVIDENCE_DONE ' in line]
            assert len(evidence_records) == 1 and len(evidence_records[0]) == 6, evidence_records
        print('MEDIA_ICONS_FIXTURE_OK', flush=True)
    finally:
        if process is not None and process.poll() is None:
            process.kill()
            try:
                process.wait(timeout=5)
            except subprocess.TimeoutExpired:
                process.kill()
                process.wait(timeout=5)
        shutil.rmtree(stage, ignore_errors=True)


if __name__ == '__main__':
    main()
