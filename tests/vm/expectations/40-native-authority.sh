#!/usr/bin/env bash
set -euo pipefail
export OVM_HOME=${OVM_HOME:-$HOME/.local/share/test-omarchy-plugin-a}
export OVM_SSH_PORT=${OVM_SSH_PORT:-2422}
repo=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/../../.." && pwd -P)
ovm=${OVM:-$repo/app/ovm-spike}
payload=$(base64 -w0 <<'PY'
import base64, json, os, pathlib, shutil, signal, socket, subprocess, tempfile, time

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
    environment = dict(os.environ, FILEBLADE_SPIKE_HOME=str(pathlib.Path(os.environ['XDG_STATE_HOME']).parent))
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

replacement_source = pathlib.Path(tempfile.mkdtemp(prefix='fileblade-native40-replacement-', dir='/home/omarchy'))
replacement_destination = replacement_source / 'destination'
replacement_destination.mkdir()
replacement_file = replacement_source / 'source.bin'
with replacement_file.open('wb') as output:
    output.truncate(1024 * 1024 * 1024)
control('setRoot', str(replacement_source))
deadline = time.monotonic() + 5
while json.loads(ipc('data-goblin.fileblade', 'status'))['rootPath'] != str(replacement_source):
    assert time.monotonic() < deadline
    time.sleep(0.02)
entries = json.dumps([{'path': str(replacement_file), 'name': replacement_file.name, 'isDir': False}])
assert control('selectEntries', 'base64:' + base64.b64encode(entries.encode()).decode()) == 'ok'
existing = {entry['op'] for entry in operation('list')['payload']}
request_id = control('moveSelectionTo', str(replacement_destination), 'true')
deadline = time.monotonic() + 5
while True:
    pending = [entry for entry in operation('list')['payload'] if entry['op'] not in existing]
    running = []
    for entry in pending:
        snapshot = operation('get', entry['op'])['payload']
        if str((snapshot.get('progress') or snapshot.get('result') or {}).get('generation')) == request_id:
            running.append(snapshot)
    partials = list(replacement_destination.glob('.fileblade-partial-*/item'))
    if running and partials and partials[0].stat().st_size > 0:
        replacement_op = running[0]['op']
        break
    assert time.monotonic() < deadline, 'replacement copy did not reach temporary output'
    time.sleep(0.001)
os.kill(holder['pid'], signal.SIGSTOP)
moved_root = root.with_name(root.name + '-native40-original')
assert not moved_root.exists()
try:
    assert partials[0].stat().st_size < replacement_file.stat().st_size
    root.rename(moved_root)
    root.mkdir(mode=0o700)
    (root / 'sentinel').write_text('unchanged')
    replacement_shot = replacement_source / 'root-replaced.png'
    subprocess.run(['grim', '-o', 'Virtual-1', str(replacement_shot)], check=True, timeout=5)
    subprocess.run(['qs', '-p', str(app), 'kill'], check=True, timeout=5)
finally:
    os.kill(holder['pid'], signal.SIGCONT)
deadline = time.monotonic() + 20
while True:
    state = operation('get', replacement_op)['payload']
    if state['complete']:
        break
    assert time.monotonic() < deadline, 'lost authority did not finish its explicit failure result'
    time.sleep(0.02)
replacement_result = state['result']
assert replacement_result['error_id'] == 'authority-lost', replacement_result
assert not replacement_result['ok'] and not replacement_result['cancelled']
assert sorted(path.name for path in root.iterdir()) == ['sentinel']
assert (root / 'sentinel').read_text() == 'unchanged'
assert replacement_file.stat().st_size == 1024 * 1024 * 1024
assert operation('fetch', replacement_op)['payload']['result'] == replacement_result
stream.close()
connection.close()
os.kill(holder['pid'], signal.SIGTERM)
deadline = time.monotonic() + 10
while pathlib.Path('/proc', str(holder['pid'])).exists():
    assert time.monotonic() < deadline, 'lost authority did not shut down'
    time.sleep(0.02)
assert sorted(path.name for path in root.iterdir()) == ['sentinel']
(root / 'sentinel').unlink()
root.rmdir()
moved_root.rename(root)
with (replacement_source / 'relaunch.log').open('w') as log:
    subprocess.Popen([str(app / 'launch')], env=environment, stdin=subprocess.DEVNULL, stdout=log, stderr=log, start_new_session=True)
deadline = time.monotonic() + 10
while True:
    try:
        if json.loads(ipc('fileblade.native', 'status'))['loaded']:
            break
    except (subprocess.SubprocessError, json.JSONDecodeError):
        pass
    assert time.monotonic() < deadline
    time.sleep(0.1)
control('setRoot', original['rootPath'])
replacement_file.unlink()
shutil.rmtree(replacement_destination)
print(json.dumps({'ok': True, 'expectations': ['E-40-04'], 'operation': replacement_op,
    'result': replacement_result, 'replacementEntries': ['sentinel'], 'screenshot': str(replacement_shot),
    'restoredOriginalRoot': str(root)}))
stream.close()
connection.close()
PY
)
"$ovm" ssh "printf %s '$payload' | base64 -d | python3"
