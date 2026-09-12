use fileblade::operations::collisions::{discard, preflight};
use serde_json::Value;
use std::fs;
use std::sync::atomic::AtomicBool;
use tempfile::tempdir;

fn release(plan: &Value) {
    assert!(discard(plan["decision_id"].as_str().unwrap()));
    assert!(!discard(plan["decision_id"].as_str().unwrap()));
}

#[test]
fn mixed_conflicts_offer_applicable_choices_without_mutation() {
    let root = tempdir().unwrap();
    let source = root.path().join("source");
    let target = root.path().join("target");
    fs::create_dir(&source).unwrap();
    fs::create_dir(&target).unwrap();
    fs::write(source.join("file"), "new").unwrap();
    fs::write(target.join("file"), "old").unwrap();
    fs::create_dir(source.join("folder")).unwrap();
    fs::create_dir(target.join("folder")).unwrap();
    fs::write(source.join("fresh"), "fresh").unwrap();
    let sources =
        ["file", "folder", "fresh"].map(|name| source.join(name).to_string_lossy().into_owned());
    let plan = preflight(
        true,
        &sources,
        target.to_str().unwrap(),
        &AtomicBool::new(false),
    );
    assert_eq!(plan["ok"], true, "{plan}");
    assert_eq!(
        plan["items"][0]["choices"],
        serde_json::json!(["replace", "keep-both", "skip", "cancel"])
    );
    assert_eq!(
        plan["items"][1]["choices"],
        serde_json::json!(["merge", "replace", "keep-both", "skip", "cancel"])
    );
    assert_eq!(plan["items"][2]["collision"], false);
    assert_eq!(fs::read_to_string(target.join("file")).unwrap(), "old");
    assert_eq!(fs::read_to_string(source.join("file")).unwrap(), "new");
    assert!(!target.join("fresh").exists());
    release(&plan);
}

#[test]
fn two_incoming_names_do_not_offer_overwrite_of_an_unwritten_result() {
    let root = tempdir().unwrap();
    let target = root.path().join("target");
    fs::create_dir(&target).unwrap();
    let sources: Vec<_> = ["a", "b"]
        .into_iter()
        .map(|name| {
            let folder = root.path().join(name);
            fs::create_dir(&folder).unwrap();
            fs::write(folder.join("same"), name).unwrap();
            folder.join("same").to_string_lossy().into_owned()
        })
        .collect();
    let plan = preflight(
        false,
        &sources,
        target.to_str().unwrap(),
        &AtomicBool::new(false),
    );
    assert_eq!(plan["ok"], true, "{plan}");
    assert_eq!(plan["items"][0]["collision"], false);
    assert_eq!(plan["items"][1]["incoming_collision"], true);
    assert_eq!(
        plan["items"][1]["choices"],
        serde_json::json!(["keep-both", "skip", "cancel"])
    );
    release(&plan);
}

#[test]
fn same_entry_never_offers_destructive_replacement_or_merge() {
    let root = tempdir().unwrap();
    let file = root.path().join("file");
    fs::write(&file, "original").unwrap();
    for copy in [true, false] {
        let plan = preflight(
            copy,
            &[file.to_string_lossy().into_owned()],
            root.path().to_str().unwrap(),
            &AtomicBool::new(false),
        );
        assert_eq!(plan["ok"], true, "{plan}");
        let choices = if copy {
            serde_json::json!(["keep-both", "skip", "cancel"])
        } else {
            serde_json::json!(["skip", "cancel"])
        };
        assert_eq!(plan["items"][0]["choices"], choices);
        release(&plan);
    }
}

#[test]
fn invalid_overlapping_remote_and_cancelled_requests_produce_no_decision() {
    let root = tempdir().unwrap();
    let source = root.path().join("source");
    let target = root.path().join("target");
    fs::create_dir(&source).unwrap();
    fs::create_dir(&target).unwrap();
    fs::write(source.join("child"), "original").unwrap();
    for sources in [
        vec![],
        vec!["sftp://host/path".into()],
        vec!["mtp://[usb:005,009]/file".into()],
        vec![
            source.to_string_lossy().into_owned(),
            source.join("child").to_string_lossy().into_owned(),
        ],
    ] {
        let plan = preflight(
            false,
            &sources,
            target.to_str().unwrap(),
            &AtomicBool::new(false),
        );
        assert_eq!(plan["ok"], false, "{plan}");
        assert!(plan["decision_id"].is_null());
    }
    let plan = preflight(
        true,
        &[source.to_string_lossy().into_owned()],
        target.to_str().unwrap(),
        &AtomicBool::new(true),
    );
    assert_eq!(plan["ok"], false, "{plan}");
    assert_eq!(
        fs::read_to_string(source.join("child")).unwrap(),
        "original"
    );
    assert_eq!(fs::read_dir(target).unwrap().count(), 0);
}

#[test]
fn large_conflicting_folder_is_not_scanned_until_merge_is_selected() {
    let root = tempdir().unwrap();
    let source = root.path().join("source/folder");
    let target = root.path().join("target");
    fs::create_dir_all(&source).unwrap();
    fs::create_dir_all(target.join("folder")).unwrap();
    for index in 0..4100 {
        fs::write(source.join(index.to_string()), "").unwrap();
    }
    let plan = preflight(
        true,
        &[source.to_string_lossy().into_owned()],
        target.to_str().unwrap(),
        &AtomicBool::new(false),
    );
    assert_eq!(plan["ok"], true, "{plan}");
    assert_eq!(plan["items"].as_array().unwrap().len(), 1);
    release(&plan);
}

#[test]
fn oversized_merge_consumes_decision_without_changing_either_directory() {
    use fileblade::operations::collisions::{Decision, execute};
    let root = tempdir().unwrap();
    let source = root.path().join("source/folder");
    let target = root.path().join("target");
    fs::create_dir_all(&source).unwrap();
    fs::create_dir_all(target.join("folder")).unwrap();
    fs::write(target.join("folder/existing"), "keep").unwrap();
    for index in 0..10_000 {
        fs::write(source.join(index.to_string()), "source").unwrap();
    }
    for copy in [true, false] {
        let plan = preflight(
            copy,
            &[source.to_string_lossy().into_owned()],
            target.to_str().unwrap(),
            &AtomicBool::new(false),
        );
        assert_eq!(plan["ok"], true, "{plan}");
        let id = plan["decision_id"].as_str().unwrap();
        let result = execute(
            id,
            &[Decision {
                id: "0".into(),
                action: "merge".into(),
                apply_to_remaining: false,
            }],
            false,
            &mut |_| {},
            &AtomicBool::new(false),
        );
        assert_eq!(result["ok"], false, "{result}");
        assert_eq!(result["consumed"], true);
        assert!(result["error"].as_str().unwrap().contains("4096"));
        assert!(result.get("decision_id").is_none());
        assert!(!discard(id));
        assert_eq!(fs::read_dir(&source).unwrap().count(), 10_000);
        assert_eq!(fs::read_dir(target.join("folder")).unwrap().count(), 1);
        assert_eq!(
            fs::read_to_string(target.join("folder/existing")).unwrap(),
            "keep"
        );
    }
}
