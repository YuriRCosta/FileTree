mod support;

use serde_json::Value;
use std::os::unix::fs::symlink;
use std::path::Path;
use std::process::{Command, Output};
use support::Resident;
use tempfile::tempdir;

fn fileblade(arguments: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_fileblade"))
        .args(arguments)
        .output()
        .expect("run fileblade")
}

fn df(path: &Path) -> (u64, u64, u64, u64) {
    let output = Command::new("df")
        .args(["-B1", "--output=used,avail,size,pcent"])
        .arg(path)
        .output()
        .expect("run df");
    let text = String::from_utf8_lossy(&output.stdout);
    let line = text.lines().nth(1).expect("df data line");
    let mut fields = line.split_whitespace();
    let mut next = || {
        fields
            .next()
            .expect("df field")
            .trim_end_matches('%')
            .parse::<u64>()
            .unwrap()
    };
    (next(), next(), next(), next())
}

fn json(output: &Output) -> Value {
    serde_json::from_slice(&output.stdout).unwrap_or_else(|_| {
        panic!(
            "stdout is not json: {}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        )
    })
}

fn stderr(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}

#[test]
fn space_reports_the_filesystem_the_way_df_does() {
    let temporary = tempdir().unwrap();
    let path = temporary.path().to_string_lossy().into_owned();
    let output = fileblade(&["-o", "json", "space", &path]);
    assert!(output.status.success(), "{}", stderr(&output));
    let document = json(&output);
    let (used, available, size, percent) = df(temporary.path());
    assert_eq!(document["ok"], true, "{document}");
    assert_eq!(document["size"].as_u64().unwrap(), size, "{document}");
    let drift = 64u64 << 20;
    assert!(
        document["used"].as_u64().unwrap().abs_diff(used) <= drift,
        "used drifted: {document} against {used}"
    );
    assert!(
        document["available"].as_u64().unwrap().abs_diff(available) <= drift,
        "available drifted: {document} against {available}"
    );
    assert!(
        document["percent"].as_u64().unwrap().abs_diff(percent) <= 1,
        "percent drifted: {document} against {percent}"
    );
    let fraction = document["fraction"].as_f64().unwrap();
    assert!((0.0..=1.0).contains(&fraction), "{document}");
    assert!(document["mountpoint"].is_string(), "{document}");
    assert!(document["filesystem"].is_string(), "{document}");
    assert!(
        temporary
            .path()
            .starts_with(document["mountpoint"].as_str().unwrap()),
        "{document}"
    );

    let output = fileblade(&["space", &path]);
    assert!(output.status.success(), "{}", stderr(&output));
    let line = String::from_utf8_lossy(&output.stdout);
    assert!(line.contains("% full"), "{line}");
    assert!(line.contains(" free on "), "{line}");
    assert!(
        line.contains(&format!("{}% full", document["percent"])),
        "{line} against {document}"
    );
}

#[test]
fn space_refuses_files_missing_paths_and_remote_uris() {
    let temporary = tempdir().unwrap();
    let file = temporary.path().join("note.txt");
    std::fs::write(&file, "x").unwrap();
    let output = fileblade(&["space", file.to_str().unwrap()]);
    assert!(!output.status.success());
    assert!(
        stderr(&output).contains("is not a folder"),
        "{}",
        stderr(&output)
    );

    let missing = temporary.path().join("nope");
    let output = fileblade(&["space", missing.to_str().unwrap()]);
    assert!(!output.status.success());
    assert!(stderr(&output).contains("nope"), "{}", stderr(&output));

    let output = fileblade(&["space", "sftp://peer/home"]);
    assert!(!output.status.success());
    assert!(!stderr(&output).is_empty());

    let local_lookalike = temporary.path().join("sftp:").join("peer").join("home");
    std::fs::create_dir_all(&local_lookalike).unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_fileblade"))
        .args(["space", "sftp://peer/home"])
        .current_dir(temporary.path())
        .output()
        .expect("run fileblade");
    assert!(
        !output.status.success(),
        "a remote URI must not resolve to a local folder"
    );

    let output = fileblade(&[
        "-o",
        "json",
        "space",
        &format!("file://{}", temporary.path().display()),
    ]);
    assert!(output.status.success(), "{}", stderr(&output));
    assert_eq!(json(&output)["ok"], true);
}

#[test]
fn space_resolves_a_symlink_to_the_folder_it_points_at() {
    let temporary = tempdir().unwrap();
    let target = temporary.path().join("target");
    std::fs::create_dir(&target).unwrap();
    let link = temporary.path().join("link");
    symlink(&target, &link).unwrap();
    let output = fileblade(&["-o", "json", "space", link.to_str().unwrap()]);
    assert!(output.status.success(), "{}", stderr(&output));
    let document = json(&output);
    let canonical = std::fs::canonicalize(&target).unwrap();
    assert_eq!(
        document["path"].as_str().unwrap(),
        canonical.to_str().unwrap(),
        "{document}"
    );
}

#[test]
fn space_without_a_path_needs_a_window() {
    let temporary = tempdir().unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_fileblade"))
        .arg("space")
        .env("HOME", temporary.path())
        .env("XDG_RUNTIME_DIR", temporary.path())
        .env("PATH", temporary.path())
        .output()
        .expect("run fileblade");
    assert!(!output.status.success());
    assert!(stderr(&output).contains("pass PATH"), "{}", stderr(&output));
}

#[test]
fn capacity_answers_through_the_resident_protocol() {
    let temporary = tempdir().unwrap();
    let path = temporary.path().to_string_lossy().into_owned();
    let mut resident = Resident::start(2);
    resident.request("one", "capacity", &["--path".to_string(), path], 5_000);
    let response = resident.response("one");
    assert_eq!(response["ok"], true, "{response}");
    assert_eq!(response["payload"]["ok"], true, "{response}");
    assert!(response["payload"]["size"].is_number(), "{response}");
    resident.finish();
}

#[test]
fn one_capacity_probe_runs_at_a_time_in_a_resident_backend() {
    let temporary = tempdir().unwrap();
    let path = temporary.path().to_string_lossy().into_owned();
    let arguments = ["--path".to_string(), path];
    let mut resident = Resident::start_with(4, &[("FILEBLADE_CAPACITY_HOLD_MS", "3000")]);
    resident.request("hold", "capacity", &arguments, 1_000);
    std::thread::sleep(std::time::Duration::from_millis(200));
    resident.request("second", "capacity", &arguments, 10_000);
    let second = resident.response("second");
    assert_eq!(second["payload"]["busy"], true, "{second}");
    assert_eq!(second["payload"]["ok"], false, "{second}");

    resident.send(serde_json::json!({"v": 1, "type": "cancel", "id": "hold", "generation": 1}));
    resident.request("third", "capacity", &arguments, 10_000);
    let third = resident.response("third");
    assert_eq!(third["payload"]["busy"], true, "{third}");

    std::thread::sleep(std::time::Duration::from_millis(1_300));
    resident.request("expired", "capacity", &arguments, 10_000);
    let expired = resident.response("expired");
    assert_eq!(expired["payload"]["busy"], true, "{expired}");

    let hold = resident.response("hold");
    assert_eq!(hold["type"], "response", "{hold}");
    resident.request("after", "capacity", &arguments, 10_000);
    let after = resident.response("after");
    assert_eq!(after["ok"], true, "{after}");
    assert_eq!(after["payload"]["ok"], true, "{after}");
    assert!(after["payload"]["fraction"].is_number(), "{after}");
    resident.finish();
}
