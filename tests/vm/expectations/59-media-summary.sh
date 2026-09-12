#!/usr/bin/env bash
set -euo pipefail
: "${OVM:?set OVM to the harness executable}"
[[ ${OVM_HOME:-} == "$HOME/.local/share/test-omarchy-plugin-b" && ${OVM_SSH_PORT:-} == 2522 ]]
python3 - <<'PY'
import json
import os
import shlex
import subprocess
import time

checks = 0

def ovm(*args):
    return subprocess.check_output([os.environ['OVM'], *map(str, args)], text=True, timeout=45).strip()


def ipc(target, *args):
    return ovm('ssh', shlex.join(['omarchy-shell', target, *map(str, args)]))


def probe(*args):
    return ipc('brindle-media-left', *args)


def control(*args):
    return ipc('data-goblin.fileblade.control', *args)


def state():
    return json.loads(probe('summary'))


def wait(predicate):
    deadline = time.monotonic() + 20
    while time.monotonic() < deadline:
        current = state()
        if predicate(current):
            return current
        time.sleep(.1)
    raise AssertionError(current)


def check(label, condition, observed):
    global checks
    assert condition, (label, observed)
    checks += 1
    print(json.dumps({'action': label, 'expected': True, 'observed': observed}), flush=True)


def shot(name):
    print(json.dumps({'shot': ovm('shot', 'brindle-59-' + name)}), flush=True)


folder = '/tmp/brindle-media/context-repo'
ovm('ssh', 'mkdir -p ' + folder + '/nested && printf "initial\\n" > ' + folder + '/README && printf "nested\\n" > ' + folder + '/nested/deep.txt && git -C ' + folder + ' init -q -b main && git -C ' + folder + ' add README nested && git -C ' + folder + ' -c user.name="Media fixture" -c user.email=media@example.invalid commit -qm fixture --allow-empty && printf "modified\\n" >> ' + folder + '/README')
control('clearSearch')
probe('hidden', 'false')
control('openBlade', 'left')
control('focusBlade', 'left')
control('setBladeWidth', 'left', 380)
if state()['mode']:
    probe('toggle')
probe('summaryInTree', 'false')
control('setRoot', folder)
s = wait(lambda s: s['root'] == folder and 'M1' in s['badge'])
check('default summary is between search and navigation with repository identity/status', s['above'] and s['searchBottom'] <= s['y'] < s['navigationY'] and 'main' in s['detail'] and s['label'] == 'context-repo', s)
probe('ordinaryChoose', 0)
s = state()
check('first navigation target excludes the informational root', s['current'] == 1 and folder not in s['selected'] and s['rootHeight'] == 0, s)
selected = s['selected']
ovm('mouse', 'click', 110, round(s['y'] + s['height'] / 2 + s['originY']))
check('summary click is inert', state()['selected'] == selected and state()['expanded'], state())
probe('treeAction', 'select-all')
s = state()
check('select-all excludes the hidden root', folder not in s['selected'] and len(s['selected']) == 2, s)
probe('rootRange')
s = wait(lambda s: folder not in s['selected'])
check('a range anchored at the root is reconciled to content', len(s['selected']) == 1, s)
probe('ordinaryChoose', 1)
probe('treeAction', 'expand')
wait(lambda s: any(r.get('depth') == 2 for r in json.loads(ipc('data-goblin.fileblade', 'tree', '20'))['entries']))
probe('treeAction', 'collapse-all')
s = wait(lambda s: s['expanded'])
tree = json.loads(ipc('data-goblin.fileblade', 'tree', '20'))['entries']
check('collapse-all retains the root and collapses child folders', s['expanded'] and not any(r.get('depth', 0) > 1 for r in tree) and any(r['path'].endswith('/nested') and not r['expanded'] for r in tree), tree)
shot('repository')
control('setBladeWidth', 'left', 280)
time.sleep(.5)
shot('narrow')
control('setBladeWidth', 'left', 380)
probe('settings')
time.sleep(.5)
point = json.loads(probe('summarySettingPoint'))
assert point, 'Summary in tree setting is absent'
ovm('mouse', 'click', round(point['x']), round(point['y']))
wait(lambda s: s['inTree'])
probe('settings')
s = state()
check('settings toggle restores one interactive tree root', s['inTree'] and not s['above'] and s['rootHeight'] > 0, s)
probe('ordinaryChoose', 0)
probe('treeAction', 'collapse')
s = wait(lambda s: not s['expanded'])
check('tree preference restores root selection and collapse', s['selected'] == [folder] and s['count'] == 'Count unavailable', s)
time.sleep(2)
subprocess.run(['tests/vm/stop-shell'], check=True, timeout=40)
ovm('restart-shell')
control('openBlade', 'left')
s = wait(lambda s: s['root'] == folder)
check('tree preference survives shell restart', s['inTree'] and not s['above'], s)
shot('tree-restart')
probe('summaryInTree', 'false')
s = wait(lambda s: s['expanded'] and folder not in s['selected'])
check('relocation reopens root and removes its selection', s['above'] and s['rootHeight'] == 0, s)
control('setRoot', '/tmp/brindle-media/empty')
s = wait(lambda s: s['count'] == '0 items')
probe('treeAction', 'activate')
probe('treeAction', 'select-all')
s = state()
check('empty informational scope has no actionable hidden root', s['expanded'] and not s['selected'] and s['root'].endswith('/empty'), s)
probe('toggle')
s = state()
check('media uses the same summary and its media footer', s['above'] and s['mode'] and s['count'].endswith(' media'), s)
probe('toggle')
shot('empty')
print(f'{checks} context-summary checks passed', flush=True)
PY
