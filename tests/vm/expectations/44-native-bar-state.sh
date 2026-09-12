#!/usr/bin/env bash
set -euo pipefail
repo=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/../../.." && pwd -P)
export OVM_HOME=/home/kurt/.local/share/test-omarchy-plugin-a OVM_SSH_PORT=2422
python3 - "$repo" <<'PY'
import base64,json,pathlib,subprocess,sys,time
repo=pathlib.Path(sys.argv[1]); ovm=str(repo/'app/ovm-spike')
base='/home/kurt/.claude/skills/test-omarchy-plugin/scripts/ovm'
out=repo/'.claude/evidence/sootscale/integration/bar-state'; out.mkdir(parents=True,exist_ok=True)
def run(*args): return subprocess.check_output([ovm,*map(str,args)],text=True,timeout=20).strip()
def guest(command): return subprocess.check_output([base,'ssh',command],text=True,timeout=20).strip()
def probe(): return json.loads(run('ssh','qs -p /home/omarchy/fileblade-runtime-spike/app ipc call fileblade.qualification status'))
def write_config(config):
    data=base64.b64encode(json.dumps(config).encode()).decode()
    guest("set -e; config_file=$(mktemp ~/.config/omarchy/shell.json.XXXXXX); printf %s '"+data+"' | base64 -d > \"$config_file\"; chmod --reference ~/.config/omarchy/shell.json \"$config_file\"; mv -f -- \"$config_file\" ~/.config/omarchy/shell.json; omarchy-shell shell reloadConfig")
def layers():
    values=json.loads(run('hypr','layers'))['Virtual-1']['levels'].values()
    return [r for rows in values for r in rows if r['namespace'] in ['omarchy-fileblade-left','omarchy-fileblade-right']]
def observe(hidden,position):
    surfaces=probe()['surfaces']; actual=layers()
    matches=len(surfaces)==2 and len(actual)==2
    size=0 if hidden else (28 if position in ['left','right'] else 26)
    for surface in surfaces:
        row=next((r for r in actual if r['namespace']=='omarchy-fileblade-'+surface['edge']),None)
        matches=matches and bool(row) and surface['barHidden']==hidden
        x=(size if position=='left' else 0) if surface['edge']=='left' else 1920-surface['width']-(size if position=='right' else 0)
        y=size if position=='top' else 0
        height=1080-(size if position in ['top','bottom'] else 0)
        matches=matches and surface['liveBarSize']==size and surface['surfaceOriginX']==x and surface['surfaceOriginY']==y and surface['height']==height
        if row:
            matches=matches and surface['surfaceOriginX']==row['x'] and surface['surfaceOriginY']==row['y'] and surface['width']==row['w'] and surface['height']==row['h']
    return matches,{'surfaces':surfaces,'layers':actual,'hidden':hidden,'position':position}
def wait(hidden,position):
    deadline=time.monotonic()+12
    while True:
        ok,value=observe(hidden,position)
        if ok: return value
        assert time.monotonic()<deadline,value
        time.sleep(.1)
def capture(name,hidden):
    value=wait(hidden,'top' if name.startswith('rapid-') else name.split('-')[0])
    (out/(name+'.json')).write_text(json.dumps(value,indent=2))
    path=pathlib.Path(run('shot','native-bar-'+name).splitlines()[-1]); (out/(name+'.png')).write_bytes(path.read_bytes())
    print('ok E-44-01 '+name+' actual and internal coordinates match',flush=True)
    return value
original=json.loads(guest('cat ~/.config/omarchy/shell.json'))
was_hidden=guest('test -f ~/.local/state/omarchy/toggles/bar-off && echo yes || echo no')=='yes'
blades=json.loads(run('ipc','data-goblin.fileblade','blades'))['blades']; records={}
try:
    run('ipc','data-goblin.fileblade.control','openBlade','left'); run('ipc','data-goblin.fileblade.control','openBlade','right')
    for position in ['top','bottom','left','right']:
        config=json.loads(json.dumps(original)); config.setdefault('bar',{})['position']=position
        write_config(config); guest('omarchy-toggle-bar off'); time.sleep(.8)
        records[position+'-visible']=capture(position+'-visible',False)
        guest('omarchy-toggle-bar on'); records[position+'-hidden']=capture(position+'-hidden',True)
        guest('omarchy-toggle-bar off'); records[position+'-restored']=capture(position+'-restored',False)
    config=json.loads(json.dumps(original)); config.setdefault('bar',{})['position']='top'; write_config(config)
    guest('for n in $(seq 1 20); do omarchy-toggle-bar on; omarchy-toggle-bar off; done; omarchy-toggle-bar on')
    records['rapid-hidden']=capture('rapid-hidden',True)
    guest('omarchy-toggle-bar off'); records['rapid-restored']=capture('rapid-restored',False)
    (out/'results.json').write_text(json.dumps({'ok':True,'records':records},indent=2))
finally:
    write_config(original); guest('omarchy-toggle-bar '+('on' if was_hidden else 'off'))
    for edge in ['left','right']: run('ipc','data-goblin.fileblade.control','openBlade' if blades[edge]['open'] else 'closeBlade',edge)
PY
