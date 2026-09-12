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
if state()['mode']:
    probe('toggle')
current = wait(lambda s: not s['loaded'] and not s['providerLoaded'])
check('ordinary mode has no media subtree', not current['loaded'] and not current['providerLoaded'], current['loaded'])
for path in ['/tmp/brindle-media/library/nested', '/tmp/brindle-media/library']:
    control('setRoot', path)
    control('closeBlade', 'left')
    control('openBlade', 'left')
    current = wait(lambda s: s['root'] == path)
    check('ordinary navigation and reopen create no media subtree', not current['loaded'] and not current['providerLoaded'], path)
shot('56-ordinary-unloaded')
probe('query', '')
if state()['recursive']:
    probe('recursive')
probe('toggle')
current = wait(lambda s: s['loaded'] and not s['busy'] and s['count'] == 425)
check('enter media loads the current folder', current['providerLoaded'], current['count'])
probe('size', 3)
probe('choose', 10, 'replace')
selected = state()['selected']
control('closeBlade', 'left')
closed = json.loads(ipc('data-goblin.fileblade', 'status'))
try:
    probe('state')
    retired = False
except RuntimeError as error:
    retired = 'Target not found' in str(error)
check('closing the blade retires its media view', not closed['open'] and retired, retired)
control('openBlade', 'left')
current = wait(lambda s: not s['busy'] and s['count'] == 425)
check('reopening media preserves size and selected identity', current['size'] == 3 and current['selected'] == selected, current['selected'])
probe('toggle')
current = wait(lambda s: not s['loaded'])
check('leaving media destroys provider and view', not current['providerLoaded'] and not current['pending'], current['providerLoaded'])
control('setRoot', '/tmp/brindle-media/library/nested')
probe('toggle')
current = wait(lambda s: not s['busy'] and s['count'] == 1)
check('reentering media reads the new location without stale rows', current['paths'][0].endswith('/nested/deep.png') and current['size'] == 3, current['paths'])
shot('56-reentered-media')
probe('toggle')
control('setRoot', '/tmp/brindle-media/library')
print(json.dumps({'checks': checks, 'result': 'passed'}))
PY
