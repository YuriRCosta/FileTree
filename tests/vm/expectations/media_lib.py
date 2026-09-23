import base64
import json
import os
from pathlib import Path
import shlex
import subprocess
import time

repo = Path(__file__).resolve().parents[3]
ovm_path = os.environ['OVM']
checks = 0


def ovm(*args):
    result = subprocess.run([ovm_path, *map(str, args)], text=True, capture_output=True, timeout=45)
    if result.returncode:
        raise RuntimeError(result.stdout + result.stderr)
    return result.stdout.strip()


def ipc(target, *args):
    return ovm('ssh', shlex.join(['omarchy-shell', target, *map(str, args)]))


def control(*args):
    return ipc('yuricosta.filetree.control', *args)


def probe(*args):
    return ipc('brindle-media-left', *args)


def state():
    return json.loads(probe('state'))


def wait(predicate):
    deadline = time.monotonic() + 20
    last = None
    while time.monotonic() < deadline:
        last = state()
        if predicate(last):
            return last
        time.sleep(.1)
    raise AssertionError(last)


def check(label, condition, observed):
    global checks
    if not condition:
        raise AssertionError((label, observed))
    checks += 1
    print(json.dumps({'action': label, 'expected': True, 'observed': observed}), flush=True)


def shot(name):
    print(json.dumps({'shot': ovm('shot', 'brindle-' + name)}), flush=True)
