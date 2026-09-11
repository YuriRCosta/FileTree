#!/usr/bin/env bash
set -euo pipefail
export OVM_HOME=${OVM_HOME:-/home/kurt/.local/share/test-omarchy-plugin-a}
export OVM_SSH_PORT=${OVM_SSH_PORT:-2422}
if [[ $OVM_HOME != /home/kurt/.local/share/test-omarchy-plugin-a || $OVM_SSH_PORT != 2422 ]]; then
  printf '%s\n' 'native authority qualification requires harness A on SSH 2422' >&2
  exit 2
fi
repo=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/../../.." && pwd -P)
ovm=${OVM:-$repo/app/ovm-spike}
payload=$(base64 -w0 <<'PY'
import base64, json, os, pathlib, shutil, socket, subprocess, tempfile, time

app = pathlib.Path('/home/omarchy/fileblade-runtime-spike/app')
root = pathlib.Path(os.environ['FILEBLADE_NATIVE_STATE_ROOT'])
connection = socket.socket(socket.AF_UNIX)
connection.settimeout(5)
connection.connect(str(root / 'authority.sock'))
stream = connection.makefile('rw')

def request(frame):
    stream.write(json.dumps({'v': 1, **frame}) + '\n')
    stream.flush()
    return json.loads(stream.readline())

def operation(action, op=''):
    return request({'type': 'operation', 'id': 'native40', 'generation': 40, 'action': action, 'op': op})

def ipc(target, method, *args):
    return subprocess.check_output(['qs', '-p', str(app), 'ipc', 'call', target, method, *args], text=True, timeout=5).strip()

def control(method, *args):
    return ipc('data-goblin.fileblade.control', method, *args)

assert request({'type': 'hello'})['authority']
original = json.loads(ipc('data-goblin.fileblade', 'status'))
holder = json.loads((root / 'authority.lock').read_text())
identity = (root.stat().st_dev, root.stat().st_ino, (root / 'authority.lock').stat().st_ino)
source = pathlib.Path(tempfile.mkdtemp(prefix='fileblade-native40-', dir='/tmp'))
destination = pathlib.Path(tempfile.mkdtemp(prefix='fileblade-native40-', dir='/home/omarchy'))
shot = source / 'accepted.png'
paths = [source / 'first.bin', source / 'second.bin']
for path in paths:
    with path.open('wb') as output:
        output.truncate(512 * 1024 * 1024)
assert source.stat().st_dev != destination.stat().st_dev
control('setRoot', str(source))
control('open')
deadline = time.monotonic() + 5
while json.loads(ipc('data-goblin.fileblade', 'status'))['rootPath'] != str(source):
    assert time.monotonic() < deadline, 'fixture navigation did not complete'
    time.sleep(0.02)
entries = json.dumps([{'path': str(path), 'name': path.name, 'isDir': False} for path in paths])
selected = control('selectEntries', 'base64:' + base64.b64encode(entries.encode()).decode())
assert selected == 'ok', selected
existing = {entry['op'] for entry in operation('list')['payload']}
request_id = control('moveSelectionTo', str(destination), 'false')
op = None
accepted = None
deadline = time.monotonic() + 5
while time.monotonic() < deadline and op is None:
    for entry in operation('list')['payload']:
        if entry['op'] in existing:
            continue
        response = operation('get', entry['op'])
        if not response.get('ok'):
            continue
        state = response['payload']
        frame = state.get('progress') or state.get('result') or {}
        if str(frame.get('generation')) == request_id:
            assert not state['complete'], 'move completed before the view-close boundary could be exercised'
            op, accepted = entry['op'], state
            break
    time.sleep(0.005)
assert op, 'the QML move did not reach authority admission'
subprocess.run(['grim', '-o', 'Virtual-1', str(shot)], check=True, timeout=5)
subprocess.run(['qs', '-p', str(app), 'kill'], check=True, timeout=5)
deadline = time.monotonic() + 20
while True:
    response = operation('get', op)
    assert response['ok'], response
    state = response['payload']
    if state['complete']:
        break
    assert time.monotonic() < deadline, 'accepted move did not finish after view close'
    time.sleep(0.02)
result = state['result']
assert result['payload']['ok'], result
assert len(result['payload']['mappings']) == 2, result
for path in paths:
    assert not path.exists(), path
    assert (destination / path.name).stat().st_size == 512 * 1024 * 1024
assert json.loads((root / 'authority.lock').read_text()) == holder
assert (root.stat().st_dev, root.stat().st_ino, (root / 'authority.lock').stat().st_ino) == identity
with (source / 'relaunch.log').open('w') as log:
    environment = dict(os.environ, FILEBLADE_SPIKE_HOME='/tmp/fileblade-native-state')
    subprocess.Popen([str(app / 'launch')], env=environment, stdin=subprocess.DEVNULL, stdout=log, stderr=log, start_new_session=True)
deadline = time.monotonic() + 10
while True:
    try:
        if json.loads(ipc('fileblade.native', 'status'))['loaded']:
            break
    except (subprocess.SubprocessError, json.JSONDecodeError):
        pass
    assert time.monotonic() < deadline, 'native view did not relaunch'
    time.sleep(0.1)
assert operation('get', op)['payload']['result'] == result
assert operation('fetch', op)['payload']['result'] == result
assert not operation('get', op)['ok']
control('setRoot', original['rootPath'])
print(json.dumps({'ok': True, 'expectations': ['E-40-01', 'E-40-02', 'E-40-03'], 'operation': op, 'accepted': accepted, 'result': result, 'holder': holder, 'identity': identity, 'screenshot': str(shot), 'fixture': str(source)}))
shutil.rmtree(destination)
stream.close()
connection.close()
PY
)
"$ovm" ssh "printf %s '$payload' | base64 -d | python3"
