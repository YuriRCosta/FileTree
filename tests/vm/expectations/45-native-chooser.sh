#!/usr/bin/env bash
set -euo pipefail
export OVM_HOME=$HOME/.local/share/test-omarchy-plugin-a
export OVM_SSH_PORT=2422
export FILEBLADE_CHOOSER_EVIDENCE=${FILEBLADE_CHOOSER_EVIDENCE:-.claude/evidence/sootscale/chooser/ui}
python3 - <<'PY'
import json
import os
from pathlib import Path
import shlex
import subprocess
import time

ovm = str(Path.cwd() / 'app/ovm-spike')
out = Path(os.environ['FILEBLADE_CHOOSER_EVIDENCE'])
out.mkdir(parents=True, exist_ok=True)

def call(*args):
    return subprocess.check_output([ovm, *args], text=True).strip()

def ipc(method, *args):
    return call('ssh', shlex.join(['qs', 'ipc', '-n', '-p', '/home/omarchy/fileblade-runtime-spike/app', 'call', '--', 'fileblade.chooser', method, *args]))

def state():
    return json.loads(ipc('status'))

def wait(check):
    end = time.monotonic() + 15
    while time.monotonic() < end:
        result = check()
        if result:
            return result
        time.sleep(0.1)
    raise AssertionError('chooser UI did not reach expected state')

def ordinary():
    status = json.loads(call('ipc', 'data-goblin.fileblade', 'status'))
    hashes = call('ssh', 'sha256sum "$XDG_STATE_HOME/omarchy/fileblade/state.json" "$XDG_CONFIG_HOME/omarchy/fileblade/blades.json"')
    return {'root': status.get('rootPath'), 'hashes': hashes}

assert json.loads(call('ipc', 'data-goblin.fileblade', 'bladeModules'))['count'] == 8
ipc('fixture', json.dumps({'offers': []}))
call('ssh', 'mkdir -p /tmp/fileblade-chooser-e45/a /tmp/fileblade-chooser-e45/b; printf first > /tmp/fileblade-chooser-e45/a/one.txt; printf second > /tmp/fileblade-chooser-e45/b/two.txt')
call('ipc', 'data-goblin.fileblade.control', 'focusBlade', 'right')
wait(lambda: json.loads(call('ipc', 'data-goblin.fileblade', 'status'))['focusedBlade'] == 'right')
time.sleep(1)
before = ordinary()
(out / 'ordinary-before.json').write_text(json.dumps(before, indent=2))
offers = [dict(handle='e45-' + name, caller='fixture:' + name, parent_window='', title='E45 chooser ' + name, accept_label='Choose', modal=True, current_folder='/tmp/fileblade-chooser-e45/' + name, current_name='', mode='open', multiple=False, filters=[], current_filter=None) for name in ['a', 'b']]
try:
    assert ipc('fixture', json.dumps({'offers': offers})) == 'presented'
    def ready():
        sessions = state()['sessions']
        return sessions if len(sessions) == 2 and all(s['root'] == '/tmp/fileblade-chooser-e45/' + s['handle'][-1] and len(s['rows']) == 2 for s in sessions) else None
    sessions = wait(ready)
    assert json.loads(call('ipc', 'data-goblin.fileblade', 'status'))['focusedBlade'] == ''
    print('PASS: opening a chooser yields the ordinary blade focus grab')
    (out / 'two-offers.json').write_text(json.dumps(sessions, indent=2))
    print('PASS: two live chooser windows retain distinct roots and rows')
    process_probe = "import pathlib,subprocess,json; pid=subprocess.check_output(['pgrep','-f','^qs -n -p /home/omarchy/fileblade-runtime-spike/app$'],text=True).strip(); children=pathlib.Path('/proc/'+pid+'/task/'+pid+'/children').read_text().split(); print(json.dumps([pathlib.Path('/proc/'+child+'/cmdline').read_bytes().replace(bytes([0]),b' ').decode() for child in children]))"
    processes = json.loads(call('ssh', shlex.join(['python3', '-c', process_probe])))
    (out / 'view-children.json').write_text(json.dumps(processes, indent=2))
    assert len(processes) == 1 and 'serve' in processes[0]
    print('PASS: two chooser views share the one resident QML backend process')
    for name, filename in [('a', 'one.txt'), ('b', 'two.txt')]:
        handle = 'e45-' + name
        path = '/tmp/fileblade-chooser-e45/' + name + '/' + filename
        clients = json.loads(call('hypr', 'clients'))
        window = next(c for c in clients if c['title'] == 'E45 chooser ' + name)
        row = json.loads(ipc('rowGeometry', handle, path))
        (out / ('click-' + name + '.json')).write_text(json.dumps({'window': window, 'row': row, 'state': state()}, indent=2))
        assert row['width'] > 0 and row['height'] > 0
        call('mouse', 'click', str(round(window['at'][0] + row['x'] + min(90, row['width'] / 2))), str(round(window['at'][1] + row['y'] + row['height'] / 2)))
        wait(lambda: any(s['handle'] == handle and [entry['path'] for entry in s['selected']] == [path] for s in state()['sessions']))
    selected = state()
    (out / 'selected.json').write_text(json.dumps(selected, indent=2))
    print('PASS: real pointer selects a different file in each request')
    shot = call('shot', 'e45-native-chooser-two')
    (out / 'shot.txt').write_text(shot + '\n')
    assert ordinary() == before
    print('PASS: chooser navigation and selection leave ordinary root/state/layout unchanged')
    call('key', 'esc')
    wait(lambda: any(s['handle'] == 'e45-b' and not s['opened'] for s in state()['sessions']))
    remaining = next(s for s in state()['sessions'] if s['handle'] == 'e45-a')
    assert remaining['opened'] and remaining['selected'][0]['path'].endswith('/a/one.txt')
    print('PASS: Escape closes one chooser and leaves the other request and selection intact')
except BaseException:
    (out / 'failure.json').write_text(json.dumps(state(), indent=2))
    (out / 'failure-shot.txt').write_text(call('shot', 'e45-native-chooser-failure') + '\n')
    raise
finally:
    ipc('fixture', json.dumps({'offers': []}))
    wait(lambda: not state()['sessions'])
    (out / 'ordinary-after.json').write_text(json.dumps(ordinary(), indent=2))
assert ordinary() == before
print('UI fixture checks only; resident completion and portal upload require registration')
PY
