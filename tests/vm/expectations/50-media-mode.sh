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
slots = [{'id': 'files', 'fraction': .75, 'modules': [{'module': 'files'}]},
         {'id': 'properties', 'fraction': .25, 'modules': [{'module': 'properties'}]}]
control('setBladeSlots', 'left', 'base64:' + base64.b64encode(json.dumps(slots).encode()).decode())
control('setRoot', '/tmp/brindle-media/library')
control('openBlade', 'left')
control('focusBlade', 'left')
control('setBladeWidth', 'left', 380)
current = state()
if current['mode']:
    probe('toggle')
control('select', '/tmp/brindle-media/library/notes.txt')
probe('toggle')
if state()['recursive']:
    probe('recursive')
probe('query', '')
probe('size', 2)
current = wait(lambda s: not s['busy'] and s['count'] == 425)
check('current-folder images and video, no descendant or text file', all(not p.endswith(('notes.txt', 'deep.png')) for p in current['paths']), current['count'])
check('hidden ordinary selection removed', not current['selected'], current['selected'])
shot('50-current-folder')
ovm('mouse', 'click', 60, 195)
ovm('key', 'shift-right')
current = wait(lambda s: len(s['selected']) == 2)
check('pointer and Shift+Right range select', len(current['selected']) == 2, current['selected'])
shot('50-selection-properties')
ovm('key', 'ctrl-a')
current = wait(lambda s: len(s['selected']) == 425)
check('Ctrl+A selects only matching media across pages', len(current['selected']) == 425, len(current['selected']))
ovm('mouse', 'click', 90, 80)
ovm('key', 'ctrl-a')
ovm('type', 'type:video')
current = wait(lambda s: s['count'] == 1)
check('real search edits media query without changing ordinary query', current['query'] == 'type:video' and current['ordinaryQuery'] == '', current['query'])
check('filtered-out images no longer targeted', all(path.endswith('unsupported.mkv') for path in current['selected']), current['selected'])
shot('50-video-visible')
ovm('key', 'esc')
wait(lambda s: s['count'] == 425)
probe('choose', 200, 'replace')
current = state()
selected = current['selected']
anchor = current['paths'][current['firstVisible']]
probe('size', 4)
current = wait(lambda s: s['size'] == 4 and s['paths'][s['firstVisible']] == anchor)
check('resize retains path and viewport anchor', current['selected'] == selected, {'selected': current['selected'], 'firstVisible': current['firstVisible']})
probe('recursive')
current = wait(lambda s: not s['busy'] and s['count'] == 426)
check('explicit recursion adds descendant and retains selection', current['selected'] == selected and any(p.endswith('deep.png') for p in current['paths']), {'count': current['count'], 'selected': current['selected']})
probe('choose', 200, 'replace')
ovm('key', 'm')
menu = json.loads(ovm('ipc', 'data-goblin.fileblade', 'status'))
check('existing action menu names exact path', menu['actionMenuOpen'] and menu['actionMenuPath'] == state()['selected'][0], menu['actionMenuPath'])
shot('50-actions-menu')
ovm('key', 'esc')
probe('toggle')
current = state()
check('ordinary mode restores location and clears media work', not current['mode'] and not current['pending'] and current['root'] == '/tmp/brindle-media/library', {'root': current['root'], 'mode': current['mode'], 'pending': current['pending']})
shot('50-ordinary-restored')
control('search', 'image')
probe('toggle')
probe('query', 'type:video')
wait(lambda s: not s['busy'] and s['count'] == 1)
probe('toggle')
current = state()
check('ordinary query survives media query edits', current['ordinaryQuery'] == 'image', current['ordinaryQuery'])
probe('toggle')
current = state()
check('media query survives a round trip through ordinary mode', current['query'] == 'type:video', current['query'])
probe('toggle')
control('clearSearch')
probe('toggle')
probe('query', 'no-matching-media')
current = wait(lambda s: not s['busy'] and s['count'] == 0)
check('no matches clears selection', not current['selected'], current['selected'])
shot('50-no-matches')
probe('query', '')
control('setRoot', '/tmp/brindle-media/empty')
current = wait(lambda s: s['root'].endswith('/empty') and not s['busy'])
check('empty folder is not an error', current['count'] == 0 and not current['error'], {'count': current['count'], 'error': current['error']})
shot('50-empty-folder')
probe('toggle')
print(f'{checks} media checks passed', flush=True)
PY
