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


ovm('ssh', 'python3 ~/.config/omarchy/plugins/data-goblin.fileblade/tests/vm/fixtures/media.py generate /tmp/brindle-media')
control('setRoot', '/tmp/brindle-media/library')
control('openBlade', 'left')
control('focusBlade', 'left')
control('setBladeWidth', 'left', 380)
if not state()['mode']:
    probe('toggle')
probe('query', '')
probe('size', 2)
wait(lambda s: not s['busy'] and s['count'] >= 425)
probe('choose', 0, 'replace')
selected = state()['selected']
control('hideDropWheel')
script = Path('tests/vm/image-gallery-drag.toml').read_text().replace('gallery-pointer', 'brindle-media-wheel').replace('START_X', '60').replace('START_Y', '195').replace('END_X', '900').replace('END_Y', '500')
encoded = base64.b64encode(script.encode()).decode()
ovm('ssh', 'python3 -c ' + shlex.quote("import base64;open('/tmp/brindle-media-wheel.toml','wb').write(base64.b64decode('" + encoded + "'))"))
ovm('ssh', 'setsid ~/.config/omarchy/plugins/data-goblin.fileblade/demos/hold-space-near.sh 900 500 12 3000 >/tmp/brindle-media-space.log 2>&1 </dev/null &')
ovm('ssh', 'setsid democtl record /tmp/brindle-media-wheel.toml --out /tmp --force >/tmp/brindle-media-record.log 2>&1 </dev/null &')
mid = None
deadline = time.monotonic() + 18
while time.monotonic() < deadline:
    wheel = json.loads(ipc('data-goblin.fileblade', 'status'))['dropWheel']
    if wheel['open'] and wheel['dragging']:
        mid = wheel
        break
    time.sleep(.15)
check('media tile starts the existing wheel carrying one selected file', mid is not None and mid['count'] == 1 and mid['fromDrag'], {'wheel': mid, 'selected': selected})
shot('51-drag')
time.sleep(5)
wheel = json.loads(ipc('data-goblin.fileblade', 'status'))['dropWheel']
check('real pointer release on the wheel hub leaves the wheel open', wheel['open'] and not wheel['dragging'] and not wheel['fromDrag'] and wheel['count'] == 1, wheel)
check('drop preserves exact selected identity', state()['selected'] == selected, state()['selected'])
shot('51-dropped')
ovm('key', 'esc')
wheel = json.loads(ipc('data-goblin.fileblade', 'status'))['dropWheel']
check('Escape dismisses the dropped wheel', not wheel['open'], wheel['open'])
probe('toggle')
print(f'{checks} media wheel checks passed', flush=True)
PY
