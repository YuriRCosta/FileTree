#!/usr/bin/env bash
set -euo pipefail
: "${OVM:?set OVM to the headless VM harness}"
FILETREE_EXPECTATIONS_LIB="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd -P)/lib.sh"
export FILETREE_EXPECTATIONS_LIB
python3 - <<'PY'
import json
import math
import os
import shlex
import shutil
import subprocess
import time
from pathlib import Path

OVM = os.environ['OVM']
PLUGIN = 'yuricosta.filetree'
LIB_SH = os.environ['FILETREE_EXPECTATIONS_LIB']
ROOT = '/home/omarchy/fb-hunk-wheel'
REPO = ROOT + '/repo'
FILE = REPO + "/nested/quote' and space.txt"
shots = Path('target/hunk-vm')
shots.mkdir(parents=True, exist_ok=True)

def ovm(*args):
    result = subprocess.run([OVM, *map(str, args)], capture_output=True, text=True, timeout=45)
    if result.returncode:
        raise RuntimeError(f'{args}: {result.stderr} {result.stdout}')
    return result.stdout.strip()

def guest(command):
    return ovm('ssh', command)

def ctl(*args):
    result = subprocess.run(
        ['bash', '-c', 'source "$1" && ctl "${@:2}"', 'filetree-ctl', LIB_SH, *map(str, args)],
        capture_output=True, text=True, timeout=45)
    if result.returncode:
        raise RuntimeError(f'{args}: {result.stderr} {result.stdout}')
    return result.stdout.strip()

def status():
    return json.loads(ovm('ipc', PLUGIN, 'status'))

def wait(probe, seconds=20):
    deadline = time.monotonic() + seconds
    last = None
    while time.monotonic() < deadline:
        last = probe()
        if last:
            return last
        time.sleep(.25)
    raise AssertionError(f'timed out: {last}')

def clients():
    return json.loads(ovm('hypr', 'clients'))

def shot(name):
    time.sleep(.8)
    original = Path(ovm('shot', 'hunk-wheel-' + name).splitlines()[-1])
    destination = shots / (name + '.png')
    shutil.copyfile(original, destination)
    original.unlink()
    print('Screenshot:', destination, flush=True)

def cleanup_case():
    ctl('hideDropWheel')
    guest("pkill -x hunk || true; pkill -x foot || true; tmux kill-server 2>/dev/null || true; herdr --session fb-hunk-wheel server stop 2>/dev/null || true")
    wait(lambda: not clients())

def launch(mux):
    command = {'herdr': 'herdr --session fb-hunk-wheel',
               'tmux': 'tmux new-session -s fb-hunk-wheel',
               'plain': 'bash --noprofile --norc'}[mux]
    guest(f'setsid foot -e {command} >{ROOT}/terminal.log 2>&1 </dev/null &')
    client = wait(lambda: next((c for c in clients() if c['class'] == 'foot'), None))
    time.sleep(1)
    return client

def hunk_processes():
    raw = guest("python3 -c " + shlex.quote('''import json, pathlib
rows=[]
for p in pathlib.Path('/proc').glob('[0-9]*'):
 try:
  if (p/'comm').read_text().strip() != 'hunk': continue
  argv=(p/'cmdline').read_bytes().decode().split('\\0')[:-1]
  if argv[1:2] != ['diff']: continue
  env=dict(x.split('=',1) for x in (p/'environ').read_bytes().decode().split('\\0') if x.startswith(('HERDR_PANE_ID=', 'TMUX_PANE=')))
  rows.append({'pid':int(p.name),'cwd':str((p/'cwd').resolve()),'argv':argv, 'env':env})
 except (OSError,UnicodeError): pass
print(json.dumps(rows))'''))
    return json.loads(raw)

def mux_state(mux):
    if mux == 'tmux':
        raw = guest("tmux list-panes -a -F '#{session_id},#{window_id},#{pane_id}'")
        rows = [row.split(',') for row in raw.splitlines()]
        return [len(set(row[i] for row in rows)) for i in range(3)]
    workspaces = json.loads(guest('herdr --session fb-hunk-wheel workspace list'))['result']['workspaces']
    panes = []
    tabs = []
    for workspace in workspaces:
        wid = workspace['workspace_id']
        panes += json.loads(guest(f'herdr --session fb-hunk-wheel pane list --workspace {wid}'))['result']['panes']
        tabs += json.loads(guest(f'herdr --session fb-hunk-wheel tab list --workspace {wid}'))['result']['tabs']
    return [len(workspaces), len(tabs), len(panes)]

def open_wheel(mux, client=None):
    if client:
        x, y = [round(client['at'][i] + client['size'][i] / 2) for i in range(2)]
    else:
        x, y = 1000, 500
    ovm('mouse', 'move', x, y)
    ctl('select', FILE)
    ctl('showDropWheel', x, y)
    wait(lambda: status()['dropWheel']['open'])
    def loaded():
        wheel = status()['dropWheel']
        return wheel if wheel['open'] and not wheel['loading'] and wheel['actions'] else None
    wheel = wait(loaded)
    expected_target = {'herdr': 'terminal (herdr)', 'tmux': 'terminal (tmux)', 'plain': 'terminal', 'desktop': 'Desktop'}[mux]
    assert wheel['target'] == expected_target, wheel
    index, row = next((i, r) for i, r in enumerate(wheel['actions']) if r['id'] == 'review')
    expected = ['Vertical split', 'Horizontal split', 'New tab', 'New space', 'New window'] if mux in ('herdr', 'tmux') else []
    assert [p['label'] for p in row['placements']] == expected, wheel
    angle = -math.pi / 2 + index * 2 * math.pi / len(wheel['actions'])
    px, py = round(x + 58 * math.cos(angle)), round(y + 58 * math.sin(angle))
    ovm('mouse', 'move', px, py)
    wait(lambda: status()['dropWheel']['highlighted'] == index)
    return wheel, row, angle, (px, py)

assert not clients(), 'Use an idle VM without open windows'
assert guest(f'test -x {ROOT}/bin/hunk && echo yes') == 'yes'
guest('mkdir -p ' + shlex.quote(REPO + '/nested'))
guest('git -C ' + REPO + ' init -q')
guest('printf "before\\n" > ' + shlex.quote(FILE))
guest(f'git -C {REPO} add . && git -C {REPO} -c user.name=Test -c user.email=test@example.invalid commit -qm initial')
guest('printf "after\\n" > ' + shlex.quote(FILE))
guest(f'mv /home/omarchy/.local/bin/hunk {ROOT}/hunk-original; ln -s {ROOT}/bin/hunk /home/omarchy/.local/bin/hunk')
try:
    ctl('setRoot', REPO)
    ctl('open')
    for mux in ['herdr', 'tmux']:
        for placement in ['pane', 'tab', 'workspace', 'window']:
            cleanup_case()
            client = launch(mux)
            before = mux_state(mux)
            wheel, row, angle, _ = open_wheel(mux, client)
            if placement == 'pane':
                shot(mux + '-choices')
            child = next(i for i, p in enumerate(row['placements']) if p['id'] == placement)
            child_angle = angle + (child - 1.5) * math.pi / 4
            px = round(wheel['x'] + 116 * math.cos(child_angle))
            py = round(wheel['y'] + 116 * math.sin(child_angle))
            ovm('mouse', 'move', px, py)
            wait(lambda: status()['dropWheel']['outerHighlighted'] == child)
            ovm('mouse', 'click', px, py)
            wait(lambda: not status()['dropWheel']['open'])
            processes = wait(hunk_processes)
            assert len(processes) == 1, processes
            process = processes[0]
            assert process['cwd'] == REPO, process['cwd']
            assert process['argv'][1:] == ['diff', '--', FILE], process['argv']
            after = mux_state(mux)
            delta = {'pane': [0, 0, 1], 'tab': [0, 1, 1], 'workspace': [1, 1, 1], 'window': [0, 0, 0]}[placement]
            assert after == [a+b for a,b in zip(before, delta)], (mux, placement, before, after)
            if placement == 'window':
                assert len(clients()) == 2, clients()
                assert json.loads(ovm('hypr', 'activewindow'))['address'] != client['address']
            else:
                assert len(clients()) == 1, clients()
                assert process['env'].get('HERDR_PANE_ID' if mux == 'herdr' else 'TMUX_PANE'), process
            shot(mux + '-' + placement)
            print(f'ok E-26-11 {mux}/{placement}: {before} -> {after}; Hunk PID {process["pid"]}, correct cwd and selected path', flush=True)
    for mux in ['plain', 'desktop']:
        cleanup_case()
        client = launch(mux) if mux == 'plain' else None
        _, _, _, point = open_wheel(mux, client)
        shot(mux + '-choices')
        ovm('mouse', 'click', *point)
        process = wait(hunk_processes)[0]
        assert process['cwd'] == REPO
        assert process['argv'][1:] == ['diff', '--', FILE]
        assert len(clients()) == (2 if client else 1)
        print(f'ok E-26-11 {mux}: no destinations; direct Hunk terminal', flush=True)
finally:
    cleanup_case()
    ctl('setRoot', '/home/omarchy')
    guest(f'rm /home/omarchy/.local/bin/hunk; mv {ROOT}/hunk-original /home/omarchy/.local/bin/hunk; rm -rf {ROOT} /home/omarchy/.config/herdr/sessions/fb-hunk-wheel')
PY
