#!/usr/bin/env bash
set -euo pipefail
repo=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/../../.." && pwd -P)
ovm=$repo/app/ovm-spike
payload=$(base64 -w0 <<'PY'
import base64, json, pathlib, subprocess, time
app='/home/omarchy/fileblade-runtime-spike/app'
out=pathlib.Path('/tmp/fileblade-r15-importers'); out.mkdir(exist_ok=True)
def ipc(target,method,*args):
    return subprocess.check_output(['qs','-p',app,'ipc','call',target,method,*args],text=True,timeout=8).strip()
def ctl(method,*args): return ipc('data-goblin.fileblade.control',method,*args)
def probe(): return json.loads(ipc('fileblade.qualification','status'))
def slots(edge,rows): return ctl('setBladeSlots',edge,'base64:'+base64.b64encode(json.dumps(rows).encode()).decode())
def shot(name): subprocess.run(['grim','-o','Virtual-1',str(out/(name+'.png'))],check=True,timeout=5)
def wait(check):
    end=time.monotonic()+12
    while True:
        value=probe()
        if check(value): return value
        assert time.monotonic()<end,value
        time.sleep(.1)
original=json.loads(ipc('data-goblin.fileblade','blades'))['blades']
results=[]
try:
    slots('right',[])
    ctl('closeBlade','right')
    for name in ['files','properties','notes','welcome','skills','memory','hooks','mcp']:
        slots('left',[{'id':'r15-'+name,'modules':[{'module':name}]}])
        ctl('focusBlade','left')
        state=wait(lambda p: any(s['module']==name and s['loaded'] and not s['failed'] and not s['providerError'] for s in p['slots']))
        time.sleep(.7)
        shot(name)
        results.append({'module':name,'snapshot':state})
        (out/'core-results.json').write_text(json.dumps(results,indent=2))
    ipc('fileblade.qualification','goblins','true')
    wait(lambda p:p['barLoaded'] and not p['fixtureError'])
    subprocess.run(['democtl','click','--output','Virtual-1','920','13'],check=True,timeout=5)
    state=wait(lambda p:p['barOpened'] and any(s['module']=='kurt.goblin-images/goblin-images' and s['loaded'] and not s['notice'] for s in p['popouts']))
    time.sleep(1)
    shot('goblins-popout')
    results.append({'module':'Goblins BarWidget/KeyboardPanel','snapshot':state})
    subprocess.run(['wtype','-k','Escape'],check=True,timeout=5)
    wait(lambda p:not p['barOpened'])
    print(json.dumps({'ok':True,'expectations':['E-41-01','E-41-02'],'results':results,'evidence':str(out)}))
finally:
    ipc('fileblade.qualification','goblins','false')
    for edge in ['left','right']:
        slots(edge,original[edge]['slots'])
        ctl('openBlade' if original[edge]['open'] else 'closeBlade',edge)
PY
)
"$ovm" ssh "printf %s '$payload' | base64 -d | python3"
