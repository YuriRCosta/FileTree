#!/usr/bin/env bash
set -euo pipefail
: "${OVM:?set OVM to the harness executable}"
[[ -n ${OVM_HOME:-} && -n ${OVM_SSH_PORT:-} ]]
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


control('clearSearch')
control('setRoot', '/tmp/brindle-media/library')
control('openBlade', 'left')
control('focusBlade', 'left')
control('setBladeWidth', 'left', 380)
if state()['mode']:
    probe('toggle')
probe('density', 2)
for key, chord in [('-', 'minus'), ('+', 'shift-equal')]:
    probe('clip', 0, 0)
    probe('ordinaryChoose', 3)
    selected = state()['selected']
    point = json.loads(probe('ordinaryPoint'))
    before = json.loads(probe('chrome'))['wheelActivations']
    control('hideDropWheel')
    ovm('mouse', 'move', round(point['x']), round(point['y']))
    script = Path('tests/vm/image-gallery-drag.toml').read_text().replace('gallery-pointer', 'brindle-s8-wheel').replace('START_X', str(round(point['x']))).replace('START_Y', str(round(point['y']))).replace('END_X', '900').replace('END_Y', '500')
    encoded = base64.b64encode(script.encode()).decode()
    ovm('ssh', 'python3 -c ' + shlex.quote("import base64;open('/tmp/brindle-s8-wheel.toml','wb').write(base64.b64decode('" + encoded + "'))"))
    ovm('ssh', 'setsid ~/.config/omarchy/plugins/data-goblin.fileblade/demos/hold-space-near.sh 900 500 12 3000 >/tmp/brindle-s8-space.log 2>&1 </dev/null &')
    ovm('ssh', 'setsid democtl record /tmp/brindle-s8-wheel.toml --out /tmp --force >/tmp/brindle-s8-record.log 2>&1 </dev/null &')
    deadline = time.monotonic() + 18
    while time.monotonic() < deadline:
        wheel = json.loads(ipc('data-goblin.fileblade', 'status'))['dropWheel']
        if wheel['open'] and wheel['dragging'] and not wheel['loading']:
            break
        time.sleep(.1)
    assert wheel['open'] and wheel['dragging'], wheel
    probe('wheelKey', key)
    ovm('key', chord)
    time.sleep(.25)
    receipt = json.loads(probe('chrome'))
    check('active ordinary-drag wheel key activates once without resizing', receipt['wheelActivations'] == before + 1 and receipt['step'] == 2 and receipt['wheelPaths'] == selected, {'key': key, 'receipt': receipt})
    time.sleep(5)
    check('pointer release does not repeat the wheel action', json.loads(probe('chrome'))['wheelActivations'] == before + 1, key)
    shot('58-wheel-key-' + ('minus' if key == '-' else 'plus'))
control('focusBlade', 'left')
probe('ordinaryChoose', 3)
ovm('key', 'minus')
wait(lambda s: s['ordinary']['step'] == 1)
check('inactive wheel leaves minus to density', state()['ordinary']['step'] == 1, state()['ordinary']['step'])
print(f'{checks} S8 wheel checks passed', flush=True)
PY
