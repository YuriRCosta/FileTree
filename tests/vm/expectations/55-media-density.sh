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


control('setRoot', '/tmp/brindle-media/library')
control('openBlade', 'left')
control('focusBlade', 'left')
control('setBladeWidth', 'left', 380)
if state()['mode']:
    probe('toggle')
current = wait(lambda s: s['ordinary']['count'] > 200 and s['ordinary']['footerCount'] == '427 items')
probe('ordinaryChoose', 200)
selected = state()['selected']
for step, factor, height in [(0, .85, 26), (1, .925, 28), (2, 1, 30), (3, 1.075, 32), (4, 1.15, 35), (0, .85, 26), (4, 1.15, 35)]:
    before = state()['ordinary']
    anchor = before['rows'][before['first']]['path']
    slider = state()['slider']
    ovm('mouse', 'click', round(slider['x'] + 12), round(slider['y'] + slider['height'] / 2))
    ovm('key', 'home')
    for i in range(step):
        ovm('key', 'right')
    current = wait(lambda s: s['ordinary']['step'] == step and s['ordinary']['rowHeight'] == height)
    check('ordinary step ' + str(step) + ' uses agreed row/icon density', current['ordinary']['density'] == factor and current['slider']['focused'], {'step': step, 'factor': current['ordinary']['density'], 'height': current['ordinary']['rowHeight']})
    check('density preserves selection', current['selected'] == selected, current['selected'])
probe('density', 0)
time.sleep(.2)
before = state()['ordinary']
anchor = before['rows'][before['first']]['path']
probe('density', 4)
time.sleep(.2)
current = state()
check('largest density jump keeps the first-visible path', current['ordinary']['rows'][current['ordinary']['first']]['path'] == anchor, {'wanted': anchor, 'observed': current['ordinary']['rows'][current['ordinary']['first']]['path']})
check('ordinary footer counts the current folder without its root row', current['ordinary']['footerCount'] == '427 items' and current['ordinary']['footerDetail'].startswith('1 selected'), {'count': current['ordinary']['footerCount'], 'detail': current['ordinary']['footerDetail']})
shot('55-density-xl')
check('production popout creates an independent Files context', probe('independent', 'true') == 'created', 'created')
time.sleep(.5)
ipc('brindle-media-popout', 'density', 1)
check('independent popout does not change blade density', json.loads(ipc('brindle-media-popout', 'state'))['ordinary']['step'] == 1 and state()['ordinary']['step'] == 4, {'blade': state()['ordinary']['step'], 'popout': json.loads(ipc('brindle-media-popout', 'state'))['ordinary']['step']})
probe('independent', 'false')
control('openBlade', 'left')
control('focusBlade', 'left')
probe('toggle')
probe('size', 1)
wait(lambda s: not s['busy'] and s['count'] == 425)
probe('toggle')
check('media and ordinary sizes remain independent', state()['ordinary']['step'] == 4 and state()['size'] == 1, {'ordinary': state()['ordinary']['step'], 'media': state()['size']})
control('setBladeWidth', 'left', 280)
current = wait(lambda s: s['slider']['x'] < 160)
check('narrow ordinary footer keeps counts and density controls', current['ordinary']['footerCount'] == '427 items' and current['slider']['width'] == 156, {'slider': current['slider'], 'count': current['ordinary']['footerCount']})
time.sleep(.4)
shot('55-density-narrow')
time.sleep(2)
subprocess.run(['tests/vm/stop-shell'], check=True, timeout=40)
ovm('restart-shell')
restart_status = json.loads(ipc('data-goblin.fileblade', 'status'))
assert restart_status['bladeModules'] and all('Unknown' not in item for item in restart_status['bladeModules']), restart_status
print(json.dumps({'restartBladeModules': restart_status['bladeModules']}), flush=True)
control('openBlade', 'left')
shot('55-density-restart')
control('focusBlade', 'left')
current = wait(lambda s: s['ordinary']['count'] > 200 and s['ordinary']['footerCount'] == '427 items')
check('density and media size survive shell restart independently', not current['mode'] and current['ordinary']['step'] == 4 and current['size'] == 1, {'ordinary': current['ordinary']['step'], 'media': current['size'], 'mode': current['mode']})
probe('ordinaryChoose', 200)
ovm('key', 'minus')
current = wait(lambda s: s['ordinary']['step'] == 3)
check('ordinary list minus shortcut changes density without selection loss', current['selected'] == selected, current['selected'])
control('setBladeWidth', 'left', 380)
print(f'{checks} ordinary density checks passed', flush=True)
PY
