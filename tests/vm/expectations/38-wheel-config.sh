#!/usr/bin/env bash
set -euo pipefail
: "${OVM:?set OVM to the harness executable}"
[[ -n ${OVM_HOME:-} && -n ${OVM_SSH_PORT:-} ]]
FILEBLADE_EXPECTATIONS_LIB="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd -P)/lib.sh"
export FILEBLADE_EXPECTATIONS_LIB
python3 - <<'PY'
import base64
import copy
import json
import os
import shlex
import subprocess
import time

ovm = os.environ['OVM']
LIB_SH = os.environ['FILEBLADE_EXPECTATIONS_LIB']
plugin = '/home/omarchy/.config/omarchy/plugins/data-goblin.fileblade'
fixture = '/tmp/fb-wheel-config'
config_path = 'Path(os.environ.get("XDG_CONFIG_HOME", str(Path.home() / ".config")))'
settings_path = config_path + ' / "omarchy/fileblade/settings.json"'
terminals_path = config_path + ' / "xdg-terminals.list"'
source = fixture + "/item ' $(echo injection) #%.txt"
receipt = fixture + '/receipt'
native_folder = os.fsencode(fixture) + b'/cwd-\xff'
native_source = native_folder + b'/odd-\xff.txt'
native_uri = 'file://' + fixture + '/cwd-%FF/odd-%FF.txt'
literal_args = ['file:///tmp/example%20name', '', "$(touch /tmp/fb-wheel-injected); 'quoted' \\ backslash\n\n"]
byte_command = ['python3','-c',"import pathlib,sys,time,os,json;pathlib.Path(sys.argv[1]).write_text(json.dumps([os.getcwdb().hex(),*[os.fsencode(arg).hex() for arg in sys.argv[2:]]]));time.sleep(3)",receipt,'{paths}','{cwd}',*literal_args]
expected_bytes = [value.hex() for value in [native_folder,native_source,os.fsencode(source),native_folder,*map(os.fsencode,literal_args)]]
checks = 0


def call(*args):
    result = subprocess.run([ovm, *map(str, args)], capture_output=True, text=True, timeout=45)
    if result.returncode:
        raise RuntimeError(result.stdout + result.stderr)
    return result.stdout.strip()


def guest(code):
    payload = base64.b64encode(code.encode()).decode()
    return call('ssh', shlex.join(['python3', '-c', 'import base64;exec(base64.b64decode(' + repr(payload) + '))']))


def control(*args):
    result = subprocess.run(
        ['bash', '-c', 'source "$1" && ctl "${@:2}"', 'fileblade-ctl', LIB_SH, *map(str, args)],
        capture_output=True, text=True, timeout=45)
    if result.returncode:
        raise RuntimeError(result.stdout + result.stderr)
    return result.stdout.strip()


def state():
    return json.loads(call('ipc', 'data-goblin.fileblade', 'status'))['dropWheel']


def backend(*args):
    return json.loads(call('ssh', shlex.join([plugin + '/fileblade', '_backend', *args])))


def shot(expectations, label):
    time.sleep(.6)
    path = call('shot', 'pebbleclaw-38-' + '-'.join(expectations) + '-' + label)
    for expectation in expectations:
        print(json.dumps(dict(expectation=expectation, shot=path)), flush=True)


def check(action, expected, observed):
    global checks
    if expected != observed:
        raise AssertionError((action, expected, observed))
    checks += 1
    print(json.dumps(dict(action=action, expected=expected, observed=observed)), flush=True)


def wait(predicate):
    deadline = time.monotonic() + 15
    last = None
    while time.monotonic() < deadline:
        last = predicate()
        if last:
            return last
        time.sleep(.15)
    raise AssertionError(('timed out', last))


def save(document):
    guest('from pathlib import Path;import json,os;p=' + settings_path + ';p.parent.mkdir(parents=True,exist_ok=True);p.write_text(' + repr(json.dumps(document)) + ');p.chmod(0o600)')


original = guest('from pathlib import Path;import base64,os;p=' + settings_path + ';print(base64.b64encode(p.read_bytes()).decode() if p.exists() else "MISSING")')
document = json.loads(base64.b64decode(original)) if original != 'MISSING' else dict(version=1, trashRetentionDays=0)
document['futureWheelTest'] = dict(preserve=True)
document['dropWheel'] = dict(version=1, future=dict(preserve=True), actions=[dict(id='custom:inspect'), dict(id='terminal', label='Shell', icon='utilities-terminal'), dict(id='open', hidden=True)], customActions=[
    dict(id='custom:inspect', label='Inspect', key='i', glyph='I', targetKinds=['desktop'], placements=[dict(id='format', label='Format', key='f', placements=[dict(id='capture', label='Capture arguments', key='c', command=['python3', '-c', "import pathlib,sys;pathlib.Path(sys.argv[1]).write_text('\\n'.join(sys.argv[2:]))", receipt, '{paths}', '{cwd}', '{git_root}', '$(touch /tmp/fb-wheel-injected)'], runMode='detached')])]),
    dict(id='custom:invalid', label='Invalid fixture', command='invalid shell string')])
try:
    guest('from pathlib import Path;import subprocess;p=Path(' + repr(fixture) + ');p.mkdir(exist_ok=True);Path(' + repr(source) + ').write_text("fixture");subprocess.run(["git","init","-q",str(p)],check=True);Path(' + repr(receipt) + ').unlink(missing_ok=True);Path("/tmp/fb-wheel-injected").unlink(missing_ok=True)')
    guest('import os;from pathlib import Path;os.makedirs(' + repr(native_folder) + ',exist_ok=True);open(' + repr(native_source) + ',"wb").write(b"fixture")')
    save(document)
    backend('preferences-set', '--trash-retention-days', '0')
    saved = backend('preferences-read')['settings']
    check('normal preference save preserves nested unknown wheel keys', document['dropWheel'], saved['dropWheel'])
    check('normal preference save preserves other future settings', document['futureWheelTest'], saved['futureWheelTest'])
    context = backend('drop-context', '--x', '900', '--y', '500', '--path', source)
    check('configured action comes first and hidden open is absent', ['custom:inspect', 'terminal', 'open-with', 'review'], [row['id'] for row in context['actions']])
    check('configured icon overrides inherited source', ['utilities-terminal', '', True], [context['actions'][1].get(key) for key in ['icon','icon_source','icon_override']])
    check('invalid action reports a diagnosis without crashing the wheel', True, any('argument array' in message for message in context['diagnostics']))
    control('openBlade', 'left')
    control('focusBlade', 'left')
    control('setRoot', fixture)
    control('select', source)
    control('showDropWheel', '900', '500')
    wait(lambda: state()['open'] and not state()['loading'])
    check('live wheel uses configured order', 'custom:inspect', state()['actions'][0]['id'])
    shot(['E-38-01', 'E-38-02', 'E-38-03', 'E-38-07', 'E-38-08'], 'configured')
    call('key', 'i')
    call('key', 'f')
    shot(['E-38-04', 'E-38-06'], 'third-ring')
    call('key', 'c')
    wait(lambda: guest('from pathlib import Path;print(Path(' + repr(receipt) + ').exists())') == 'True')
    observed = json.loads(guest('from pathlib import Path;import json;print(json.dumps(Path(' + repr(receipt) + ').read_text().splitlines()))'))
    check('third-ring command receives exact whole arguments', [source, fixture, fixture, '$(touch /tmp/fb-wheel-injected)'], observed)
    check('literal shell-looking argument never executes', 'False', guest('from pathlib import Path;print(Path("/tmp/fb-wheel-injected").exists())'))
    wait(lambda: not state()['open'])
    check('successful custom action closes wheel', False, state()['open'])
    document['dropWheel']['customActions'] = [dict(id='custom:bytes',label='Byte receipt',command=byte_command)]
    save(document)
    guest('from pathlib import Path;Path(' + repr(receipt) + ').unlink(missing_ok=True)')
    result = backend('drop-run','--action','configured','--placement','["custom:bytes"]','--target','{"kind":"desktop"}','--path',native_uri,'--path',source)
    check('detached byte probe dispatch accepted',True,result['ok'])
    wait(lambda: guest('from pathlib import Path;print(Path(' + repr(receipt) + ').exists())') == 'True')
    check('detached preserves native cwd and complete child argv bytes',expected_bytes,json.loads(guest('from pathlib import Path;print(Path(' + repr(receipt) + ').read_text())')))
    check('detached never evaluates shell-looking arguments','False',guest('from pathlib import Path;print(Path("/tmp/fb-wheel-injected").exists())'))
    terminal_preferences = guest('from pathlib import Path;import base64,os;p=' + terminals_path + ';print(base64.b64encode(p.read_bytes()).decode() if p.exists() else "MISSING")')
    try:
        for terminal, desktop in [('foot', 'foot.desktop'), ('footclient', 'footclient.desktop')]:
            guest('from pathlib import Path;import os;(' + terminals_path + ').write_text(' + repr(desktop + '\n') + ')')
            for mode in ['terminal', 'herdr', 'tmux']:
                call('ssh', 'pkill -x foot || true; pkill -x ghostty || true; herdr --session wheel-config server stop >/dev/null 2>&1 || true; tmux kill-session -t wheelconfig 2>/dev/null || true')
                wait(lambda: not json.loads(call('hypr', 'clients')))
                if terminal == 'footclient':
                    call('ssh', 'setsid foot --server >/tmp/fb-wheel-config-foot-server.log 2>&1 </dev/null &')
                    wait(lambda: guest('import os;from pathlib import Path;print(any(Path(os.environ["XDG_RUNTIME_DIR"]).glob("foot-*.sock")))') == 'True')
                target = dict(kind='desktop')
                before = []
                if mode != 'terminal':
                    mux = ['herdr','--session','wheel-config'] if mode == 'herdr' else ['tmux','new-session','-s','wheelconfig']
                    call('ssh', 'setsid ' + shlex.join([terminal, '-e', *mux]) + ' >/tmp/fb-wheel-config-terminal.log 2>&1 </dev/null &')
                    before = wait(lambda: json.loads(call('hypr','clients')))
                    client = before[0]
                    x = str(round(client['at'][0] + client['size'][0] / 2))
                    y = str(round(client['at'][1] + client['size'][1] / 2))
                    def resolved():
                        context = backend('drop-context','--x',x,'--y',y,'--path',source)
                        target = context['target']
                        details = target.get('terminal',dict())
                        ready = details.get('pane_id') if mode == 'herdr' else details.get('session')
                        return target if details.get('multiplexer') == mode and ready else None
                    target = wait(resolved)
                command = byte_command
                entry = dict(id='custom:mode',label='Mode probe',key='u',command=command,runMode='terminal' if mode == 'terminal' else 'multiplexer')
                if mode != 'terminal':
                    entry['placement'] = 'right'
                document['dropWheel']['customActions'] = [entry]
                document['dropWheel']['actions'] = [dict(id='custom:mode')]
                save(document)
                guest('from pathlib import Path;Path(' + repr(receipt) + ').unlink(missing_ok=True)')
                outcome = backend('drop-run','--action','configured','--placement',json.dumps(['custom:mode']),'--target',json.dumps(target),'--path',native_uri,'--path',source)
                print(json.dumps(dict(terminal=terminal,mode=mode,target=target,outcome=outcome)),flush=True)
                check(terminal + '/' + mode + ' custom dispatch accepted',True,outcome['ok'])
                wait(lambda: guest('from pathlib import Path;print(Path(' + repr(receipt) + ').exists())') == 'True')
                actual = json.loads(guest('from pathlib import Path;print(Path(' + repr(receipt) + ').read_text())'))
                check(terminal + '/' + mode + ' preserves native cwd and complete child argv bytes',expected_bytes,actual)
                check(terminal + '/' + mode + ' never evaluates shell-looking arguments','False',guest('from pathlib import Path;print(Path("/tmp/fb-wheel-injected").exists())'))
                clients = json.loads(call('hypr','clients'))
                check(terminal + '/' + mode + ' runs in the expected terminal class',True,any(client['class'] == terminal for client in clients))
                expectations = ['E-38-05', 'E-38-09'] if terminal == 'foot' and mode == 'herdr' else ['E-38-09']
                shot(expectations, terminal + '-' + mode)
    finally:
        call('ssh', 'pkill -x foot || true; pkill -x ghostty || true; herdr --session wheel-config server stop >/dev/null 2>&1 || true; tmux kill-session -t wheelconfig 2>/dev/null || true')
        if terminal_preferences == 'MISSING':
            guest('from pathlib import Path;import os;(' + terminals_path + ').unlink(missing_ok=True)')
        else:
            guest('from pathlib import Path;import base64,os;p=' + terminals_path + ';p.write_bytes(base64.b64decode(' + repr(terminal_preferences) + '))')
    alias = dict(id='custom:alias',label='Current alias',key='a',builtin=dict(action='terminal'),conditions=dict(path=[fixture+'/*']))
    document['dropWheel'] = dict(version=1,actions=[dict(id='custom:alias')],customActions=[alias])
    save(document)
    context = backend('drop-context','--x','900','--y','500','--path',source)
    captured = next(row for row in context['actions'] if row['id'] == 'custom:alias')
    check('custom built-in alias receives a configured route',['custom:alias'],captured.get('command_route'))
    control('openBlade', 'left')
    control('focusBlade', 'left')
    control('setRoot', fixture)
    control('select', source)
    control('showDropWheel', '900', '500')
    wait(lambda: state()['open'] and not state()['loading'])
    shot(['E-38-10'], 'alias-route')
    route = json.dumps(captured['command_route'])
    result = backend('drop-run','--action','configured','--placement',route,'--target','{"kind":"desktop"}','--path',source,'--dry-run')
    check('current alias invokes the fixed terminal implementation',True,result['ok'] and any('xdg-terminal-exec' in arg for cmd in result['commands'] for arg in cmd))
    for change in ['removed','hidden','path condition','mime condition','target kind']:
        changed = copy.deepcopy(alias)
        if change == 'hidden':
            changed['hidden'] = True
        if change == 'path condition':
            changed['conditions'] = dict(path=['/unavailable/*'])
        if change == 'mime condition':
            changed['conditions'] = dict(mime=['image/*'])
        if change == 'target kind':
            changed['targetKinds'] = ['editor']
        document['dropWheel']['customActions'] = [] if change == 'removed' else [changed]
        save(document)
        result = backend('drop-run','--action','configured','--placement',route,'--target','{"kind":"desktop"}','--path',source,'--dry-run')
        check(change + ' invalidates a captured built-in alias',False,result['ok'])
        check(change + ' alias launches no command',[],result['commands'])
    document['dropWheel']['customActions'] = []
    save(document)
    result = backend('drop-run', '--action', 'configured', '--placement', json.dumps(['custom:inspect','format','capture']), '--target', '{"kind":"desktop"}', '--path', source, '--dry-run')
    check('removed custom action cannot run through a stale route', False, result['ok'])
    check('stale action dispatch launches no command', [], result['commands'])
finally:
    control('hideDropWheel')
    if original == 'MISSING':
        guest('from pathlib import Path;import os;(' + settings_path + ').unlink(missing_ok=True)')
    else:
        guest('from pathlib import Path;import base64,os;p=' + settings_path + ';p.write_bytes(base64.b64decode(' + repr(original) + '));p.chmod(0o600)')
print(str(checks) + ' wheel configuration checks passed')
PY
