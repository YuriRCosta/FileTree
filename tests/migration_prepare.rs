use fileblade::lease::Authority;
use fileblade::migration::{Roots, Status, prepare};
use serde_json::json;
use std::fs;
use std::os::unix::fs::{PermissionsExt, symlink};
use std::path::{Path, PathBuf};
use std::process::Command;

fn write(path: &Path, bytes: impl AsRef<[u8]>) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, bytes).unwrap();
}

fn roots(path: &Path) -> Roots {
    Roots {
        config: path.join("config/omarchy/fileblade"),
        state: path.join("state"),
        recovery: path.join("recovery"),
    }
}

#[test]
fn migration_worker() {
    let Ok(scenario) = std::env::var("MIGRATION_CASE") else {
        return;
    };
    let base = PathBuf::from(std::env::var_os("HOME").unwrap());
    let legacy = roots(&base.join("legacy"));
    let native = roots(&base.join("native"));
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
    write(&legacy.recovery.join("hooks-recovery/record.json"), serde_json::to_vec(&json!({"formatVersion":1,"transactionId":"record","definitionId":"definition","payload":{"keep":true}})).unwrap());
    let bin = base.join("data/fileblade/bin");
    write(&bin.join("hooks/entry/manifest.json"), serde_json::to_vec(&json!({"schemaVersion":1,"module":"hooks","id":"definition","helperRecordId":"record","payload":{"keep":true},"items":[]})).unwrap());
    write(&bin.join("skills/entry/manifest.json"), serde_json::to_vec(&json!({"schemaVersion":1,"module":"skills","items":[{"stored":"items/0","type":"link"}]})).unwrap());
    fs::create_dir_all(bin.join("skills/entry/items")).unwrap();
    symlink("../unavailable-target", bin.join("skills/entry/items/0")).unwrap();
    match scenario.as_str() {
        "malformed" => write(&legacy.state.join("state.json"), "{broken"),
        "newer" => write(&legacy.config.join("settings.json"), br#"{"version":2}"#),
        "missing" => fs::remove_file(legacy.recovery.join("hooks-recovery/record.json")).unwrap(),
        "mismatched" => write(&legacy.recovery.join("hooks-recovery/record.json"), br#"{"formatVersion":1,"transactionId":"record","definitionId":"definition","payload":null}"#),
        "alias-conflict" => write(&legacy.state.join("modules/skills/opaque.bin"), "different"),
        "conflict" => write(&native.state.join("state.json"), "user data"),
        "unsafe-link" => { fs::remove_file(legacy.state.join("state.json")).unwrap(); symlink("outside", legacy.state.join("state.json")).unwrap(); },
        _ => {}
    }
    let authority =
        Authority::acquire_bound(&native.state, &native.config, &native.recovery).unwrap();
    if scenario == "identity" {
        fs::rename(&native.state, native.state.with_extension("old")).unwrap();
        fs::create_dir(&native.state).unwrap();
    }
    let result = prepare(&legacy, &native, &authority).unwrap();
    if [
        "malformed",
        "newer",
        "missing",
        "mismatched",
        "alias-conflict",
        "conflict",
        "unsafe-link",
        "identity",
    ]
    .contains(&scenario.as_str())
    {
        assert!(
            matches!(result.status, Status::Refused { .. }),
            "{result:?}"
        );
        assert!(!result.receipt_path.exists());
        return;
    }
    if ["active", "unknown"].contains(&scenario.as_str()) {
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
        fs::read(native.recovery.join("hooks-recovery/record.json")).unwrap(),
        fs::read(legacy.recovery.join("hooks-recovery/record.json")).unwrap()
    );
    assert!(!bin.join("hooks/entry").exists());
    assert!(!bin.join("skills/entry").exists());
    let receipt = fs::read(&result.receipt_path).unwrap();
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
            "native user edit after first launch",
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
                b"native user edit after first launch"
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
    for scenario in [
        "first",
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
    ] {
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
                } else if scenario == "unknown" {
                    "#!/bin/sh\nexit 1\n"
                } else {
                    "#!/bin/sh\nprintf '%s\\n' 'Target not found.'\n"
                },
            ),
        ] {
            let path = base.join("bin").join(name);
            write(&path, script);
            fs::set_permissions(path, fs::Permissions::from_mode(0o700)).unwrap();
        }
        let output = Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "migration_worker", "--nocapture"])
            .env("MIGRATION_CASE", scenario)
            .env("HOME", base)
            .env("PATH", base.join("bin"))
            .env("XDG_DATA_HOME", base.join("data"))
            .env("XDG_CONFIG_HOME", base.join("legacy/config"))
            .env("OMARCHY_PATH", base.join("omarchy"))
            .env_remove("FILEBLADE_NATIVE_STATE_ROOT")
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{scenario}: {} {}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }
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
