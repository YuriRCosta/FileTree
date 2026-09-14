#!/usr/bin/env bash
set -euo pipefail

repo=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/../../.." && pwd -P)
export OVM_HOME=${OVM_HOME:-$HOME/.local/share/test-omarchy-plugin-a}
export OVM_SSH_PORT=${OVM_SSH_PORT:-2422}
export FILEBLADE_PORTAL_EVIDENCE=${FILEBLADE_PORTAL_EVIDENCE:-$repo/.claude/evidence/sootscale/portal}
python3 - "$repo" <<'PY'
import base64
import hashlib
import json
import os
from pathlib import Path
import shlex
import subprocess
import time
import uuid

repo = Path(__import__('sys').argv[1])
shape = os.environ.get('FILEBLADE_SHAPE', 'plugin')
if shape not in ('plugin', 'native'):
    raise SystemExit('FILEBLADE_SHAPE must be plugin or native')
if shape == 'native' and os.environ.get('SKIP_PUSH') != '1':
    raise SystemExit('native portal qualification requires SKIP_PUSH=1')
ovm = os.environ.get('OVM') or (str(repo / 'tests/vm/native-ovm') if shape == 'native' else str(Path.home() / '.claude/skills/test-omarchy-plugin/scripts/ovm'))
out = Path(os.environ['FILEBLADE_PORTAL_EVIDENCE'])
out.mkdir(parents=True, exist_ok=True)
native_root = os.environ.get('FILEBLADE_NATIVE_ROOT', '/home/omarchy/fileblade-runtime-spike')
state_root = os.environ.get('FILEBLADE_SPIKE_HOME', '/home/omarchy/fileblade-runtime-state')
app = native_root + '/app' if shape == 'plugin' else None
binary = os.environ.get('FILEBLADE_NATIVE_BINARY', native_root + '/target/release/fileblade')
run_id = 'fileblade-portal-' + uuid.uuid4().hex
guest_fixture = '/tmp/' + run_id
upload_path = '/home/omarchy/000-' + run_id + '.txt'
portal_port = 38000 + int(uuid.uuid4().hex[:4], 16) % 1000
upload_body = ('FileBlade portal upload ' + run_id + '\n').encode()
backend_pid = None
browser_pid = None
server_pid = None
config_before = ''
layout_path = None
original_layout = None
original_root = None
original_focus = None
native_view_changed = False

def call(*args, timeout=30):
    return subprocess.check_output([ovm, *map(str, args)], text=True, timeout=timeout).strip()

def guest(command, timeout=30):
    return call('ssh', command, timeout=timeout)

def shell_path(path):
    return shlex.quote(path)

def write_guest(path, value):
    if isinstance(value, str):
        value = value.encode()
    encoded = base64.b64encode(value).decode()
    guest('mkdir -p ' + shell_path(str(Path(path).parent)) + '; printf %s ' + shell_path(encoded) + ' | base64 -d > ' + shell_path(path))

def read_guest_file(path):
    value = guest('if test -f ' + shell_path(path) + '; then printf found; base64 -w0 ' + shell_path(path) + '; else printf absent; fi')
    if not value.startswith('found'):
        return None
    return base64.b64decode(value[5:]).decode()

def ipc(method, *args):
    if shape == 'native':
        return json.loads(call('ipc', 'fileblade.chooser', method, *args))
    command = ['qs', 'ipc', '-n', '-p', app, 'call', '--', 'fileblade.chooser', method, *map(str, args)]
    return json.loads(guest(shlex.join(command)))

def fileblade_ipc(method, *args):
    if shape == 'native':
        return json.loads(call('ipc', 'data-goblin.fileblade', method, *args))
    command = ['qs', 'ipc', '-n', '-p', app, 'call', '--', 'data-goblin.fileblade', method, *map(str, args)]
    return json.loads(guest(shlex.join(command)))

def control(method, *args):
    if shape == 'native':
        return call('ipc', 'data-goblin.fileblade.control', method, *args)
    command = ['qs', 'ipc', '-n', '-p', app, 'call', '--', 'data-goblin.fileblade.control', method, *map(str, args)]
    return guest(shlex.join(command))

def native_identity():
    try:
        return json.loads(call('ipc', 'fileblade.native', 'status'))
    except (subprocess.SubprocessError, json.JSONDecodeError):
        return {}

def fileblade_status():
    try:
        return fileblade_ipc('status')
    except (subprocess.SubprocessError, json.JSONDecodeError):
        return {}

def native_probe():
    identity = native_identity()
    source = identity.get('sourceDir')
    if not source:
        return {}
    command = ['qs', 'ipc', '-n', '-p', source + '/app', 'call', '--', 'fileblade.qualification', 'status']
    try:
        return json.loads(guest(shlex.join(command)))
    except (subprocess.SubprocessError, json.JSONDecodeError):
        return {}

def launcher_command():
    launcher = os.environ.get('FILEBLADE_NATIVE_LAUNCHER')
    if not launcher:
        launcher = guest('if test -x "$HOME/.local/bin/fileblade"; then printf %s "$HOME/.local/bin/fileblade"; else printf /usr/bin/fileblade; fi')
    return shell_path(launcher)

def wait(predicate, label, seconds=30):
    deadline = time.monotonic() + seconds
    last = None
    while time.monotonic() < deadline:
        last = predicate()
        if last:
            return last
        time.sleep(0.2)
    raise AssertionError(label + ': ' + json.dumps(last, sort_keys=True))

def clients():
    return json.loads(call('hypr', 'clients'))

def browser_window():
    return wait(lambda: next((item for item in clients() if 'FileBlade portal upload' in item.get('title', '') and 'chrom' in str(item.get('class', '')).lower()), None), 'Chromium window')

def chooser_session():
    return wait(lambda: next((item for item in ipc('status').get('sessions', []) if item.get('opened')), None), 'FileBlade chooser session')

def chooser_window(session):
    title = session.get('title', '')
    return wait(lambda: next((item for item in clients() if item.get('title') == title), None), 'FileBlade chooser window')

def click_chooser(session, path):
    window = chooser_window(session)
    time.sleep(0.7)
    call('key', 'end')
    time.sleep(0.3)
    row = ipc('rowGeometry', session['handle'], path)
    if row.get('width', 0) <= 0 or row.get('height', 0) <= 0:
        raise AssertionError('chooser row has no visible geometry: ' + json.dumps(row))
    call('mouse', 'click', str(round(window['at'][0] + row['x'] + min(90, row['width'] / 2))), str(round(window['at'][1] + row['y'] + row['height'] / 2)))
    wait(lambda: next((item for item in ipc('status').get('sessions', []) if item.get('handle') == session['handle'] and item.get('selected')), None), 'chooser selection')
    shot = call('shot', 'native-portal-chooser-selection')
    (out / 'chooser-selection-shot.txt').write_text(shot + '\n')
    button = ipc('buttonGeometry', session['handle'], 'Open')
    if button.get('width', 0) <= 0 or button.get('height', 0) <= 0:
        raise AssertionError('chooser Open button has no visible geometry: ' + json.dumps(button))
    call('mouse', 'click', str(round(window['at'][0] + button['x'] + button['width'] / 2)), str(round(window['at'][1] + button['y'] + button['height'] / 2)))

def browser_server():
    page = repr('''<!doctype html><meta charset="utf-8"><title>FileBlade portal upload</title><style>body{margin:0;padding:40px;background:#202330;color:#fff;font:24px sans-serif}button{display:block;width:620px;height:120px;margin:0 0 24px;font-size:30px}#status{white-space:pre}</style><input id=file type=file hidden><button id=pick autofocus>Choose upload</button><button id=send disabled>Upload</button><pre id=status>ready</pre><script>const file=document.querySelector('#file'),pick=document.querySelector('#pick'),send=document.querySelector('#send'),status=document.querySelector('#status');let waiting=false;function report(value){status.textContent=value;fetch('/event?value='+encodeURIComponent(value)).catch(()=>{});}pick.onclick=()=>{waiting=true;report('choosing');file.click()};file.oncancel=()=>{waiting=false;report('cancelled')};file.onchange=()=>{waiting=false;if(file.files.length){status.textContent='selected:'+file.files[0].name+':'+file.files[0].size;send.disabled=false;fetch('/selected?name='+encodeURIComponent(file.files[0].name)+'&size='+file.files[0].size)}};send.onclick=async()=>{const value=file.files[0];const response=await fetch('/upload',{method:'POST',body:await value.arrayBuffer()});status.textContent=await response.text();};report('ready');</script>''')
    source = r'''
import hashlib,json,sys
from http.server import BaseHTTPRequestHandler,ThreadingHTTPServer
from pathlib import Path
root=Path(sys.argv[1]); body=bytes.fromhex(sys.argv[2]); result=root/'upload.json'; selected=root/'selected.json'; cancelled=root/'cancelled'
class Handler(BaseHTTPRequestHandler):
    def log_message(self,*args): pass
    def do_GET(self):
        if self.path == '/':
            value=page.encode(); self.send_response(200); self.send_header('Content-Length',str(len(value))); self.end_headers(); self.wfile.write(value); return
        if self.path.startswith('/selected?'):
            from urllib.parse import parse_qs,urlparse
            selected.write_text(json.dumps(parse_qs(urlparse(self.path).query)))
        if self.path.startswith('/event?'):
            from urllib.parse import parse_qs,urlparse
            query=parse_qs(urlparse(self.path).query)
            if query.get('value') == ['cancelled']: cancelled.touch()
            if query.get('value') == ['ready']: (root/'ready').touch()
            (root/'events').open('a').write(str(query)+'\n')
        self.send_response(204); self.end_headers()
    def do_POST(self):
        if self.path != '/upload': self.send_error(404); return
        value=self.rfile.read(int(self.headers.get('Content-Length','0')))
        result.write_text(json.dumps({'name': 'portal-upload', 'size': len(value), 'sha256': hashlib.sha256(value).hexdigest()}))
        self.send_response(200); self.send_header('Content-Length','8'); self.end_headers(); self.wfile.write(b'uploaded')
server=ThreadingHTTPServer(('127.0.0.1',int(sys.argv[3])),Handler)
server.serve_forever()
'''
    source = 'page=' + page + '\n' + source
    guest('mkdir -p ' + shell_path(guest_fixture))
    server_path = guest_fixture + '/server.py'
    write_guest(server_path, source)
    body_hex = upload_body.hex()
    return int(guest('setsid python3 ' + shell_path(server_path) + ' ' + shell_path(guest_fixture) + ' ' + shell_path(body_hex) + ' ' + str(portal_port) + ' >' + shell_path(guest_fixture + '/server.log') + ' 2>&1 & echo $!'))

def snapshot_layout():
    global layout_path, original_layout, original_root, original_focus
    status = fileblade_ipc('status')
    layout_path = status.get('bladeLayoutPath')
    if not isinstance(layout_path, str) or not layout_path.startswith('/') or not layout_path.endswith('/blades.json'):
        raise AssertionError('native layout path is unavailable: ' + str(layout_path))
    text = read_guest_file(layout_path)
    if text is None:
        raise AssertionError('native layout document is unavailable: ' + layout_path)
    try:
        original_layout = json.loads(text)
    except json.JSONDecodeError as error:
        raise AssertionError('native layout document is invalid: ' + str(error))
    if not isinstance(original_layout, dict) or not isinstance(original_layout.get('blades'), dict) or any(edge not in original_layout['blades'] for edge in ['left', 'right']):
        raise AssertionError('native layout document has no left and right blades')
    original_root = status.get('rootPath')
    original_focus = status.get('focusedBlade')
    (out / 'layout-before.json').write_text(text)
    if shape == 'native':
        (out / 'native-identity.json').write_text(json.dumps(native_identity(), indent=2) + '\n')

def temporary_close_blades():
    for edge in ['left', 'right']:
        control('closeBlade', edge)

def layout_matches():
    try:
        current = json.loads(read_guest_file(layout_path) or '')
    except (subprocess.CalledProcessError, json.JSONDecodeError, TypeError):
        return False
    return isinstance(current, dict) and all(current.get('blades', {}).get(edge) == original_layout['blades'][edge] for edge in ['left', 'right'])

def restore_layout():
    if not original_layout:
        return
    for edge in ['left', 'right']:
        blade = original_layout['blades'][edge]
        encoded = base64.b64encode(json.dumps(blade.get('slots', [])).encode()).decode()
        control('setBladeSlots', edge, 'base64:' + encoded)
        control('setBladeWidth', edge, str(blade.get('width', 0)))
        control('undockBlade' if blade.get('mode') == 'window' else 'dockBlade', edge)
        control('openBlade' if blade.get('open') else 'closeBlade', edge)
    if original_root:
        control('setRoot', original_root)
    if original_focus:
        control('focusBlade', original_focus)
    else:
        control('releaseBladeFocus')
    wait(layout_matches, 'original layout restoration', 20)
    (out / 'layout-after.json').write_text(read_guest_file(layout_path))

def stage_portal():
    global config_before, descriptor, config
    nonlocal_config = guest('printf %s "${XDG_CONFIG_HOME:-$HOME/.config}"')
    data_home = guest('printf %s "${XDG_DATA_HOME:-$HOME/.local/share}"')
    config_dirs = guest('printf %s "${XDG_CONFIG_DIRS:-/etc/xdg}"').split(':')
    data_dirs = guest('printf %s "${XDG_DATA_DIRS:-/usr/local/share:/usr/share}"').split(':')
    desktop = guest("systemctl --user show-environment | sed -n 's/^XDG_CURRENT_DESKTOP=//p'")
    desktops = [value.lower() for value in desktop.replace(';', ':').split(':') if value]
    bases = [nonlocal_config] + config_dirs + ['/etc', data_home] + data_dirs + ['/usr/share']
    candidates = []
    for base in bases:
        for desktop in desktops:
            candidates.append(base + '/xdg-desktop-portal/' + desktop + '-portals.conf')
        candidates.append(base + '/xdg-desktop-portal/portals.conf')
    effective_path = next((path for path in candidates if read_guest_file(path) is not None), None)
    config_before = read_guest_file(effective_path) if effective_path else ''
    config = effective_path if effective_path and effective_path.startswith(nonlocal_config + '/') else nonlocal_config + '/xdg-desktop-portal/portals.conf'
    descriptor = data_home + '/xdg-desktop-portal/portals/fileblade.portal'
    guest('mkdir -p ' + shell_path(guest_fixture + '/backup') + ' ' + shell_path(str(Path(descriptor).parent)) + ' ' + shell_path(str(Path(config).parent)))
    descriptor_path = shell_path(descriptor)
    config_path = shell_path(config)
    backup_descriptor = shell_path(guest_fixture + '/backup/descriptor')
    backup_config = shell_path(guest_fixture + '/backup/config')
    guest(f'if test -e {descriptor_path}; then cp -a {descriptor_path} {backup_descriptor}; else touch {backup_descriptor}.absent; fi')
    guest(f'if test -e {config_path}; then cp -a {config_path} {backup_config}; else touch {backup_config}.absent; fi')
    descriptor_text = '[portal]\nDBusName=org.freedesktop.impl.portal.desktop.fileblade\nInterfaces=org.freedesktop.impl.portal.FileChooser\n'
    if '[preferred]' in config_before:
        lines = (config_before.rstrip('\n') + '\n').splitlines(keepends=True)
        result = []
        in_preferred = False
        replaced = False
        for line in lines:
            stripped = line.strip()
            if stripped.startswith('['):
                if in_preferred and not replaced:
                    result.append('org.freedesktop.impl.portal.FileChooser=fileblade\n')
                    replaced = True
                in_preferred = stripped.lower() == '[preferred]'
            if in_preferred and stripped.lower().startswith('org.freedesktop.impl.portal.filechooser='):
                if not replaced:
                    result.append('org.freedesktop.impl.portal.FileChooser=fileblade\n')
                    replaced = True
                continue
            result.append(line)
        if in_preferred and not replaced:
            result.append('org.freedesktop.impl.portal.FileChooser=fileblade\n')
        config_text = ''.join(result)
    else:
        config_text = config_before.rstrip('\n') + ('\n' if config_before else '') + '[preferred]\norg.freedesktop.impl.portal.FileChooser=fileblade\n'
    write_guest(descriptor, descriptor_text)
    write_guest(config, config_text)
    return descriptor, config, config_text, effective_path or ''

def restart_portal_frontend():
    guest('systemctl --user restart xdg-desktop-portal.service')

def drain_native_view():
    result = json.loads(guest(launcher_command() + ' native drain --timeout-ms 30000 --json', timeout=40))
    if not (result.get('schema') == 1 and result.get('action') == 'drain'
            and result.get('status') in ['drained', 'already_stopped']
            and result.get('error') == ''
            and isinstance(result.get('operation_ids'), list)
            and all(isinstance(value, str) for value in result['operation_ids'])
            and isinstance(result.get('dirty_note_ids'), list)
            and all(isinstance(value, str) for value in result['dirty_note_ids'])):
        raise AssertionError('installed native view did not drain: ' + json.dumps(result, sort_keys=True))

def restore_portal(descriptor, config):
    backup_descriptor = shell_path(guest_fixture + '/backup/descriptor')
    backup_config = shell_path(guest_fixture + '/backup/config')
    if descriptor:
        descriptor_path = shell_path(descriptor)
        guest(f'if test -f {backup_descriptor}; then cp -a {backup_descriptor} {descriptor_path}; elif test -f {backup_descriptor}.absent; then rm -f {descriptor_path}; fi')
    if config:
        config_path = shell_path(config)
        guest(f'if test -f {backup_config}; then cp -a {backup_config} {config_path}; elif test -f {backup_config}.absent; then rm -f {config_path}; fi')
    for path, backup in [(descriptor, backup_descriptor), (config, backup_config)]:
        if path:
            guest(f'if test -f {backup}; then cmp -s {backup} {shell_path(path)}; elif test -f {backup}.absent; then test ! -e {shell_path(path)}; fi')
    if descriptor or config:
        restart_portal_frontend()

def start_backend():
    if shape == 'native':
        command = 'env FILEBLADE_QUALIFICATION=1 FILEBLADE_CHOOSER=1 ' + launcher_command() + ' native portal'
    else:
        command = 'env FILEBLADE_SPIKE_HOME=' + shell_path(state_root) + ' XDG_CONFIG_HOME=' + shell_path(state_root + '/config') + ' XDG_STATE_HOME=' + shell_path(state_root + '/state') + ' FILEBLADE_NATIVE_STATE_ROOT=' + shell_path(state_root + '/state/omarchy/fileblade') + ' ' + shell_path(binary) + ' native portal'
    return int(guest('setsid ' + command + ' >' + shell_path(guest_fixture + '/portal.log') + ' 2>&1 < /dev/null & echo $!'))

def start_browser():
    profile = guest_fixture + '/chromium'
    url = 'http://127.0.0.1:' + str(portal_port) + '/'
    command = 'env GDK_BACKEND=wayland GTK_USE_PORTAL=1 ' + 'chromium --user-data-dir=' + shell_path(profile) + ' --no-first-run --no-default-browser-check --disable-session-crashed-bubble --ozone-platform=wayland --start-maximized ' + shell_path(url)
    return int(guest('setsid ' + command + ' >' + shell_path(guest_fixture + '/browser.log') + ' 2>&1 < /dev/null & echo $!'))

def stop_pid(pid):
    if pid:
        guest('kill -TERM ' + str(pid) + ' >/dev/null 2>&1 || true')

def cleanup(descriptor, config):
    global native_view_changed
    errors = []
    def attempt(label, action):
        try:
            action()
        except Exception as error:
            errors.append(label + ': ' + str(error))
    attempt('stop Chromium', lambda: stop_pid(browser_pid))
    attempt('stop browser server', lambda: stop_pid(server_pid))
    attempt('stop portal backend', lambda: stop_pid(backend_pid))
    attempt('restore original layout', restore_layout)
    if descriptor or config:
        attempt('restore portal configuration', lambda: restore_portal(descriptor, config))
    if native_view_changed:
        attempt('drain qualification view', drain_native_view)
        native_view_changed = False
        attempt('restart ordinary native view', lambda: call('restart', timeout=40))
    attempt('remove upload fixture', lambda: guest('rm -f ' + shell_path(upload_path)))
    if errors:
        (out / 'cleanup-error.txt').write_text('\n'.join(errors) + '\n')
        raise RuntimeError('; '.join(errors))
    guest('rm -rf ' + shell_path(guest_fixture))

def route_lines(value):
    return [line.strip() for line in value.splitlines() if '=' in line and not line.strip().lower().startswith('org.freedesktop.impl.portal.filechooser=')]

descriptor = config = None
try:
    guest('mkdir -p ' + shell_path(guest_fixture))
    snapshot_layout()
    if shape == 'native':
        drain_native_view()
        native_view_changed = True
    guest('printf %s ' + shell_path(upload_body.decode()) + ' > ' + shell_path(upload_path))
    descriptor, config, config_after, effective_config = stage_portal()
    backend_pid = start_backend()
    if shape == 'native':
        wait(lambda: fileblade_status().get('bladeModules'), 'installed native view', 30)
        wait(lambda: native_probe().get('slots') is not None, 'installed qualification chooser probe', 30)
    if ipc('status').get('sessions'):
        raise AssertionError('close existing fixture chooser sessions before running E-47')
    wait(lambda: guest('gdbus call --session --dest org.freedesktop.impl.portal.desktop.fileblade --object-path /org/freedesktop/portal/desktop --method org.freedesktop.DBus.Peer.Ping >/dev/null 2>&1 && echo yes || true') == 'yes', 'FileBlade portal bus registration', 20)
    restart_portal_frontend()
    server_pid = browser_server()
    guest('for attempt in $(seq 1 50); do curl -fsS http://127.0.0.1:' + str(portal_port) + '/ >/dev/null && exit 0; sleep .1; done; exit 1', timeout=20)
    temporary_close_blades()
    browser_pid = start_browser()
    browser = browser_window()
    wait(lambda: guest('test -f ' + shell_path(guest_fixture + '/ready') + ' && echo yes || true') == 'yes', 'browser page ready', 20)
    call('key', 'ret')
    cancelled = chooser_session()
    chooser_window(cancelled)
    time.sleep(0.7)
    call('key', 'esc')
    wait(lambda: not ipc('status').get('sessions'), 'cancelled chooser cleanup', 20)
    wait(lambda: guest('test -f ' + shell_path(guest_fixture + '/cancelled') + ' && echo yes || true') == 'yes', 'browser receives cancellation', 20)
    if guest('test -f ' + shell_path(guest_fixture + '/upload.json') + ' && echo yes || true') == 'yes':
        raise AssertionError('cancelled browser chooser uploaded data')
    browser = browser_window()
    call('key', 'ret')
    session = chooser_session()
    click_chooser(session, upload_path)
    wait(lambda: guest('test -f ' + shell_path(guest_fixture + '/selected.json') + ' && echo yes || true') == 'yes', 'browser receives selected file', 20)
    browser = browser_window()
    call('key', 'tab')
    call('key', 'ret')
    result_path = guest_fixture + '/upload.json'
    wait(lambda: guest('test -f ' + shell_path(result_path) + ' && echo yes || true') == 'yes', 'browser upload result', 30)
    result = json.loads(guest('cat ' + shell_path(result_path)))
    expected = {'name': 'portal-upload', 'size': len(upload_body), 'sha256': hashlib.sha256(upload_body).hexdigest()}
    if result != expected:
        raise AssertionError('browser uploaded unexpected bytes: ' + json.dumps(result, sort_keys=True))
    selected = json.loads(guest('cat ' + shell_path(guest_fixture + '/selected.json')))
    if selected.get('name') != [Path(upload_path).name] or selected.get('size') != [str(len(upload_body))]:
        raise AssertionError('browser selected metadata mismatch: ' + json.dumps(selected, sort_keys=True))
    browser_shot = call('shot', 'native-portal-browser-upload')
    (out / 'browser-upload-shot.txt').write_text(browser_shot + '\n')
    non_filechooser_before = route_lines(config_before)
    non_filechooser_after = route_lines(config_after)
    if non_filechooser_before != non_filechooser_after:
        raise AssertionError('portal config changed a route other than FileChooser')
    evidence = {'descriptor': descriptor, 'config': config, 'effective_config': effective_config, 'bus_name': 'org.freedesktop.impl.portal.desktop.fileblade', 'interface': 'org.freedesktop.impl.portal.FileChooser', 'cancelled': True, 'selected': selected, 'upload': result, 'other_routes_preserved': True, 'shape': shape, 'qualification_flags': ['FILEBLADE_QUALIFICATION=1', 'FILEBLADE_CHOOSER=1'] if shape == 'native' else []}
    (out / 'E47-portal.json').write_text(json.dumps(evidence, indent=2) + '\n')
    (out / 'upload-shot.txt').write_text(browser_shot + '\n')
except Exception:
    try:
        (out / 'failure-shot.txt').write_text(call('shot', 'native-portal-failure') + '\n')
        (out / 'failure-status.json').write_text(json.dumps(ipc('status'), indent=2) + '\n')
    except Exception:
        pass
    raise
finally:
    try:
        for name in ['portal.log', 'server.log', 'browser.log', 'events']:
            value = read_guest_file(guest_fixture + '/' + name)
            if value is not None:
                (out / name).write_text(value)
    finally:
        cleanup(descriptor, config)
print('PASS E-47-01: Chromium opened the registered FileBlade portal and cancellation closed its chooser')
print('PASS E-47-02: Chromium uploaded the selected bytes through the FileBlade portal')
print('PASS E-47-03: Other portal routes, original chooser configuration and blade state were preserved')
PY
