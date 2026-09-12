#!/usr/bin/env bash
set -euo pipefail
repo=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/../../.." && pwd -P)
export OVM_HOME=${OVM_HOME:-$HOME/.local/share/test-omarchy-plugin-a} OVM_SSH_PORT=${OVM_SSH_PORT:-2422}
python3 - "$repo" "${1:?choose plugin or native after staging that shape}" <<'PY'
import base64,json,pathlib,shlex,subprocess,sys,time
repo=pathlib.Path(sys.argv[1]); shape=sys.argv[2]; assert shape in ['plugin','native']
base=str(pathlib.Path.home()/'.claude/skills/test-omarchy-plugin/scripts/ovm')
ovm=str(repo/'app/ovm-spike') if shape=='native' else base
out=repo/'.claude/evidence/sootscale/r15/parity'/shape; out.mkdir(parents=True,exist_ok=True)
def run(*args):
    if shape=='plugin' and args[0]=='ipc': args=('ssh',shlex.join(['omarchy-shell',*map(str,args[1:])]))
    return subprocess.check_output([ovm,*map(str,args)],text=True,timeout=20).strip()
def ctl(method,*args): return run('ipc','data-goblin.fileblade.control',method,*args)
def status(): return json.loads(run('ipc','data-goblin.fileblade','status'))
def blades(): return json.loads(run('ipc','data-goblin.fileblade','blades'))['blades']
def slots(edge,rows): return ctl('setBladeSlots',edge,'base64:'+base64.b64encode(json.dumps(rows).encode()).decode())
def wait(predicate):
    end=time.monotonic()+15
    while not predicate():
        assert time.monotonic()<end,status()
        time.sleep(.1)
def bar_hidden(hidden):
    subprocess.check_call([base,'ssh','omarchy-toggle-bar '+('on' if hidden else 'off')],stdout=subprocess.DEVNULL)
    time.sleep(.8)
def check(value,label):
    assert value,label
    print('ok '+label,flush=True)
def capture(name):
    state=status(); monitor=json.loads(run('hypr','monitors'))[0]; layers=json.loads(run('hypr','layers'))
    rows=[r for values in layers[monitor['name']]['levels'].values() for r in values if r['namespace'].startswith('omarchy-fileblade-')]
    data={'status':state,'reserved':monitor['reserved'],'layers':rows,'window':json.loads(run('hypr','activewindow'))}
    if shape=='native': data['probe']=json.loads(run('ssh','qs -p /home/omarchy/fileblade-runtime-spike/app ipc call fileblade.qualification status'))
    (out/(name+'.json')).write_text(json.dumps(data,indent=2))
    image=pathlib.Path(run('shot','r15-'+shape+'-'+name).splitlines()[-1]); (out/(name+'.png')).write_bytes(image.read_bytes())
    return data
original=blades(); original_root=status()['rootPath']; fixture='/home/omarchy/fileblade-r15-parity'; sink=None
initial_hidden=run('ssh','test -f ~/.local/state/omarchy/toggles/bar-off && echo yes || echo no')=='yes'
records={}
try:
    run('ssh',f'mkdir -p {fixture}; printf one > {fixture}/needle.txt; printf two > {fixture}/other.txt')
    bar_hidden(False)
    for edge in ['left','right']: ctl('closeBlade',edge); ctl('dockBlade',edge)
    slots('left',[]); slots('right',[])
    slots('left',[{'id':'r15-files','modules':[{'module':'files'}]}]); slots('right',[{'id':'r15-notes','modules':[{'module':'notes'}]}])
    ctl('setBladeWidth','left','380'); ctl('setBladeWidth','right','360'); ctl('setMonitorMode','active',''); ctl('setRoot',fixture)
    run('ssh',f"setsid foot --title=FileBladeParity-{shape} sh -c 'stty -icanon -echo; cat > \"$1\"' sh {fixture}/keys >/dev/null 2>&1 </dev/null &")
    wait(lambda:any(w.get('initialTitle')=='FileBladeParity-'+shape for w in json.loads(run('hypr','clients'))))
    sink=next(w['pid'] for w in json.loads(run('hypr','clients')) if w.get('initialTitle')=='FileBladeParity-'+shape)
    time.sleep(.7); records['closed']=capture('closed')
    ctl('focusBlade','left'); time.sleep(.8); records['left']=capture('left')
    check(records['left']['status']['focusedBlade']=='left','E-43-01 left focus acquired')
    check(records['left']['reserved']==[380,26,0,0],'E-43-02 left exclusive zone')
    run('key','slash'); run('type','needle'); wait(lambda:status()['searchQuery']=='needle'); records['typing']=capture('typing')
    check(run('ssh',f'stat -c %s {fixture}/keys')=='0','E-43-01 blade captures typed input')
    ctl('clearSearch'); run('ssh','democtl click --output Virtual-1 960 500'); run('key','x'); time.sleep(.7)
    check(run('ssh',f'cat {fixture}/keys')=='x','E-43-01 ordinary window receives input after click')
    records['released']=capture('released')
    ctl('focusBlade','right'); time.sleep(.8); records['both']=capture('both')
    check(records['both']['status']['focusedBlade']=='right','E-43-01 right focus acquired')
    check(records['both']['reserved']==[380,26,360,0],'E-43-02 both exclusive zones')
    bar_hidden(True); time.sleep(1)
    records['bar_hidden']=capture('bar-hidden')
    check(records['bar_hidden']['reserved'][1]==0,'E-43-03 real bar releases its zone while hidden')
    check(sorted(row['namespace'] for row in records['bar_hidden']['layers'])==['omarchy-fileblade-left','omarchy-fileblade-right'],'E-43-03 both blade surfaces are present')
    check(all(row['y']==0 and row['h']==1080 for row in records['bar_hidden']['layers']),'E-43-03 compositor extends blades to the top edge')
    bar_hidden(False); time.sleep(1)
    records['bar_restored']=capture('bar-restored')
    ctl('closeBlade','left'); ctl('closeBlade','right'); time.sleep(.8); records['closed_again']=capture('closed-again')
    check(records['closed_again']['reserved']==records['closed']['reserved'],'E-43-02 closing returns all blade reservations')
    (out/'results.json').write_text(json.dumps(records,indent=2))
    print(json.dumps({'ok':True,'shape':shape,'evidence':str(out)}),flush=True)
finally:
    bar_hidden(initial_hidden)
    if sink: run('ssh',f'kill -TERM {sink}')
    slots('left',[]); slots('right',[]); ctl('setRoot',original_root)
    for edge in ['left','right']:
        slots(edge,original[edge]['slots']); ctl('setBladeWidth',edge,str(original[edge]['width'])); ctl('openBlade' if original[edge]['open'] else 'closeBlade',edge)
    run('ssh',f'rm -f {fixture}/needle.txt {fixture}/other.txt {fixture}/keys; rmdir {fixture} 2>/dev/null || true')
PY
