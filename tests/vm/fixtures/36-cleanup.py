import hashlib
import json
import os
from pathlib import Path
import shutil
import stat
import subprocess
import sys
import tempfile
import uuid

action, config, backup, *arguments = sys.argv[1:]
config, backup = Path(config), Path(backup)
plugin = Path(arguments[0]) if arguments else None
manifest = backup / "manifest.json"


def lstat(path):
    try:
        return path.lstat()
    except FileNotFoundError:
        return None


def file_info(path):
    info = lstat(path)
    if info is None:
        return None
    if not stat.S_ISREG(info.st_mode):
        raise RuntimeError("refusing non-regular fixture document: " + str(path))
    return {"mode": stat.S_IMODE(info.st_mode), "atime_ns": info.st_atime_ns, "mtime_ns": info.st_mtime_ns}


def directory_info(path, label):
    info = lstat(path)
    if info is None:
        return None
    if stat.S_ISLNK(info.st_mode) or not stat.S_ISDIR(info.st_mode):
        raise RuntimeError("refusing unexpected " + label + " root: " + str(path))
    return info


def validate_path(path, label):
    if not path.is_absolute() or "\0" in str(path):
        raise RuntimeError("refusing unexpected " + label + " path: " + str(path))
    for parent in reversed(path.parents):
        info = lstat(parent)
        if info is not None and (stat.S_ISLNK(info.st_mode) or not stat.S_ISDIR(info.st_mode)):
            raise RuntimeError("refusing unexpected " + label + " path: " + str(path))


def set_file_metadata(path, info):
    os.chmod(path, info["mode"], follow_symlinks=False)
    os.utime(path, ns=(info["atime_ns"], info["mtime_ns"]), follow_symlinks=False)


def file_digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def atomic_replace(path, data, mode=None):
    fd, temporary = tempfile.mkstemp(prefix="." + path.name + ".", dir=path.parent)
    temporary = Path(temporary)
    try:
        with os.fdopen(fd, "wb") as stream:
            stream.write(data)
        if mode is not None:
            os.chmod(temporary, mode)
        os.replace(temporary, path)
    finally:
        temporary.unlink(missing_ok=True)


def atomic_json(path, value):
    atomic_replace(path, json.dumps(value).encode(), 0o600)


def atomic_symlink(path, target):
    temporary = path.parent / ("." + path.name + "." + uuid.uuid4().hex)
    try:
        os.symlink(target, temporary)
        os.replace(temporary, path)
    finally:
        temporary.unlink(missing_ok=True)


def atomic_copy(saved, path, info, digest):
    if file_info(saved) is None:
        raise RuntimeError("missing saved fixture document: " + str(saved))
    if file_digest(saved) != digest:
        raise RuntimeError("saved fixture bytes changed: " + str(saved))
    set_file_metadata(saved, info)
    fd, temporary = tempfile.mkstemp(prefix="." + path.name + ".", dir=path.parent)
    temporary = Path(temporary)
    try:
        with saved.open("rb") as source, os.fdopen(fd, "wb") as stream:
            shutil.copyfileobj(source, stream)
        shutil.copystat(saved, temporary, follow_symlinks=False)
        set_file_metadata(temporary, info)
        os.replace(temporary, path)
    finally:
        temporary.unlink(missing_ok=True)


def remove_absent(path):
    info = lstat(path)
    if info is None:
        return
    if not stat.S_ISREG(info.st_mode) and not stat.S_ISLNK(info.st_mode):
        raise RuntimeError("refusing to remove unexpected fixture path: " + str(path))
    path.unlink()


def xdg_home(variable, fallback):
    value = os.environ.get(variable)
    path = Path(value) if value else Path.home() / fallback
    if not path.is_absolute() or "\0" in str(path):
        raise RuntimeError(variable + " must be an absolute path")
    return Path(os.path.normpath(path))


def isolated_roots():
    if os.environ.get("FILEBLADE_NATIVE_STATE_ROOT"):
        raise RuntimeError("isolate requires the plugin storage roots")
    data = xdg_home("XDG_DATA_HOME", ".local/share")
    state = xdg_home("XDG_STATE_HOME", ".local/state")
    roots = [("trash", data / "Trash"), ("artifact-bin", data / "fileblade/bin"), ("recovery", state / "fileblade")]
    for _, path in roots:
        validate_path(path, "storage")
    for index, (_, path) in enumerate(roots):
        if any(path == other or path in other.parents or other in path.parents for _, other in roots[index + 1:]):
            raise RuntimeError("storage roots overlap")
    return roots


def unique_path(parent, prefix):
    for _ in range(100):
        path = parent / ("." + prefix + "." + uuid.uuid4().hex)
        if lstat(path) is None:
            return path
    raise RuntimeError("could not allocate a unique fixture path")


def empty_root(path, label):
    path.mkdir(mode=0o700, parents=True)
    os.chmod(path, 0o700, follow_symlinks=False)
    if label == "trash":
        for name in ("info", "files"):
            child = path / name
            child.mkdir(mode=0o700)
            os.chmod(child, 0o700, follow_symlinks=False)


def read_journal():
    value = json.loads(manifest.read_text())
    if not isinstance(value, dict) or not isinstance(value.get("documents"), list):
        raise RuntimeError("invalid fixture journal")
    value.setdefault("isolation", [])
    value.setdefault("instrumentation", [])
    return value


def write_journal(journal):
    atomic_json(manifest, journal)


def document_paths():
    expected_config = xdg_home("XDG_CONFIG_HOME", ".config") / "omarchy/fileblade"
    if config != expected_config:
        raise RuntimeError("unexpected config root: " + str(config))
    validate_path(config, "config")
    if directory_info(config, "config") is None:
        raise RuntimeError("missing config root: " + str(config))
    state = xdg_home("XDG_STATE_HOME", ".local/state")
    paths = [config / name for name in ("blades.json", "settings.json", "keybindings.json")]
    paths.append(state / "omarchy/fileblade/state.json")
    for path in paths:
        validate_path(path, "config document")
    return paths


def backup_documents():
    records = []
    for index, path in enumerate(document_paths()):
        info = file_info(path)
        saved = backup / str(index)
        digest = file_digest(path) if info is not None else None
        if info is not None:
            set_file_metadata(path, info)
            shutil.copy2(path, saved, follow_symlinks=False)
            if file_digest(saved) != digest:
                raise RuntimeError("saved fixture bytes mismatch: " + str(saved))
            set_file_metadata(saved, info)
            set_file_metadata(path, info)
        records.append({"path": str(path), "saved": str(saved), "exists": info is not None, "info": info, "sha256": digest})
    write_journal({"documents": records, "isolation": [], "instrumentation": []})


def restore_documents(journal):
    for record in journal["documents"]:
        path, saved = Path(record["path"]), Path(record["saved"])
        if record["exists"]:
            atomic_copy(saved, path, record["info"], record["sha256"])
            matches = file_digest(path) == record["sha256"]
            set_file_metadata(path, record["info"])
            set_file_metadata(saved, record["info"])
            if not matches or file_info(path) != record["info"]:
                raise RuntimeError("fixture restore mismatch: " + str(path))
        else:
            remove_absent(path)


def prepare_isolation(journal):
    if journal["isolation"]:
        raise RuntimeError("storage isolation is already journaled")
    records = []
    for label, path in isolated_roots():
        info = directory_info(path, label)
        archive = unique_path(path.parent, "fileblade-e36-" + label) if info is not None else None
        records.append({"label": label, "path": str(path), "original": str(archive) if archive else None,
                        "identity": {"dev": info.st_dev, "ino": info.st_ino} if info is not None else None,
                        "existed": info is not None, "moved": False, "generated": None,
                        "restored": False})
    journal["isolation"] = records
    write_journal(journal)
    for record in records:
        path = Path(record["path"])
        if record["existed"]:
            os.rename(path, record["original"])
            record["moved"] = True
            write_journal(journal)
        empty_root(path, record["label"])


def move_generated(record, journal):
    path = Path(record["path"])
    info = directory_info(path, record["label"])
    generated = Path(record["generated"]) if record["generated"] else unique_path(path.parent, "fileblade-e36-generated-" + record["label"])
    if generated.parent != path.parent or not generated.name.startswith(".fileblade-e36-generated-" + record["label"] + "."):
        raise RuntimeError("refusing unexpected generated " + record["label"] + " path: " + str(generated))
    if info is None:
        if record["generated"] and lstat(generated) is None:
            raise RuntimeError("missing generated " + record["label"] + " root: " + str(generated))
        return
    if lstat(generated) is not None:
        raise RuntimeError("generated " + record["label"] + " archive already exists: " + str(generated))
    record["generated"] = str(generated)
    write_journal(journal)
    os.rename(path, generated)


def restore_isolation(journal):
    expected = dict(isolated_roots()) if journal["isolation"] else {}
    for record in journal["isolation"]:
        if record["restored"]:
            continue
        path = Path(record["path"])
        if expected.get(record["label"]) != path:
            raise RuntimeError("refusing unexpected " + record["label"] + " root: " + str(path))
        validate_path(path, "storage")
        original = Path(record["original"]) if record["original"] else None
        if original is not None:
            if original.parent != path.parent or not original.name.startswith(".fileblade-e36-" + record["label"] + "."):
                raise RuntimeError("refusing unexpected saved " + record["label"] + " path: " + str(original))
            validate_path(original, "saved storage")
        source_info = directory_info(path, record["label"])
        original_info = directory_info(original, "saved " + record["label"]) if original else None
        if source_info is not None:
            if record["existed"] and original_info is None and not record["moved"]:
                record["restored"] = True
                write_journal(journal)
                continue
            if record["existed"] and original_info is None and record["moved"] and {
                "dev": source_info.st_dev, "ino": source_info.st_ino
            } == record["identity"]:
                record["restored"] = True
                write_journal(journal)
                continue
            move_generated(record, journal)
        if record["existed"]:
            if original_info is not None:
                os.rename(original, path)
            elif not record["moved"]:
                raise RuntimeError("missing saved " + record["label"] + " root: " + str(original))
            elif lstat(path) is None:
                raise RuntimeError("missing restored " + record["label"] + " root: " + str(path))
        record["restored"] = True
        write_journal(journal)


def instrumentation_paths(root):
    validate_path(root, "plugin")
    if directory_info(root, "plugin") is None:
        raise RuntimeError("missing plugin root: " + str(root))
    if file_info(root / "manifest.json") is None or directory_info(root / "modules", "plugin modules") is None:
        raise RuntimeError("unexpected plugin root: " + str(root))
    paths = [(module, root / "modules" / module / "blades/Module.qml") for module in ("skills", "memory", "hooks", "mcp")]
    for _, path in paths:
        validate_path(path, "plugin module")
    return paths


def prepare_instrumentation(journal, root):
    if journal["instrumentation"]:
        raise RuntimeError("module instrumentation is already journaled")
    destination = backup / "instrumentation"
    destination.mkdir(mode=0o700)
    records = []
    for module, path in instrumentation_paths(root):
        info = file_info(path)
        source = path.read_bytes()
        digest = hashlib.sha256(source).hexdigest()
        set_file_metadata(path, info)
        if "tests/core_modules/LiveProbe.qml" in source.decode(errors="replace"):
            raise RuntimeError("module is already instrumented: " + str(path))
        saved = destination / (module + ".qml")
        records.append({"module": module, "path": str(path), "saved": str(saved), "info": info,
                        "sha256": digest, "savedCopy": False, "applied": False, "restored": False})
    journal["instrumentation"] = records
    write_journal(journal)
    for record in records:
        path, saved = Path(record["path"]), Path(record["saved"])
        shutil.copy2(path, saved, follow_symlinks=False)
        if file_digest(saved) != record["sha256"]:
            raise RuntimeError("saved module bytes mismatch: " + str(saved))
        set_file_metadata(saved, record["info"])
        set_file_metadata(path, record["info"])
        record["savedCopy"] = True
        write_journal(journal)
    for record in records:
        path, saved = Path(record["path"]), Path(record["saved"])
        if file_digest(saved) != record["sha256"]:
            raise RuntimeError("saved module bytes changed: " + str(saved))
        source = path.read_text()
        subject = "root" if record["module"] == "mcp" else "module"
        fragment = '\n  Loader { source: Qt.resolvedUrl("../../../tests/core_modules/LiveProbe.qml"); onLoaded: item.subject = ' + subject + ' }\n'
        atomic_replace(path, (source[:source.rfind("}")] + fragment + "}\n").encode(), record["info"]["mode"])
        record["applied"] = True
        write_journal(journal)


def restore_instrumentation(journal):
    for record in journal["instrumentation"]:
        if record["restored"]:
            continue
        saved, path = Path(record["saved"]), Path(record["path"])
        if not record["savedCopy"]:
            if record["applied"]:
                raise RuntimeError("missing module backup: " + str(saved))
            record["restored"] = True
            write_journal(journal)
            continue
        atomic_copy(saved, path, record["info"], record["sha256"])
        if file_digest(path) != record["sha256"]:
            raise RuntimeError("module restore mismatch: " + str(path))
        set_file_metadata(path, record["info"])
        set_file_metadata(saved, record["info"])
        record["restored"] = True
        write_journal(journal)


def project_files(project):
    return {
        project / ".claude/skills/rivet-fixture/SKILL.md": b"---\nname: rivet-fixture\ndescription: Dedicated core integration fixture\n---\nFixture text.\n",
        project / "AGENTS.md": b"Core integration fixture instructions.\n",
        project / ".claude/settings.json": json.dumps({"hooks": {"PreToolUse": [{"hooks": [{"type": "command", "command": "printf RIVET_NOT_EXECUTED"}]}]}}).encode(),
        project / ".mcp.json": json.dumps({"mcpServers": {"rivet-fixture": {"command": "printf", "args": ["RIVET_NOT_EXECUTED"]}}}).encode(),
    }


def write_project(project):
    project.mkdir(mode=0o700, parents=True, exist_ok=True)
    for directory in (project / ".git", project / ".claude/skills/rivet-fixture"):
        validate_path(directory, "fixture project")
        if directory_info(directory, "fixture project") is None:
            directory.mkdir(mode=0o700, parents=True)
    for path, data in project_files(project).items():
        validate_path(path, "fixture project")
        atomic_replace(path, data, 0o600)


def seed_project(journal, root):
    project = backup / "project"
    if lstat(project) is not None:
        raise RuntimeError("fixture project already exists: " + str(project))
    write_project(project)
    if root is not None:
        prepare_instrumentation(journal, root)
    print(project)


def reseed_project(journal):
    project = backup / "project"
    if directory_info(project, "fixture project") is None:
        raise RuntimeError("missing fixture project: " + str(project))
    write_project(project)
    print(project)


def update_git(cwd, *args):
    environment = dict(
        os.environ,
        GIT_AUTHOR_NAME="t",
        GIT_AUTHOR_EMAIL="t@example.invalid",
        GIT_COMMITTER_NAME="t",
        GIT_COMMITTER_EMAIL="t@example.invalid",
    )
    result = subprocess.run(
        ["git", *args],
        cwd=cwd,
        env=environment,
        check=True,
        capture_output=True,
        text=True,
    )
    return result.stdout.strip()


def update_manifest(version):
    return (
        json.dumps(
            {
                "schemaVersion": 1,
                "id": "t.plugin",
                "name": "T",
                "version": version,
                "kinds": ["service"],
                "entryPoints": {"service": "Service.qml"},
            },
            separators=(",", ":"),
        )
        + "\n"
    ).encode()


def update_fixture():
    source = backup / "update-source"
    checkout = backup / "update-checkout"
    for path, label in ((source, "update source"), (checkout, "update checkout")):
        if lstat(path) is not None:
            raise RuntimeError(f"{label} already exists")
    source.mkdir(mode=0o700)
    update_git(source, "init", "-q", "-b", "main")
    atomic_replace(source / "manifest.json", update_manifest("1.0.0"), 0o644)
    atomic_replace(
        source / "Service.qml",
        b"import QtQuick\nItem {}\n",
        0o644,
    )
    update_git(source, "add", "manifest.json", "Service.qml")
    update_git(source, "commit", "-q", "-m", "first")
    update_git(backup, "clone", "-q", "--no-local", str(source), str(checkout))
    atomic_replace(source / "manifest.json", update_manifest("1.1.0"), 0o644)
    update_git(source, "add", "manifest.json")
    update_git(source, "commit", "-q", "-m", "second")
    payload = {
        "repository": "t.plugin",
        "source": str(source),
        "checkout": str(checkout),
        "origin": update_git(checkout, "config", "--get", "remote.origin.url"),
        "source_head": update_git(source, "rev-parse", "HEAD"),
        "checkout_head": update_git(checkout, "rev-parse", "HEAD"),
    }
    atomic_json(backup / "updates.json", payload)
    print(json.dumps(payload))


if Path.home() != Path("/home/omarchy"):
    raise RuntimeError("run only in the assigned Omarchy test guest")
if len(arguments) > 1:
    raise RuntimeError("expected at most one plugin root")

if action == "backup":
    backup_documents()
elif action == "isolate":
    journal = read_journal()
    prepare_isolation(journal)
elif action == "seed":
    journal = read_journal()
    seed_project(journal, plugin)
elif action == "seed-project":
    journal = read_journal()
    reseed_project(journal)
elif action == "updates":
    update_fixture()
elif action == "unanswered":
    for name, document in (
        ("settings.json", {"version": 1, "fixture": "E36"}),
        ("keybindings.json", {"version": 1, "bindings": {"next": ["n"]}}),
    ):
        atomic_replace(config / name, json.dumps(document).encode(), 0o600)
elif action == "fresh":
    remove_absent(config / "settings.json")
elif action == "rejected":
    denied = backup / "denied.json"
    if lstat(denied) is not None:
        raise RuntimeError("denied settings fixture already exists: " + str(denied))
    atomic_replace(denied, b'{"version":1,"fixture":"E36 denied"}', 0o400)
    atomic_symlink(config / "settings.json", denied)
    print(denied)
elif action == "restore":
    journal = read_journal()
    restore_instrumentation(journal)
    restore_isolation(journal)
    restore_documents(journal)
else:
    raise RuntimeError("unknown fixture action")
