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


def chrome():
    return json.loads(probe('chrome'))


def ready(label, total=None):
    wait(lambda s: s['ordinary']['footerCount'] == label)
    value = chrome()
    if total is not None:
        assert value['count']['total'] == total, value
    return value


def scope_filter(value):
    probe('filter', base64.b64encode(json.dumps(value).encode()).decode())


control('setRoot', '/tmp/brindle-media/library')
control('openBlade', 'left')
control('focusBlade', 'left')
control('setBladeWidth', 'left', 380)
probe('summaryInTree', 'true')
if state()['mode']:
    probe('toggle')
control('clearSearch')
scope_filter({})
probe('expandRoot', 'true')
ready('427 items', 427)
before_updates = chrome()['countUpdates']
control('setRoot', '/tmp/brindle-media/empty')
ready('0 items', 0)
control('setRoot', '/tmp/brindle-media/library')
ready('427 items', 427)
updates = chrome()['countUpdates'] - before_updates
check('page insertion coalesces count updates instead of scanning each row', updates < 20, updates)
probe('expandRoot', 'false')
value = ready('Count unavailable')
check('collapsed paged root does not claim empty', not value['count']['known'], value['count'])
probe('expandRoot', 'true')
value = ready('427 items', 427)
check('reopening paged root restores listing total', value['count']['loaded'] == 400, value['count'])
probe('expandRoot', 'false')
control('setRoot', '/tmp/brindle-media/s8')
probe('hidden', 'false')
value = ready('2 items', 2)
check('navigation after collapse uses new scope', value['root'].endswith('/s8'), value['count'])
probe('expandRoot', 'false')
probe('hidden', 'true')
probe('expandRoot', 'true')
value = ready('3 items', 3)
check('hidden scope gets its own count', value['count']['known'], value['count'])
scope_filter({'size': {'min': 1000000000}})
wait(lambda s: s['ordinary']['footerCount'].endswith(' matching'))
value = chrome()
check('filtered count does not reuse unfiltered total', value['count']['total'] < 3, value)
probe('expandRoot', 'false')
ready('Count unavailable')
check('collapsed filtered scope is explicitly unavailable', not chrome()['count']['known'], chrome()['label'])
scope_filter({})
control('setRoot', '/tmp/brindle-media/empty')
value = ready('0 items', 0)
check('actually empty loaded folder is zero', value['count']['known'], value['count'])
probe('expandRoot', 'false')
check('collapsed empty root invalidates count without child removal', not ready('Count unavailable')['count']['known'], chrome())
probe('expandRoot', 'true')
ready('0 items', 0)
scope_filter({'size': {'min': 1000000000}})
ready('0 matching', 0)
probe('expandRoot', 'false')
check('collapsed filtered-empty root invalidates count', not ready('Count unavailable')['count']['known'], chrome())
scope_filter({})
probe('expandRoot', 'true')
ready('0 items', 0)
probe('failRefresh')
check('failed background reconciliation invalidates successful count', not ready('Count unavailable')['count']['known'], chrome())
shot('57-count-empty')
control('setRoot', '/tmp/brindle-media/library')
ready('427 items', 427)
probe('ordinaryChoose', 180)
selected = state()['selected']
for large, small, offset in [(4, 0, 30), (0, 4, 20)]:
    probe('density', large)
    wait(lambda s: s['ordinary']['step'] == large)
    probe('clip', 160, offset)
    before = chrome()
    probe('density', small)
    time.sleep(.2)
    after = chrome()
    check('partial ordinary row survives density change', before['path'] == after['path'] and after['selected'] == selected and abs(after['offset'] / after['height'] - before['offset'] / before['height']) < .06, {'before': before, 'after': after})
probe('density', 4)
probe('clip', 160, 30)
before = chrome()
probe('densityBurst')
time.sleep(.2)
after = chrome()
check('rapid density changes preserve one anchor', before['path'] == after['path'] and after['step'] == 0, {'before': before, 'after': after})
control('setRoot', '/tmp/brindle-media/s8')
probe('deepSearch')
wait(lambda s: s['ordinary']['count'] >= 40)
probe('loadSearch')
wait(lambda s: s['ordinary']['count'] >= 190)
probe('ordinaryChoose', 180)
for large, small, offset in [(4, 0, 42), (0, 4, 30)]:
    probe('density', large)
    probe('clip', 160, offset)
    before = chrome()
    assert before['height'] == (50 if large == 4 else 34), before
    probe('density', small)
    time.sleep(.2)
    after = chrome()
    check('partial path-visible search row survives density change', before['path'] == after['path'] and abs(after['offset'] / after['height'] - before['offset'] / before['height']) < .06, {'before': before, 'after': after})
shot('57-search-anchor')
control('clearSearch')
control('setRoot', '/tmp/brindle-media/library')
ready('427 items', 427)
probe('density', 2)
time.sleep(.2)
probe('ordinaryChoose', 3)
probe('bindMinus', 'true')
before = state()
ovm('key', 'minus')
after = wait(lambda s: s['selected'] != before['selected'])
check('configured minus binding precedes density fallback', after['ordinary']['step'] == 2, after['selected'])
probe('bindMinus', 'false')
ovm('key', 'minus')
after = wait(lambda s: s['ordinary']['step'] == 1)
check('unbound minus still changes density', after['ordinary']['step'] == 1, after['ordinary']['step'])
probe('summaryInTree', 'false')
print(f'{checks} S8 chrome checks passed', flush=True)
PY
