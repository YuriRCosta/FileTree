use fileblade::lease::Authority;
use fileblade::migration::{Roots, Status, prepare};
use serde_json::json;
use std::fs;
use std::os::unix::fs::{PermissionsExt, symlink};
use std::path::{Path, PathBuf};
use std::process::Command;
use xattr::FileExt;

fn write(path: &Path, bytes: impl AsRef<[u8]>) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, bytes).unwrap();
    fs::set_permissions(path, fs::Permissions::from_mode(0o600)).unwrap();
}

fn roots(path: &Path) -> Roots {
    Roots {
        config: path.join("config/omarchy/fileblade"),
        state: path.join("state"),
        recovery: path.join("recovery"),
    }
}

fn manifest(module: &str, items: serde_json::Value) -> serde_json::Value {
    json!({"schemaVersion":1,"module":module,"id":"definition","name":"Fixture","kind":"","scope":"","detail":"","path":"","realpath":"","deletedAt":"2026-09-12 12:00","items":items})
}

fn damage_payload(scenario: &str, payload: &mut serde_json::Value) {
    let parts: Vec<_> = scenario.split('-').collect();
    if parts.len() != 4 {
        return;
    }
    let field = parts[2];
    if parts[3] == "missing" {
        payload.as_object_mut().unwrap().remove(field);
    } else {
        payload[field] = match field {
            "index" | "position" | "offset" => json!(-1),
            "before" | "after" | "definition" => json!("g".repeat(64)),
            "fields" => json!({"hooks": []}),
            "container" => json!(["foreign"]),
            "text" => json!("[mcp_servers.fixture]\ncommand='printf'\n[foreign]\nx=1\n"),
            "raw" | "entry" => json!([]),
            _ => json!(42),
        };
    }
}

fn invalid_native_document(scenario: &str, roots: &Roots) -> Option<(bool, PathBuf, Vec<u8>)> {
    let parts: Vec<_> = scenario.split('-').collect();
    if parts.len() != 3 || !["native", "completed"].contains(&parts[0]) {
        return None;
    }
    let (path, version) = match parts[1] {
        "state" => (roots.state.join("state.json"), 13),
        "settings" => (roots.config.join("settings.json"), 2),
        "layout" => (roots.config.join("blades.json"), 2),
        "keybindings" => (roots.config.join("keybindings.json"), 2),
        _ => return None,
    };
    let bytes = match parts[2] {
        "newer" => serde_json::to_vec(&json!({"version":version})).unwrap(),
        "malformed" => b"{ malformed original".to_vec(),
        "unsupported" => br#"{"version":0}"#.to_vec(),
        _ => return None,
    };
    Some((parts[0] == "completed", path, bytes))
}

#[test]
fn migration_worker() {
    let Ok(scenario) = std::env::var("MIGRATION_CASE") else {
        return;
    };
    let base = PathBuf::from(std::env::var_os("HOME").unwrap());
    let legacy = roots(&base.join("legacy"));
    let native = if scenario == "shared-roots" {
        legacy.clone()
    } else {
        roots(&base.join("native"))
    };
    let invalid_native = invalid_native_document(&scenario, &native);
    let state = br#"{"version":12,"welcomeState":"dismissed","future":{"keep":[1,2]}}"#;
    write(&legacy.state.join("state.json"), state);
    write(
        &legacy.config.join("settings.json"),
        br#"{"version":1,"agentManagement":true,"future":42}"#,
    );
    write(&legacy.config.join("blades.json"), br#"{"version":1,"blades":{"left":{"slots":[{"modules":[{"module":"data-goblin.fileblade-skills/skills","state":{"future":true}}]}]}}}"#);
    write(
        &legacy
            .state
            .join("modules/data-goblin.fileblade-skills+skills/opaque.bin"),
        [0, 255, 13, 10],
    );
    let record_id = "0123456789abcdef0123456789abcdef";
    let mut payload = json!({"format":2,"agent":"claude-code","event":"PreToolUse","source":"/fixture/settings.json","target":"/fixture/settings.json","entry":{"type":"command","command":"printf FIXTURE"},"fields":{},"index":0,"group":"","grouped":true,"removedGroup":true,"before":"9b131f3ff5d55b5097a131dfe25290eae7c0d58f68da0e13167ae7c7741ae2e4","after":"74234e98afe7498fb5daf1f36ac2d78acc339464f950703b8c019892f982b90b"});
    if scenario.starts_with("payload-hooks-") {
        damage_payload(&scenario, &mut payload);
    }
    write(&legacy.recovery.join(format!("hooks-recovery/{record_id}.json")), serde_json::to_vec(&json!({"formatVersion":1,"createdAt":1,"context":{},"transactionId":record_id,"definitionId":"definition","payload":payload})).unwrap());
    let bin = base.join("data/fileblade/bin");
    let mut hooks = manifest("hooks", json!([]));
    hooks["payload"] = payload;
    hooks["helperRecordId"] = record_id.into();
    hooks["restoreHelper"] =
        json!({"provider":"data-goblin.fileblade-hooks","helper":"inventory","directory":"legacy"});
    if scenario == "legacy-payload" {
        hooks.as_object_mut().unwrap().remove("helperRecordId");
    }
    if scenario == "external" {
        let mut external = manifest("goblins", json!([]));
        external["restoreHelper"] = json!({"provider":"fixture.external","directory":"/fixture/external","helper":"inventory"});
        external["helperRecordId"] = record_id.into();
        external["payload"] = json!({"future":true});
        write(
            &bin.join("goblins/entry/manifest.json"),
            serde_json::to_vec(&external).unwrap(),
        );
    }
    write(
        &bin.join("hooks/entry/manifest.json"),
        serde_json::to_vec(&hooks).unwrap(),
    );
    if scenario.starts_with("payload-json") || scenario.starts_with("payload-toml") {
        let mut payload = json!({"format":2,"agent":"claude-code","path":"/fixture/mcp.json","target":"/fixture/mcp.json","kind":"json","container":["mcpServers"],"name":"fixture","raw":{"command":"printf","args":["FIXTURE"]},"position":0,"definition":"ac39c42ab90004e9d90639689f8fc0995f4d28e90aeca3ed743fd036018763a0"});
        if scenario.starts_with("payload-toml") {
            payload["kind"] = json!("toml");
            payload["text"] = json!("[mcp_servers.fixture]\ncommand='printf'\nargs=['FIXTURE']\n");
            payload["offset"] = json!(0);
            payload["after"] =
                json!("e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855");
        }
        damage_payload(&scenario, &mut payload);
        let mut mcp = manifest("mcp", json!([]));
        mcp["payload"] = payload.clone();
        mcp["helperRecordId"] = record_id.into();
        mcp["restoreHelper"] = json!({"provider":"data-goblin.fileblade-mcp","helper":"inventory","directory":"legacy"});
        write(
            &bin.join("mcp/entry/manifest.json"),
            serde_json::to_vec(&mcp).unwrap(),
        );
        write(&legacy.recovery.join(format!("mcp-recovery/{record_id}.json")), serde_json::to_vec(&json!({"formatVersion":1,"createdAt":1,"context":{},"transactionId":record_id,"definitionId":"definition","payload":payload})).unwrap());
    }
    let skills = manifest(
        "skills",
        json!([{"from":"/fixture/link","mode":511,"size":0,"target":"../unavailable-target","stored":"items/0","type":"symlink"}]),
    );
    write(
        &bin.join("skills/entry/manifest.json"),
        serde_json::to_vec(&skills).unwrap(),
    );
    fs::create_dir_all(bin.join("skills/entry/items")).unwrap();
    symlink("../unavailable-target", bin.join("skills/entry/items/0")).unwrap();
    let memory = manifest(
        "memory",
        json!([{"from":"/fixture/tree","mode":448,"size":0,"stored":"items/0","type":"dir"},{"from":"/fixture/tree/data","mode":384,"size":6,"stored":"items/0/data","type":"file"}]),
    );
    write(
        &bin.join("memory/entry/manifest.json"),
        serde_json::to_vec(&memory).unwrap(),
    );
    write(&bin.join("memory/entry/items/0/data"), b"opaque");
    fs::File::open(bin.join("memory/entry/items/0"))
        .unwrap()
        .set_xattr("user.directory", b"folder attribute")
        .unwrap();
    fs::File::open(bin.join("memory/entry/items/0/data"))
        .unwrap()
        .set_xattr("user.file", &[0, 255, 13, 10])
        .unwrap();
    if scenario == "killed-removal" {
        let mut bulk = memory.clone();
        for index in 0..512 {
            let name = format!("bulk-{index}");
            write(&bin.join("memory/entry/items/0").join(&name), b"opaque");
            bulk["items"].as_array_mut().unwrap().push(json!({"from":format!("/fixture/tree/{name}"),"mode":384,"size":6,"stored":format!("items/0/{name}"),"type":"file"}));
        }
        write(
            &bin.join("memory/entry/manifest.json"),
            serde_json::to_vec(&bulk).unwrap(),
        );
    }
    if let Some(field) = scenario.strip_prefix("artifact-") {
        let mut malformed = memory.clone();
        match field {
            "id" => {
                hooks["helperRecordId"] = "bad".into();
                write(
                    &bin.join("hooks/entry/manifest.json"),
                    serde_json::to_vec(&hooks).unwrap(),
                );
            }
            "type" => malformed["items"][1]["type"] = "link".into(),
            "size" => malformed["items"][1]["size"] = 100.into(),
            "incomplete" => {
                malformed.as_object_mut().unwrap().remove("name");
            }
            "target" => {
                let mut wrong = skills.clone();
                wrong["items"][0]["target"] = "different".into();
                write(
                    &bin.join("skills/entry/manifest.json"),
                    serde_json::to_vec(&wrong).unwrap(),
                );
            }
            "kind" => {
                fs::remove_file(bin.join("memory/entry/items/0/data")).unwrap();
                fs::create_dir(bin.join("memory/entry/items/0/data")).unwrap();
            }
            "attributes-conflict" => {
                write(
                    &native.state.join("artifact-bin/memory/entry/items/0/data"),
                    b"opaque",
                );
                fs::File::open(native.state.join("artifact-bin/memory/entry/items/0/data"))
                    .unwrap()
                    .set_xattr("user.file", b"changed")
                    .unwrap();
            }
            _ => panic!("unknown artifact scenario"),
        }
        write(
            &bin.join("memory/entry/manifest.json"),
            serde_json::to_vec(&malformed).unwrap(),
        );
    }
    match scenario.as_str() {
        "malformed" => write(&legacy.state.join("state.json"), "{broken"),
        "newer" => write(&legacy.config.join("settings.json"), br#"{"version":2}"#),
        "missing" => fs::remove_file(legacy.recovery.join("hooks-recovery/0123456789abcdef0123456789abcdef.json")).unwrap(),
        "mismatched" => write(&legacy.recovery.join("hooks-recovery/0123456789abcdef0123456789abcdef.json"), br#"{"formatVersion":1,"transactionId":"0123456789abcdef0123456789abcdef","definitionId":"definition","payload":null}"#),
        "alias-conflict" => write(&legacy.state.join("modules/skills/opaque.bin"), "different"),
        "conflict" => write(&native.state.join("state.json"), "user data"),
        "unsafe-link" => { fs::remove_file(legacy.state.join("state.json")).unwrap(); symlink("outside", legacy.state.join("state.json")).unwrap(); },
        _ => {}
    }
    if let Some((false, path, bytes)) = &invalid_native {
        write(path, bytes);
        let source = if path.starts_with(&native.state) {
            legacy.state.join("state.json")
        } else {
            legacy.config.join(path.file_name().unwrap())
        };
        if source.exists() {
            fs::remove_file(source).unwrap();
        }
    }
    let authority =
        Authority::acquire_bound(&native.state, &native.config, &native.recovery).unwrap();
    if scenario == "identity" {
        fs::rename(&native.state, native.state.with_extension("old")).unwrap();
        fs::create_dir(&native.state).unwrap();
    }
    let _legacy_lock = if scenario == "bin-locked"
        || scenario == "journal-locked"
        || scenario == "helper-locked"
    {
        use std::os::fd::AsRawFd;
        let path = if scenario == "bin-locked" {
            bin.join(".mutation.lock")
        } else if scenario == "helper-locked" {
            legacy.recovery.join("hooks-recovery/.lock")
        } else {
            legacy.state.join("journal.json.lock")
        };
        let file = fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(path)
            .unwrap();
        assert_eq!(
            unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) },
            0
        );
        Some(file)
    } else {
        None
    };
    let original_records: Vec<_> = ["hooks", "mcp"]
        .iter()
        .flat_map(|module| {
            [
                legacy
                    .recovery
                    .join(format!("{module}-recovery/{record_id}.json")),
                bin.join(format!("{module}/entry/manifest.json")),
            ]
        })
        .filter_map(|path| fs::read(&path).ok().map(|bytes| (path, bytes)))
        .collect();
    let result = prepare(&legacy, &native, &authority).unwrap();
    assert!(matches!(
        authority.write_mode(),
        fileblade::lease::WriteMode::ReadOnly { .. }
    ));
    if scenario.starts_with("payload-") && !scenario.ends_with("-valid") {
        assert!(
            matches!(result.status, Status::Refused { .. }),
            "{result:?}"
        );
        assert!(!result.receipt_path.exists());
        for (path, bytes) in original_records {
            assert_eq!(fs::read(path).unwrap(), bytes);
        }
        return;
    }
    if let Some((false, path, bytes)) = &invalid_native {
        assert!(
            matches!(result.status, Status::Refused { .. }),
            "{result:?}"
        );
        assert_eq!(fs::read(path).unwrap(), *bytes);
        assert!(!result.receipt_path.exists());
        return;
    }
    if scenario.starts_with("late-") {
        assert!(
            matches!(result.status, Status::Refused { .. }),
            "{result:?}"
        );
        assert!(result.receipt_path.exists());
        assert!(!native.state.join("migration-020/complete.json").exists());
        assert!(!bin.join(".migration-020-retired").exists());
        match scenario.as_str() {
            "late-add" => assert_eq!(
                fs::read(bin.join("memory/entry/new-data")).unwrap(),
                b"must survive"
            ),
            "late-change" => assert_eq!(
                fs::read(bin.join("memory/entry/items/0/data")).unwrap(),
                b"changed"
            ),
            "late-root" => {
                assert_eq!(fs::read(bin.join("replacement")).unwrap(), b"must survive");
                assert_eq!(
                    fs::read(bin.with_extension("old").join("memory/entry/items/0/data")).unwrap(),
                    b"opaque"
                );
            }
            "late-attributes" => assert_eq!(
                fs::File::open(bin.join("memory/entry/items/0/data"))
                    .unwrap()
                    .get_xattr("user.file")
                    .unwrap(),
                Some(b"changed".to_vec())
            ),
            _ => unreachable!(),
        }
        return;
    }
    if scenario.starts_with("artifact-")
        || [
            "malformed",
            "newer",
            "missing",
            "mismatched",
            "alias-conflict",
            "conflict",
            "unsafe-link",
            "identity",
            "lock-replaced",
        ]
        .contains(&scenario.as_str())
    {
        assert!(
            matches!(result.status, Status::Refused { .. }),
            "{result:?}"
        );
        assert!(!result.receipt_path.exists());
        if scenario == "artifact-attributes-conflict" {
            assert_eq!(
                fs::File::open(bin.join("memory/entry/items/0/data"))
                    .unwrap()
                    .get_xattr("user.file")
                    .unwrap(),
                Some(vec![0, 255, 13, 10])
            );
            assert_eq!(
                fs::File::open(native.state.join("artifact-bin/memory/entry/items/0/data"))
                    .unwrap()
                    .get_xattr("user.file")
                    .unwrap(),
                Some(b"changed".to_vec())
            );
        }
        return;
    }
    if [
        "active",
        "unknown",
        "bin-locked",
        "journal-locked",
        "helper-locked",
        "evidence-changed",
    ]
    .contains(&scenario.as_str())
    {
        assert!(
            matches!(result.status, Status::ReadOnly { .. }),
            "{result:?}"
        );
        assert!(!result.receipt_path.exists());
        return;
    }
    assert_eq!(result.status, Status::Ready, "{result:?}");
    assert_eq!(fs::read(native.state.join("state.json")).unwrap(), state);
    assert_eq!(
        fs::read(native.state.join("modules/skills/opaque.bin")).unwrap(),
        [0, 255, 13, 10]
    );
    assert_eq!(
        fs::read_link(native.state.join("artifact-bin/skills/entry/items/0")).unwrap(),
        Path::new("../unavailable-target")
    );
    assert_eq!(
        fs::read(
            native
                .recovery
                .join("hooks-recovery/0123456789abcdef0123456789abcdef.json")
        )
        .unwrap(),
        fs::read(
            legacy
                .recovery
                .join("hooks-recovery/0123456789abcdef0123456789abcdef.json")
        )
        .unwrap()
    );
    assert_eq!(
        fs::File::open(native.state.join("artifact-bin/memory/entry/items/0/data"))
            .unwrap()
            .get_xattr("user.file")
            .unwrap(),
        Some(vec![0, 255, 13, 10])
    );
    assert_eq!(
        fs::File::open(native.state.join("artifact-bin/memory/entry/items/0"))
            .unwrap()
            .get_xattr("user.directory")
            .unwrap(),
        Some(b"folder attribute".to_vec())
    );
    assert!(!bin.join("hooks/entry").exists());
    assert!(!bin.join("skills/entry").exists());
    if let Some((true, path, bytes)) = &invalid_native {
        write(path, bytes);
        assert!(matches!(
            prepare(&legacy, &native, &authority).unwrap().status,
            Status::Refused { .. }
        ));
        assert_eq!(fs::read(path).unwrap(), *bytes);
        return;
    }
    if scenario == "shared-roots" {
        assert_eq!(
            prepare(&legacy, &native, &authority).unwrap().status,
            Status::Ready
        );
        return;
    }
    if scenario == "completed-newer" {
        write(&native.state.join("state.json"), br#"{"version":13}"#);
        assert!(matches!(
            prepare(&legacy, &native, &authority).unwrap().status,
            Status::Refused { .. }
        ));
        assert_eq!(
            fs::read(native.state.join("state.json")).unwrap(),
            br#"{"version":13}"#
        );
        return;
    }
    let receipt = fs::read(&result.receipt_path).unwrap();
    if scenario == "retired-change" {
        fs::remove_file(native.state.join("migration-020/complete.json")).unwrap();
        let snapshot: serde_json::Value = serde_json::from_slice(&receipt).unwrap();
        let index = snapshot["entries"]
            .as_array()
            .unwrap()
            .iter()
            .position(|entry| entry["source"] == "bin" && entry["from"] == "memory/entry/items/0")
            .unwrap();
        let extra = bin.join(format!(".migration-020-retired/{index}/new-data"));
        write(&extra, b"must survive");
        assert!(matches!(
            prepare(&legacy, &native, &authority).unwrap().status,
            Status::Refused { .. }
        ));
        assert_eq!(fs::read(&extra).unwrap(), b"must survive");
        assert!(!native.state.join("migration-020/complete.json").exists());
        return;
    }
    let receipt_time = fs::metadata(&result.receipt_path)
        .unwrap()
        .modified()
        .unwrap();
    if scenario == "resume" || scenario == "resume-conflict" {
        fs::remove_file(native.state.join("migration-020/complete.json")).unwrap();
        fs::remove_file(native.state.join("state.json")).unwrap();
        if scenario == "resume-conflict" {
            write(&native.state.join("state.json"), "new user edit");
        }
    } else {
        write(
            &native.state.join("state.json"),
            r#"{"version":12,"nativeEdit":true}"#,
        );
        write(
            &legacy.state.join("state.json"),
            "legacy changed after completed import",
        );
    }
    let repeated = prepare(&legacy, &native, &authority).unwrap();
    if scenario == "resume-conflict" {
        assert!(matches!(repeated.status, Status::Refused { .. }));
        assert_eq!(
            fs::read(native.state.join("state.json")).unwrap(),
            b"new user edit"
        );
    } else {
        assert_eq!(repeated.status, Status::Ready, "{repeated:?}");
        assert_eq!(
            fs::read(native.state.join("state.json")).unwrap(),
            if scenario == "resume" {
                state.as_slice()
            } else {
                br#"{"version":12,"nativeEdit":true}"#
            }
        );
    }
    assert_eq!(fs::read(&result.receipt_path).unwrap(), receipt);
    assert_eq!(
        fs::metadata(&result.receipt_path)
            .unwrap()
            .modified()
            .unwrap(),
        receipt_time
    );
}

#[test]
fn migration_preserves_refuses_and_resumes() {
    let mut scenarios: Vec<String> = [
        "first",
        "payload-json-valid",
        "payload-toml-valid",
        "killed-removal",
        "retired-change",
        "legacy-payload",
        "external",
        "late-add",
        "late-change",
        "late-root",
        "late-attributes",
        "helper-locked",
        "artifact-id",
        "artifact-type",
        "artifact-size",
        "artifact-incomplete",
        "artifact-target",
        "artifact-kind",
        "artifact-attributes-conflict",
        "held",
        "lock-replaced",
        "evidence-changed",
        "shared-roots",
        "completed-newer",
        "native-environment",
        "bin-locked",
        "journal-locked",
        "resume",
        "resume-conflict",
        "malformed",
        "newer",
        "missing",
        "mismatched",
        "alias-conflict",
        "conflict",
        "unsafe-link",
        "identity",
        "active",
        "unknown",
    ]
    .into_iter()
    .map(str::to_owned)
    .collect();
    for (module, fields) in [
        (
            "hooks",
            vec![
                "group",
                "index",
                "grouped",
                "removedGroup",
                "fields",
                "entry",
                "before",
                "after",
            ],
        ),
        (
            "json",
            vec!["container", "name", "raw", "position", "definition"],
        ),
        (
            "toml",
            vec!["name", "text", "offset", "after", "definition"],
        ),
    ] {
        for field in fields {
            for damage in ["missing", "wrong"] {
                scenarios.push(format!("payload-{module}-{field}-{damage}"));
            }
        }
    }
    for phase in ["native", "completed"] {
        for kind in ["state", "settings", "layout", "keybindings"] {
            for fault in ["newer", "malformed", "unsupported"] {
                scenarios.push(format!("{phase}-{kind}-{fault}"));
            }
        }
    }
    for scenario in scenarios {
        let scenario = scenario.as_str();
        let temp = tempfile::tempdir().unwrap();
        let base = temp.path();
        write(&base.join("omarchy/shell/shell.qml"), "");
        write(
            &base.join("legacy/config/omarchy/shell.json"),
            br#"{"plugins":[],"disabledPlugins":[]}"#,
        );
        for (name, script) in [
            ("omarchy", "#!/bin/sh\nprintf '%s\\n' '[]'\n"),
            (
                "qs",
                if scenario == "active" {
                    "#!/bin/sh\nprintf '%s\\n' '{}'\n"
                } else if scenario == "held"
                    || scenario == "lock-replaced"
                    || scenario.starts_with("late-")
                {
                    "#!/bin/sh\ncount=0\nif [ -f \"$HOME/probes\" ]; then read count < \"$HOME/probes\"; fi\ncount=$((count+1))\nprintf '%s\\n' \"$count\" > \"$HOME/probes\"\nif [ \"$count\" = 2 ]; then : > \"$HOME/held\"; while [ ! -f \"$HOME/release\" ]; do /usr/bin/sleep 0.01; done; fi\nprintf '%s\\n' 'Target not found.'\n"
                } else if scenario == "evidence-changed" {
                    "#!/bin/sh\nif [ -f \"$HOME/probed\" ]; then printf '%s\\n' '{}'; else : > \"$HOME/probed\"; printf '%s\\n' 'Target not found.'; fi\n"
                } else if scenario == "unknown" {
                    "#!/bin/sh\nexit 1\n"
                } else {
                    "#!/bin/sh\nprintf '%s\\n' 'Target not found.'\n"
                },
            ),
        ] {
            let path = base.join("bin").join(name);
            let script = if scenario.starts_with("late-") {
                script.replace("= 2 ]", "= 3 ]")
            } else {
                script.to_owned()
            };
            write(&path, script);
            fs::set_permissions(path, fs::Permissions::from_mode(0o700)).unwrap();
        }
        let mut command = Command::new(std::env::current_exe().unwrap());
        command
            .args(["--exact", "migration_worker", "--nocapture"])
            .env("MIGRATION_CASE", scenario)
            .env(
                "FILEBLADE_APP_ROOT",
                fileblade::paths::app_root()
                    .unwrap_or_else(|_| PathBuf::from(env!("CARGO_MANIFEST_DIR"))),
            )
            .env("HOME", base)
            .env("PATH", base.join("bin"))
            .env("XDG_DATA_HOME", base.join("data"))
            .env("XDG_CONFIG_HOME", base.join("legacy/config"))
            .env("OMARCHY_PATH", base.join("omarchy"))
            .env_remove("FILEBLADE_NATIVE_STATE_ROOT");
        if scenario == "native-environment" {
            command.env("FILEBLADE_NATIVE_STATE_ROOT", base.join("native/state"));
        }
        let output = if scenario == "killed-removal" {
            command
                .stdout(std::process::Stdio::piped())
                .stderr(std::process::Stdio::piped());
            let mut child = command.spawn().unwrap();
            let retired = base.join("data/fileblade/bin/.migration-020-retired");
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(15);
            while !fs::read_dir(&retired).is_ok_and(|mut entries| entries.next().is_some())
                && std::time::Instant::now() < deadline
            {
                std::thread::sleep(std::time::Duration::from_millis(1));
            }
            assert!(retired.exists());
            child.kill().unwrap();
            assert!(!child.wait_with_output().unwrap().status.success());
            assert!(
                !base
                    .join("native/state/migration-020/complete.json")
                    .exists()
            );
            Command::new(std::env::current_exe().unwrap())
                .args(["--exact", "interrupted_consumer_worker", "--nocapture"])
                .envs(
                    command
                        .get_envs()
                        .filter_map(|(key, value)| value.map(|value| (key, value))),
                )
                .output()
                .unwrap()
        } else if scenario == "held" || scenario == "lock-replaced" || scenario.starts_with("late-")
        {
            use std::os::fd::AsRawFd;
            command
                .stdout(std::process::Stdio::piped())
                .stderr(std::process::Stdio::piped());
            let child = command.spawn().unwrap();
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
            while !base.join("held").exists() && std::time::Instant::now() < deadline {
                std::thread::sleep(std::time::Duration::from_millis(5));
            }
            assert!(base.join("held").exists());
            for path in [
                base.join("legacy/state/journal.json.lock"),
                base.join("data/fileblade/bin/.mutation.lock"),
                base.join("legacy/recovery/hooks-recovery/.lock"),
            ] {
                let file = fs::OpenOptions::new()
                    .read(true)
                    .write(true)
                    .open(&path)
                    .unwrap();
                assert_ne!(
                    unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) },
                    0
                );
                let mut range: libc::flock = unsafe { std::mem::zeroed() };
                range.l_type = libc::F_WRLCK as _;
                range.l_whence = libc::SEEK_SET as _;
                assert_ne!(
                    unsafe { libc::fcntl(file.as_raw_fd(), libc::F_OFD_SETLK, &range) },
                    0
                );
            }
            if scenario == "lock-replaced" {
                let path = base.join("data/fileblade/bin/.mutation.lock");
                fs::rename(&path, path.with_extension("original")).unwrap();
                write(&path, "");
            }
            match scenario {
                "late-add" => write(
                    &base.join("data/fileblade/bin/memory/entry/new-data"),
                    "must survive",
                ),
                "late-change" => write(
                    &base.join("data/fileblade/bin/memory/entry/items/0/data"),
                    "changed",
                ),
                "late-attributes" => {
                    fs::File::open(base.join("data/fileblade/bin/memory/entry/items/0/data"))
                        .unwrap()
                        .set_xattr("user.file", b"changed")
                        .unwrap()
                }
                "late-root" => {
                    let bin = base.join("data/fileblade/bin");
                    fs::rename(&bin, bin.with_extension("old")).unwrap();
                    write(&bin.join("replacement"), "must survive");
                }
                _ => {}
            }
            write(&base.join("release"), "");
            child.wait_with_output().unwrap()
        } else {
            command.output().unwrap()
        };
        assert!(
            output.status.success(),
            "{scenario}: {} {}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }
}

#[test]
fn interrupted_consumer_worker() {
    if std::env::var("MIGRATION_CASE").as_deref() != Ok("killed-removal") {
        return;
    }
    let base = PathBuf::from(std::env::var_os("HOME").unwrap());
    let legacy = roots(&base.join("legacy"));
    let native = roots(&base.join("native"));
    let authority =
        Authority::acquire_bound(&native.state, &native.config, &native.recovery).unwrap();
    let prepared = prepare(&legacy, &native, &authority).unwrap();
    assert_eq!(prepared.status, Status::Ready, "{prepared:?}");
    assert_eq!(
        prepare(&legacy, &native, &authority).unwrap().status,
        Status::Ready
    );
    assert_eq!(
        fs::File::open(native.state.join("artifact-bin/memory/entry/items/0/data"))
            .unwrap()
            .get_xattr("user.file")
            .unwrap(),
        Some(vec![0, 255, 13, 10])
    );
    assert_eq!(
        fs::File::open(native.state.join("artifact-bin/memory/entry/items/0"))
            .unwrap()
            .get_xattr("user.directory")
            .unwrap(),
        Some(b"folder attribute".to_vec())
    );
    assert!(!base.join("data/fileblade/bin/memory").exists());
    println!("killed during artifact retirement: retry completed with original attributes");
}

#[test]
fn generated_fixture_worker() {
    let Some(path) = std::env::var_os("MIGRATION_GENERATED_FIXTURE") else {
        return;
    };
    let fixture: serde_json::Value = serde_json::from_slice(&fs::read(path).unwrap()).unwrap();
    let read_roots = |key: &str| Roots {
        config: PathBuf::from(fixture[key]["config"].as_str().unwrap()),
        state: PathBuf::from(fixture[key]["state"].as_str().unwrap()),
        recovery: PathBuf::from(fixture[key]["recovery"].as_str().unwrap()),
    };
    let legacy = read_roots("legacy");
    let native = read_roots("native");
    let authority =
        Authority::acquire_bound(&native.state, &native.config, &native.recovery).unwrap();
    let result = prepare(&legacy, &native, &authority).unwrap();
    let scenario = fixture["scenario"].as_str().unwrap();
    match scenario {
        "stopped" => {
            assert_eq!(result.status, Status::Ready, "{result:?}");
            for (source, target) in [
                (&legacy.state, &native.state),
                (&legacy.config, &native.config),
            ] {
                for name in [
                    "state.json",
                    "settings.json",
                    "keybindings.json",
                    "blades.json",
                ] {
                    if source.join(name).is_file() {
                        assert_eq!(
                            fs::read(source.join(name)).unwrap(),
                            fs::read(target.join(name)).unwrap()
                        );
                    }
                }
            }
            assert_eq!(
                prepare(&legacy, &native, &authority).unwrap().status,
                Status::Ready
            );
            drop(authority);
            let output = Command::new(std::env::current_exe().unwrap())
                .args(["--exact", "native_consumer_worker", "--nocapture"])
                .env("MIGRATION_NATIVE_CONSUMER", "1")
                .env(
                    "XDG_CONFIG_HOME",
                    native.config.parent().unwrap().parent().unwrap(),
                )
                .env(
                    "XDG_STATE_HOME",
                    native.state.parent().unwrap().parent().unwrap(),
                )
                .env("FILEBLADE_NATIVE_STATE_ROOT", &native.state)
                .output()
                .unwrap();
            assert!(
                output.status.success(),
                "{} {}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            );
            println!("{}", String::from_utf8_lossy(&output.stdout));
            let bin = PathBuf::from(fixture["artifactBin"].as_str().unwrap());
            fs::rename(&bin, bin.with_extension("retained")).unwrap();
            fs::rename(native.state.join("artifact-bin"), &bin).unwrap();
            fs::rename(&legacy.recovery, legacy.recovery.with_extension("retained")).unwrap();
            fs::rename(&native.recovery, &legacy.recovery).unwrap();
            for module in ["hooks", "mcp"] {
                let rows = fileblade::artifact_bin::rows(module);
                assert_eq!(rows["ok"], true, "{rows}");
                assert_eq!(rows["items"].as_array().unwrap().len(), 1, "{rows}");
                let entry = fixture["recoveryEntries"][module].as_str().unwrap();
                let restored = fileblade::artifact_bin::restore(
                    module,
                    entry,
                    &std::sync::atomic::AtomicBool::new(false),
                );
                assert_eq!(restored["ok"], true, "{module}: {restored}");
            }
            let file = fs::File::open(fixture["attributeFile"].as_str().unwrap()).unwrap();
            assert_eq!(
                file.get_xattr("user.fileblade-fixture").unwrap(),
                Some(vec![0, 255, 13, 10])
            );
            let tree = PathBuf::from(fixture["attributeTree"].as_str().unwrap());
            assert_eq!(
                fs::File::open(&tree)
                    .unwrap()
                    .get_xattr("user.fileblade-fixture")
                    .unwrap(),
                Some(b"directory attribute".to_vec())
            );
            assert_eq!(
                fs::File::open(tree.join("child"))
                    .unwrap()
                    .get_xattr("user.fileblade-fixture")
                    .unwrap(),
                Some(b"child attribute".to_vec())
            );
            println!(
                "production recovery round trip passed using relocated imported objects; native route wiring is qualified separately"
            );
        }
        "active" => assert!(
            matches!(result.status, Status::ReadOnly { .. }),
            "{result:?}"
        ),
        _ => assert!(
            matches!(result.status, Status::Refused { .. }),
            "{result:?}"
        ),
    }
    println!("fixture {scenario}: {:?}", result.status);
}

#[test]
fn native_consumer_worker() {
    if std::env::var("MIGRATION_NATIVE_CONSUMER").as_deref() != Ok("1") {
        return;
    }
    let fixture: serde_json::Value = serde_json::from_slice(
        &fs::read(std::env::var_os("MIGRATION_GENERATED_FIXTURE").unwrap()).unwrap(),
    )
    .unwrap();
    let roots = |role: &str| Roots {
        config: PathBuf::from(fixture[role]["config"].as_str().unwrap()),
        state: PathBuf::from(fixture[role]["state"].as_str().unwrap()),
        recovery: PathBuf::from(fixture[role]["recovery"].as_str().unwrap()),
    };
    let legacy = roots("legacy");
    let native = roots("native");
    let authority = std::sync::Arc::new(
        Authority::acquire_bound(&native.state, &native.config, &native.recovery).unwrap(),
    );
    assert_eq!(
        prepare(&legacy, &native, &authority).unwrap().status,
        Status::Ready
    );
    authority
        .set_write_mode(fileblade::lease::WriteMode::Full)
        .unwrap();
    let _session = fileblade::lease::persistence::PersistenceSession::open(authority).unwrap();
    for module in ["skills", "memory", "hooks", "mcp"] {
        let rows = fileblade::artifact_bin::rows(module);
        assert_eq!(rows["ok"], true, "{rows}");
        assert_eq!(rows["items"].as_array().unwrap().len(), 1, "{rows}");
    }
    let restored = fileblade::artifact_bin::restore(
        "memory",
        fixture["recoveryEntries"]["memory"].as_str().unwrap(),
        &std::sync::atomic::AtomicBool::new(false),
    );
    assert_eq!(restored["ok"], true, "{restored}");
    assert_eq!(
        fs::File::open(fixture["attributeFile"].as_str().unwrap())
            .unwrap()
            .get_xattr("user.fileblade-fixture")
            .unwrap(),
        Some(vec![0, 255, 13, 10])
    );
    let tree = PathBuf::from(fixture["attributeTree"].as_str().unwrap());
    assert_eq!(
        fs::File::open(&tree)
            .unwrap()
            .get_xattr("user.fileblade-fixture")
            .unwrap(),
        Some(b"directory attribute".to_vec())
    );
    assert_eq!(
        fs::File::open(tree.join("child"))
            .unwrap()
            .get_xattr("user.fileblade-fixture")
            .unwrap(),
        Some(b"child attribute".to_vec())
    );
    println!(
        "native reader: four valid bins; native put/import/restore: file and directory attributes preserved"
    );
}
