#!/usr/bin/env bash
set -euo pipefail
repo=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/../../.." && pwd -P)
python3 - "$repo" <<'PY'
import base64,json,pathlib,subprocess,sys,time
repo=pathlib.Path(sys.argv[1]); ovm=str(repo/'app/ovm-spike'); out=repo/'.claude/evidence/sootscale/r15/layout'; out.mkdir(parents=True,exist_ok=True)
app='/home/omarchy/fileblade-runtime-spike/app'
def run(*args): return subprocess.check_output([ovm,*map(str,args)],text=True,timeout=15).strip()
def ipc(target,method,*args):
    import shlex
    return run('ssh',shlex.join(['qs','-p',app,'ipc','call',target,method,*map(str,args)]))
def ctl(method,*args): return ipc('data-goblin.fileblade.control',method,*args)
def blades(): return json.loads(ipc('data-goblin.fileblade','blades'))['blades']
def probe(): return json.loads(ipc('fileblade.qualification','status'))
def slots(edge,rows): return ctl('setBladeSlots',edge,'base64:'+base64.b64encode(json.dumps(rows).encode()).decode())
def check(value,label):
    assert value,label
    print('ok '+label,flush=True)
def wait(predicate):
    deadline=time.monotonic()+12
    while True:
        if predicate(): return
        assert time.monotonic()<deadline,probe()
        time.sleep(.1)
def snap(name):
    (out/(name+'.json')).write_text(json.dumps({'probe':probe(),'blades':blades()},indent=2))
    source=pathlib.Path(run('shot','r15-layout-'+name).splitlines()[-1]); (out/(name+'.png')).write_bytes(source.read_bytes())
def slot(edge,index): return next(s for s in probe()['slots'] if s['edge']==edge and s['index']==index)
def names(edge): return [[t['module'] for t in s['modules']] for s in blades()[edge]['slots']]
def point(s,dx=35,dy=16): return (s['geometry']['x']+dx,s['geometry']['y']+dy)
def pointer(command): run('ssh',"printf '%s\\n' '"+command+"' > /tmp/fileblade-r15-pointer.fifo")
def move(p): pointer(f'move {round(p[0])} {round(p[1])}')
def click(p): run('mouse','click',round(p[0]),round(p[1])); time.sleep(.7)
def start_drag(a,b):
    move(a); pointer('down'); move((a[0]+12,a[1]+8)); time.sleep(.15); move(b); time.sleep(.7)
    wait(lambda:probe()['drag']['active'])
def release(): pointer('up'); time.sleep(.8)
original=blades(); original_root=json.loads(ipc('data-goblin.fileblade','status'))['rootPath']
fixture='/home/omarchy/fileblade-r15-layout'
failures=[]
run('ssh','test -p /tmp/fileblade-r15-pointer.fifo || mkfifo -m 600 /tmp/fileblade-r15-pointer.fifo; nohup /tmp/fileblade-qualification-pointer /tmp/fileblade-r15-pointer.fifo 1920 1080 >/tmp/fileblade-r15-pointer.log 2>&1 </dev/null &')
try:
    slots('left',[]); slots('right',[])
    slots('left',[{'id':'r15-'+name,'modules':[{'module':name}]} for name in ['files','notes','properties']])
    slots('right',[{'id':'r15-welcome','modules':[{'module':'welcome'}]}])
    ctl('openBlade','left'); ctl('openBlade','right'); time.sleep(1)
    s=slot('left',1); click(point(s,10))
    check(slot('left',1)['collapsed'],'E-20-01 caret collapses section')
    collapsed_height=slot('left',1)['geometry']['height']
    ctl('closeBlade','left'); ctl('openBlade','left'); time.sleep(1)
    check(slot('left',1)['collapsed'],'E-20-02 collapse survives reopen')
    click(point(slot('left',1),10))
    check(not slot('left',1)['collapsed'] and slot('left',1)['geometry']['height']>collapsed_height,'E-20-01 caret expands section')
    s=slot('left',1); last=slot('left',2)['geometry']; target=(last['x']+160,last['y']+last['height']-4)
    start_drag(point(s),target)
    check(any(s['dropVisible'] for s in probe()['stacks']),'E-20-06 vertical insertion line is shown')
    snap('vertical-insertion'); release()
    check(names('left')==[['files'],['properties'],['notes']],'E-20-03 section moves below another section')
    source=slot('left',2); target=point(slot('right',0),160,2)
    start_drag(point(source),target); snap('cross-blade'); release()
    check(names('right')==[['notes'],['welcome']],'E-20-04 section moves to other blade')
    source=slot('right',0); dest=slot('right',1)['geometry']
    start_drag(point(source),(dest['x']+150,dest['y']+dest['height']/2))
    check(probe()['drag']['tabBand']=='body','E-20-07 target section is outlined for a tab drop')
    snap('body-outline'); release()
    check(names('right')==[['welcome','notes']],'E-20-05 module joins another section as a tab')
    row=next(t for t in probe()['tabs'] if t['edge']=='right' and t['slot']==0)
    source=row['entries'][1]['geometry']; dest=row['entries'][0]['geometry']
    start_drag((source['x']+source['width']/2,source['y']+16),(dest['x']+2,dest['y']+16))
    check(probe()['drag']['tabBand']=='tabs' and probe()['drag']['tabIndex']==0,'E-20-08 tab insertion targets the first position')
    snapshot=probe()
    def overlaps(a,b): return a['x']<b['x']+b['width'] and b['x']<a['x']+a['width'] and a['y']<b['y']+b['height'] and b['y']<a['y']+a['height']
    obscured=any(overlaps(card,tab['geometry']) for card in snapshot['dragCards'] for row in snapshot['tabs'] if row['dropIndex']>=0 for tab in row['entries'])
    if obscured:
        failures.append('E-20-08 drag card obscures tab titles')
        print('FAIL '+failures[-1],flush=True)
    else: check(True,'E-20-08 existing tab titles remain unobscured')
    snap('tab-insertion'); release()
    check(names('right')==[['notes','welcome']],'E-20-05 tabs reorder within their row')
    before=names('right')
    row=next(t for t in probe()['tabs'] if t['edge']=='right' and t['slot']==0); source=row['entries'][0]['geometry']
    start_drag((source['x']+source['width']/2,source['y']+16),(960,500))
    check(not probe()['drag']['edge'],'E-20-09 outside drop has no target'); snap('outside-target'); release()
    check(names('right')==before,'E-20-09 outside drop preserves layout')
    ctl('closeBlade','right'); ctl('openBlade','right'); time.sleep(1)
    check(names('right')==before,'E-20-10 arranged tabs survive reopen')
    upper=slot('left',0)['geometry']; lower=slot('left',1)['geometry']; initial=upper['height']
    a=(upper['x']+180,lower['y']-3); move(a); pointer('down'); move((a[0],a[1]+65)); time.sleep(.4); release()
    grown=slot('left',0)['geometry']['height']
    check(grown>initial+40,'E-20-12 divider resizes adjacent sections')
    snap('resized'); ctl('closeBlade','left'); ctl('openBlade','left'); time.sleep(1)
    check(abs(slot('left',0)['geometry']['height']-grown)<=2,'E-20-12 divider sizes survive reopen')
    run('ssh',f'mkdir -p {fixture}; printf keep > {fixture}/before.txt')
    ctl('setRoot',fixture); time.sleep(.8); ctl('select',fixture+'/before.txt'); ctl('renameSelection','after.txt')
    wait(lambda:run('ssh',f'test -f {fixture}/after.txt && echo yes || echo no')=='yes')
    saved=names('right'); ctl('focusBlade','left'); run('key','ctrl-z'); time.sleep(1)
    check(run('ssh',f'test -f {fixture}/before.txt && test ! -e {fixture}/after.txt && echo yes || echo no')=='yes','E-20-11 Ctrl+Z undoes the file operation')
    check(names('right')==saved,'E-20-11 Ctrl+Z leaves arranged layout intact')
    snap('undo-file-only')
    print(json.dumps({'ok':not failures,'failures':failures,'expectations':[f'E-20-{i:02}' for i in range(1,13)],'evidence':str(out)}),flush=True)
finally:
    pointer('up')
    ctl('setRoot',original_root)
    slots('left',[]); slots('right',[])
    for edge in ['left','right']:
        slots(edge,original[edge]['slots']); ctl('openBlade' if original[edge]['open'] else 'closeBlade',edge)
    pointer('quit')
    run('ssh',f'rm -f {fixture}/before.txt {fixture}/after.txt; rmdir {fixture} 2>/dev/null || true')
if failures: raise SystemExit(1)
PY
