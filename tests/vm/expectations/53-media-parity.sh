#!/usr/bin/env bash
set -euo pipefail
: "${OVM:?set OVM to the harness executable}"
[[ ${OVM_HOME:-} == "$HOME/.local/share/test-omarchy-plugin-b" && ${OVM_SSH_PORT:-} == 2522 ]]
python3 - <<'PY'
import base64
import json
import os
from pathlib import Path
import shlex
import subprocess
import time

ovm_path = os.environ['OVM']
checks = 0


def ovm(*args):
    result = subprocess.run([ovm_path, *map(str, args)], text=True, capture_output=True, timeout=45)
    if result.returncode:
        raise RuntimeError(result.stdout + result.stderr)
    return result.stdout.strip()


def ipc(target, *args):
    return ovm('ssh', shlex.join(['omarchy-shell', target, *map(str, args)]))


def control(*args):
    return ipc('data-goblin.fileblade.control', *args)


def probe(*args):
    return ipc('brindle-media-left', *args)


def state():
    return json.loads(probe('state'))


def wait(predicate):
    deadline = time.monotonic() + 20
    last = None
    while time.monotonic() < deadline:
        last = state()
        if predicate(last):
            return last
        time.sleep(.1)
    raise AssertionError(last)


def check(label, condition, observed):
    global checks
    if not condition:
        raise AssertionError((label, observed))
    checks += 1
    print(json.dumps({'action': label, 'expected': True, 'observed': observed}), flush=True)


def shot(name):
    print(json.dumps({'shot': ovm('shot', 'brindle-' + name)}), flush=True)


def sort(key=None, descending=False):
    value = [] if key is None else [{'key': key, 'desc': descending}]
    probe('sort', base64.b64encode(json.dumps(value).encode()).decode())


control('setRoot', '/tmp/brindle-media/library')
control('openBlade', 'left')
control('focusBlade', 'left')
control('setBladeWidth', 'left', 380)
if not state()['mode']:
    probe('toggle')
probe('query', '')
if state()['recursive']:
    probe('recursive')
wait(lambda s: not s['busy'] and s['count'] == 425)
sort()
probe('size', 2)
probe('choose', 200, 'replace')
current = state()
selected = current['selected']
anchor = current['paths'][current['firstVisible']]
for key in ['name', 'size', 'modified']:
    for descending in [False, True]:
        before = state()
        anchor = before['paths'][before['firstVisible']]
        sort(key, descending)
        time.sleep(.2)
        current = state()
        field = [row[key] for row in current['values']]
        check(f'{key} descending={descending} reorders all 425 files through PaneView', field == sorted(field, reverse=descending) and len(field) == 425, {'first': field[0], 'last': field[-1], 'sorts': current['sorts']})
        check(f'{key} descending={descending} preserves exact selection and scroll anchor', current['selected'] == selected and anchor in current['paths'][current['firstVisible']:current['firstVisible'] + current['columns']], {'selected': current['selected'], 'firstVisible': current['paths'][current['firstVisible']], 'anchor': current['anchor'], 'wanted': anchor, 'busy': current['busy']})
shot('53-sorted')
sort()
probe('choose', 0, 'replace')
ovm('key', 'v')
ovm('key', 'right')
current = state()
check('V then Right extends horizontal selection', current['visual'] and len(current['selected']) == 2, current['selected'])
ovm('key', 'down')
current = state()
check('VISUAL Down extends by the actual grid column count', current['visual'] and len(current['selected']) == 2 + current['columns'], current['selected'])
shot('53-visual-arrows')
ovm('key', 'v')
ovm('key', 'right')
current = state()
check('ordinary Right returns to a single selection', not current['visual'] and len(current['selected']) == 1, current['selected'])
ovm('key', 'shift-right')
current = state()
check('Shift Right keeps range selection without VISUAL', not current['visual'] and len(current['selected']) == 2, current['selected'])
control('setRoot', '/tmp/brindle-media/library/nested')
wait(lambda s: not s['busy'] and s['count'] == 1)
probe('timelineFocus')
ovm('key', 'alt-left')
current = wait(lambda s: s['root'] == '/tmp/brindle-media/library' and not s['busy'] and s['count'] == 425)
check('timeline focus preserves browser Alt Left navigation', current['root'] == '/tmp/brindle-media/library', current['root'])
print(f'{checks} media review checks passed', flush=True)
PY
