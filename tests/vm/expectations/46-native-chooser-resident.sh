#!/usr/bin/env bash
set -euo pipefail
export OVM_HOME=$HOME/.local/share/test-omarchy-plugin-a
export OVM_SSH_PORT=2422
export FILEBLADE_CHOOSER_EVIDENCE=${FILEBLADE_CHOOSER_EVIDENCE:-.claude/evidence/sootscale/chooser/resident-ui}
python3 - <<'PY'
import json
import os
from pathlib import Path
import shlex
import subprocess
import time
import uuid

ovm = str(Path.cwd() / 'app/ovm-spike')
app = '/home/omarchy/fileblade-runtime-spike/app'
out = Path(os.environ['FILEBLADE_CHOOSER_EVIDENCE'])
out.mkdir(parents=True, exist_ok=True)
fixture = '/tmp/fileblade-chooser-' + uuid.uuid4().hex
callers = {}

def call(*args):
    return subprocess.check_output([ovm, *args], text=True).strip()

def guest(*argv):
    return call('ssh', shlex.join(argv))

def ipc(method, *args):
    return json.loads(guest('qs', 'ipc', '-n', '-p', app, 'call', '--', 'fileblade.chooser', method, *args))

def wait(check):
    deadline = time.monotonic() + 20
    while time.monotonic() < deadline:
        result = check()
        if result:
            return result
        time.sleep(0.1)
    raise AssertionError('resident chooser did not reach expected state: ' + json.dumps(ipc('status')))

def session(handle):
    return next((item for item in ipc('status')['sessions'] if item['handle'] == handle), None)

def ordinary():
    state = json.loads(call('ipc', 'data-goblin.fileblade', 'status'))
    return {'root': state['rootPath'], 'hashes': call('ssh', 'sha256sum "$XDG_STATE_HOME/omarchy/fileblade/state.json" "$XDG_CONFIG_HOME/omarchy/fileblade/blades.json"')}

client = '''import json,os,pathlib,socket,sys
connection=socket.socket(socket.AF_UNIX)
connection.settimeout(300)
connection.connect(os.environ['FILEBLADE_NATIVE_STATE_ROOT']+'/authority.sock')
reader=connection.makefile('rb')
def send(frame): connection.sendall(json.dumps(frame).encode()+b'\\n')
send({'v':1,'type':'hello'})
assert json.loads(reader.readline())['authority']
document=json.loads(sys.argv[1])
send({'v':1,'type':'request','id':'caller','generation':1,'command':'chooser','arguments':['offer','--document',json.dumps(document)],'deadline_ms':300000})
response=json.loads(reader.readline())
path=pathlib.Path(sys.argv[2]); temporary=path.with_suffix('.pending')
temporary.write_text(json.dumps(response)); temporary.replace(path)
'''

def offer(name, mode='open', multiple=False, filters=None, current_name=''):
    handle = fixture.rsplit('/', 1)[-1] + '-' + name
    document = dict(handle=handle, caller='qualification:' + name, parent_window='', title='R58 ' + name, accept_label='', modal=True, current_folder=fixture, current_name=current_name, mode=mode, multiple=multiple, filters=filters or [], current_filter=None)
    result = fixture + '/' + name + '.result'
    command = shlex.join(['python3', '-c', client, json.dumps(document), result])
    pid = int(call('ssh', 'setsid ' + command + ' >' + shlex.quote(fixture + '/' + name + '.log') + ' 2>&1 </dev/null & echo $!'))
    callers[handle] = (pid, result)
    wait(lambda: (item := session(handle)) and item['opened'] and len(item['rows']) > 1)
    return handle

def click(handle, method, value):
    item = ipc(method, handle, value)
    assert item.get('width', 0) > 0 and item.get('height', 0) > 0, item
    name = handle[len(fixture.rsplit('/', 1)[-1]) + 1:]
    window = next(item for item in json.loads(call('hypr', 'clients')) if item['title'] == 'R58 ' + name)
    x = window['at'][0] + item['x'] + (min(90, item['width'] / 2) if method == 'rowGeometry' else item['width'] / 2)
    y = window['at'][1] + item['y'] + item['height'] / 2
    call('mouse', 'click', str(round(x)), str(round(y)))

def result(handle):
    path = callers[handle][1]
    wait(lambda: guest('sh', '-c', 'test -f ' + shlex.quote(path) + ' && echo ready || true') == 'ready')
    response = json.loads(guest('cat', path))
    assert response['type'] == 'response' and response['payload']['ok'], response
    (out / (handle.rsplit('-', 1)[-1] + '.json')).write_text(json.dumps(response, indent=2))
    wait(lambda: session(handle) is None)
    del callers[handle]
    return response['payload']['outcome']

assert json.loads(call('ipc', 'data-goblin.fileblade', 'bladeModules'))['count'] == 8
assert not ipc('status')['sessions']
guest('python3', '-c', "from pathlib import Path; import sys; p=Path(sys.argv[1]); p.mkdir(); (p/'alpha.txt').write_text('original'); (p/'beta.png').write_text('not text'); (p/'folder').mkdir()", fixture)
time.sleep(1)
before = ordinary()
try:
    opened = offer('open', filters=[['Text', [[0, '*.txt']]]])
    saved = offer('save', mode='save', current_name='alpha.txt')
    assert len(ipc('status')['sessions']) == 2
    rows = session(opened)['rows']
    assert next(row for row in rows if row['name'] == 'alpha.txt')['selectable']
    assert not next(row for row in rows if row['name'] == 'beta.png')['selectable']
    click(opened, 'rowGeometry', fixture + '/beta.png')
    assert session(opened)['selected'] == []
    click(opened, 'rowGeometry', fixture + '/alpha.txt')
    wait(lambda: len(session(opened)['selected']) == 1)
    (out / 'two-requests.json').write_text(json.dumps(ipc('status'), indent=2))
    (out / 'two-shot.txt').write_text(call('shot', 'r58-chooser-two') + '\n')
    click(opened, 'buttonGeometry', 'Open')
    assert result(opened)['uris'] == [Path(fixture + '/alpha.txt').as_uri()]
    assert session(saved)['opened']
    print('PASS E46-01: filtered Open returns its URI through the resident authority while Save stays open')
    click(saved, 'buttonGeometry', 'Save')
    wait(lambda: session(saved)['overwrite'])
    (out / 'overwrite-shot.txt').write_text(call('shot', 'r58-chooser-overwrite') + '\n')
    click(saved, 'buttonGeometry', 'Replace')
    assert result(saved)['uris'] == [Path(fixture + '/alpha.txt').as_uri()]
    assert guest('cat', fixture + '/alpha.txt') == 'original'
    print('PASS E46-02: Save confirms replacement and returns a URI without writing the target')
    multiple = offer('multiple', multiple=True)
    click(multiple, 'rowGeometry', fixture + '/alpha.txt')
    call('hold', 'ctrl')
    try:
        click(multiple, 'rowGeometry', fixture + '/beta.png')
    finally:
        call('release', 'ctrl')
    wait(lambda: len(session(multiple)['selected']) == 2)
    click(multiple, 'buttonGeometry', 'Open')
    assert set(result(multiple)['uris']) == {Path(fixture + '/' + name).as_uri() for name in ['alpha.txt', 'beta.png']}
    print('PASS E46-07: real multi-selection returns both local URIs')
    new = offer('new', mode='save', current_name='new file.txt')
    click(new, 'buttonGeometry', 'Save')
    assert result(new)['uris'] == [Path(fixture + '/new file.txt').as_uri()]
    guest('test', '!', '-e', fixture + '/new file.txt')
    print('PASS E46-08: new Save destination returns its URI without creating the file')
    folder = offer('folder', mode='folder')
    click(folder, 'rowGeometry', fixture + '/folder')
    click(folder, 'buttonGeometry', 'Open')
    assert result(folder)['uris'] == [Path(fixture + '/folder').as_uri()]
    print('PASS E46-03: folder selection completes through the same authority')
    cancelled = offer('cancel')
    click(cancelled, 'rowGeometry', fixture + '/alpha.txt')
    call('key', 'esc')
    assert result(cancelled)['status'] == 'cancelled'
    print('PASS E46-04: real Escape returns cancellation to the resident caller')
    disconnected = offer('disconnect')
    guest('kill', '-TERM', str(callers[disconnected][0]))
    wait(lambda: session(disconnected) is None)
    del callers[disconnected]
    print('PASS E46-05: caller process exit closes its chooser via actual socket EOF')
    assert ordinary() == before
    (out / 'ordinary.json').write_text(json.dumps({'before': before, 'after': ordinary()}, indent=2))
    print('PASS E46-06: resident chooser completion preserves ordinary state and layout')
except BaseException:
    (out / 'failure.json').write_text(json.dumps(ipc('status'), indent=2))
    (out / 'failure-shot.txt').write_text(call('shot', 'r58-chooser-failure') + '\n')
    raise
finally:
    for pid, path in callers.values():
        call('ssh', 'kill -TERM ' + str(pid) + ' 2>/dev/null || true')
    wait(lambda: not ipc('status')['sessions'])
print('Resident native fixture qualification; foreign parent, portal mediation and browser upload remain separate')
PY
