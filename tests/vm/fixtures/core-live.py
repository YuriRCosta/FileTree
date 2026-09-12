import json
from pathlib import Path
import shutil
import sys

root = Path(__file__).resolve().parents[3]
state = Path('/tmp/rivet-core-live')
modules = ('skills', 'memory', 'hooks', 'mcp')

if Path.home() != Path('/home/omarchy'):
    raise SystemExit('Run only in the assigned Omarchy test guest')

if sys.argv[1] == 'prepare':
    state.mkdir(mode=0o700, exist_ok=True)
    project = state / 'project'
    if project.exists():
        raise SystemExit('Fixture already exists; restore it before preparing again')
    for directory in ('.git', '.claude/skills/rivet-fixture'):
        (project / directory).mkdir(parents=True)
    (project / '.claude/skills/rivet-fixture/SKILL.md').write_text('---\nname: rivet-fixture\ndescription: Dedicated core integration fixture\n---\nFixture text.\n')
    (project / 'AGENTS.md').write_text('Core integration fixture instructions.\n')
    (project / '.claude/settings.json').write_text(json.dumps({'hooks': {'PreToolUse': [{'hooks': [{'type': 'command', 'command': 'printf RIVET_NOT_EXECUTED'}]}]}}))
    (project / '.mcp.json').write_text(json.dumps({'mcpServers': {'rivet-fixture': {'command': 'printf', 'args': ['RIVET_NOT_EXECUTED']}}}))
    for module in modules:
        path = root / 'modules' / module / 'blades/Module.qml'
        backup = state / (module + '-Module.qml')
        if backup.exists():
            raise SystemExit('Existing instrumentation backup: ' + str(backup))
        shutil.copy2(path, backup)
        source = path.read_text()
        subject = 'root' if module == 'mcp' else 'module'
        fragment = '\n  Loader { source: Qt.resolvedUrl("../../../tests/core_modules/LiveProbe.qml"); onLoaded: item.subject = ' + subject + ' }\n'
        path.write_text(source[:source.rfind('}')] + fragment + '}\n')
    print(project)
elif sys.argv[1] == 'restore':
    for module in modules:
        backup = state / (module + '-Module.qml')
        if backup.exists():
            shutil.copy2(backup, root / 'modules' / module / 'blades/Module.qml')
            backup.unlink()
    print('Module instrumentation restored; fixture and evidence retained')
else:
    raise SystemExit('Expected prepare or restore')
