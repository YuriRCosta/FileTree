#!/usr/bin/env bash
set -euo pipefail
[[ $(hostname) == omarchy-test && $# == 1 ]] || { printf '%s\n' 'Run in the assigned guest: 97-delivery-adapter-runtime.sh ADAPTER' >&2; exit 1; }
python3 - "$1" <<'PY'
import hashlib
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile

adapter = str(Path(sys.argv[1]).resolve(strict=True))
with tempfile.TemporaryDirectory(prefix='fileblade-delivery-97.') as temporary:
    work = Path(temporary)
    home = work / 'home with spaces'
    installation = work / 'data/fileblade/installation'
    manifest = b'{}'
    digest = hashlib.sha256(manifest).hexdigest()
    runtime = installation / 'versions' / digest
    generation = installation / 'generations/generation.fixture'
    for path in (home / '.local/bin', runtime / 'app', runtime / 'bin', generation):
        path.mkdir(parents=True)
    (runtime / 'payload.json').write_bytes(manifest)
    (generation / 'receipt.json').write_text(json.dumps(dict(schema=1, owner='direct', installation=str(installation), payload=digest)))
    (generation / 'runtime').symlink_to('../../versions/' + digest)
    (installation / 'active').symlink_to('generations/generation.fixture')
    launcher = home / '.local/bin/fileblade'
    launcher.symlink_to(installation / 'launcher')
    log = work / 'calls.jsonl'
    def executable(path, text):
        path.write_text(text)
        path.chmod(0o755)
    executable(installation / 'launcher', '#!/usr/bin/env bash\nexec "$FILEBLADE_APP_ROOT/app/launch" "$@"\n')
    executable(runtime / 'bin/fileblade', '#!/usr/bin/env bash\nexit 0\n')
    executable(work / 'ovm', '#!/usr/bin/env bash\n[[ $# == 2 && $1 == ssh ]] || exit 2\nexec bash -c "$2"\n')
    executable(runtime / 'app/launch', '''#!/usr/bin/env python3
import json, os, pathlib, sys
args = sys.argv[1:]
with open(os.environ['CALL_LOG'], 'a') as log:
    log.write(json.dumps(dict(args=args, env={key: os.environ[key] for key in ['FILEBLADE_APP_ROOT', 'FILEBLADE_SOURCE_DIR', 'FILEBLADE_BINARY', 'FILEBLADE_NATIVE_STATE_ROOT']})) + '\\n')
if not args:
    pathlib.Path(os.environ['STARTED']).touch()
elif args[:2] == ['native', 'drain']:
    if os.environ.get('DRAIN_MODE') == 'changed':
        (pathlib.Path(os.environ['INSTALLATION']) / 'active').unlink()
    status = os.environ.get('DRAIN_MODE', 'drained')
    if status in ('drained', 'changed'):
        pathlib.Path(os.environ['STARTED']).unlink(missing_ok=True)
    print(json.dumps(dict(schema=1, action='drain', status='drained' if status == 'changed' else status, operation_ids=[], dirty_note_ids=[], error='')))
    sys.exit(3 if status == 'busy' else 0)
elif args[:3] == ['native', 'ipc', '--']:
    if args[3:] == ['fileblade.native', 'status']:
        print(json.dumps(dict(loaded=pathlib.Path(os.environ['STARTED']).exists(), sourceDir=os.environ['FILEBLADE_APP_ROOT'])))
    else:
        print(json.dumps(dict(bladeModules=['files', 'notes', 'properties', 'welcome', 'hooks', 'mcp', 'memory', 'skills', 'extension'])))
else:
    print('{}')
''')
    env = dict(os.environ, HOME=str(home), XDG_STATE_HOME=str(work / 'state'), OVM_REAL=str(work / 'ovm'), CALL_LOG=str(log), INSTALLATION=str(installation), STARTED=str(work / 'started'))
    env.pop('FILEBLADE_NATIVE_LAUNCHER', None)
    def call(*args, **extra):
        log.write_text('')
        result = subprocess.run([adapter, *args], env=dict(env, **extra), capture_output=True, text=True, timeout=10)
        calls = [json.loads(line) for line in log.read_text().splitlines()]
        return result, calls
    values = ['', '  ', 'line\nline', '"quotes"', '$(printf injected); `echo nope`']
    result, calls = call('ipc', 'data-goblin.fileblade.control', 'echo', *values)
    assert result.returncode == 0, result.stderr
    assert calls[0]['args'] == ['native', 'ipc', '--', 'data-goblin.fileblade.control', 'echo', *values]
    assert calls[0]['env'] == dict(FILEBLADE_APP_ROOT=str(runtime), FILEBLADE_SOURCE_DIR=str(runtime), FILEBLADE_BINARY=str(runtime / 'bin/fileblade'), FILEBLADE_NATIVE_STATE_ROOT=str(work / 'state/omarchy/fileblade'))
    print('PASS E-97-01 native IPC argv and R49 environment (protocol fixture)')
    for command in ('drop-context', 'drop-run', 'preferences-read', 'preferences-set'):
        result, calls = call('ipc', command, *values)
        assert result.returncode == 0 and calls[0]['args'] == ['_backend', command, *values], result.stderr
    print('PASS E-97-02 published backend argv (fixture; production implementation pending)')
    for verb in ('restart', 'restart-shell'):
        result, calls = call(verb)
        assert result.returncode == 0, result.stderr
        assert calls[0]['args'] == ['native', 'drain', '--timeout-ms', '30000', '--json']
        assert any(item['args'] == [] for item in calls)
    print('PASS E-97-03 both restart verbs drain then launch with extension-compatible readiness')
    for mode in ('busy', 'unknown', 'changed'):
        result, calls = call('restart', DRAIN_MODE=mode)
        assert result.returncode != 0 and all(item['args'] for item in calls), (mode, result, calls)
        if mode == 'busy':
            assert result.returncode == 3
    print('PASS E-97-04 busy, unknown and changed activation refuse restart')
    (installation / 'active').symlink_to('generations/generation.fixture')
    (runtime / 'bin/fileblade').unlink()
    result, calls = call('ipc', 'data-goblin.fileblade', 'status')
    assert result.returncode != 0 and not calls
    print('PASS E-97-05 missing production backend refuses before launch')
PY
