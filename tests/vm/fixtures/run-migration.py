import json
import os
from pathlib import Path
import subprocess
import tempfile

root = Path(__file__).resolve().parents[3]
if Path.home() != Path('/home/omarchy'):
    raise SystemExit('Run only in the assigned Omarchy test guest')
binary = root / 'target/release/rivet-migration-prepare'
with tempfile.TemporaryDirectory(prefix='rivet-migration-') as temporary:
    base = Path(temporary)
    tools = base / 'tools'
    tools.mkdir()
    for name, script in [('omarchy', '#!/bin/sh\nprintf "%s\\n" "$MIGRATION_ACTIVATION"\n'),
                         ('qs', '#!/bin/sh\nprintf "%s\\n" "Target not found."\n')]:
        path = tools / name
        path.write_text(script)
        path.chmod(0o700)
    shell = base / 'omarchy/shell'
    shell.mkdir(parents=True)
    (shell / 'shell.qml').write_text('')
    for scenario in ('stopped', 'active', 'malformed', 'newer', 'missing-recovery'):
        destination = base / scenario
        subprocess.run(['python3', '-B', str(Path(__file__).with_name('migration.py')),
                        str(destination), '--scenario', scenario], check=True, stdout=subprocess.DEVNULL)
        fixture = json.loads((destination / 'fixture.json').read_text())
        home = destination / 'legacy-home'
        environment = dict(os.environ, HOME=str(home), XDG_CONFIG_HOME=str(home / '.config'),
                           XDG_DATA_HOME=str(home / '.local/share'), OMARCHY_PATH=str(base / 'omarchy'),
                           PATH=str(tools) + ':/usr/bin', XDG_STATE_HOME=str(home / '.local/state'), XDG_CACHE_HOME=str(home / '.cache'), FILEBLADE_APP_ROOT=str(root), MIGRATION_GENERATED_FIXTURE=str(destination / 'fixture.json'))
        environment['MIGRATION_ACTIVATION'] = json.dumps([{'id': provider, 'enabled': scenario == 'active'} for provider in ['data-goblin.fileblade', *['data-goblin.fileblade-' + module for module in ('skills', 'memory', 'hooks', 'mcp')]]] + [{'id': 'data-goblin.goblins', 'enabled': True}])
        subprocess.run([str(binary), '--exact', 'generated_fixture_worker', '--nocapture'], env=environment, check=True)
