#!/usr/bin/env python3
import json
import os
from pathlib import Path
import shutil
import subprocess


def main():
    if os.environ.get('USER') != 'omarchy':
        raise SystemExit('Run inside harness B')
    plugin = Path(__file__).resolve().parents[3]
    source = plugin / '.brindle-media-icons.qml'
    commons = plugin / 'Commons'
    assert not source.exists() and not commons.exists()
    try:
        shutil.copytree(plugin / 'tests/imports/qs/Commons', commons)
        color = commons / 'Color.qml'
        color.write_text(color.read_text().rstrip()[:-1] + '\n  property var menu: ({ text: "white", selectedText: "white", selectedBorder: "blue", selectedBackground: "black" })\n}\n')
        source.write_text(Path(__file__).with_suffix('.qml').read_text().replace('../../../', './')
                          .replace('property string pluginRoot: ""', 'property string pluginRoot: ' + json.dumps(str(plugin))))
        result = subprocess.run(['quickshell', '-p', str(source)], text=True, capture_output=True, timeout=15)
        print(result.stdout, end='')
        print(result.stderr, end='')
        log = result.stdout + result.stderr
        assert result.returncode == 0 and 'MEDIA_ICONS_FAIL' not in log, result.returncode
        records = [json.loads(line.split('MEDIA_ICONS_PASS ', 1)[1]) for line in log.splitlines() if 'MEDIA_ICONS_PASS ' in line]
        assert len(records) == 1 and len(records[0]) == 3, records
    finally:
        source.unlink(missing_ok=True)
        if commons.exists():
            shutil.rmtree(commons)


if __name__ == '__main__':
    main()
