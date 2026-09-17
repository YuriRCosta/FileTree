use fileblade::core_modules::recovery_store::{
    MAX_RECORDS, RecoveryError, RecoveryStore, valid_identifier,
};
use serde_json::{Value, json};
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

const TRANSACTION: &str = "0123456789abcdef0123456789abcdef";

fn store_at(root: &Path) -> (RecoveryStore, PathBuf) {
    let directory = root.join("recovery");
    fs::create_dir_all(&directory).unwrap();
    fs::set_permissions(&directory, fs::Permissions::from_mode(0o700)).unwrap();
    (RecoveryStore::new(directory.clone()), directory)
}

fn baseline_record() -> Value {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/golden/python-baseline");
    let manifest: Value =
        serde_json::from_slice(&fs::read(root.join("manifest.json")).unwrap()).unwrap();
    let sandbox = manifest["root"].as_str().unwrap().to_string();
    let text = fs::read_to_string(root.join("hooks/recovery-record.json")).unwrap();
    serde_json::from_str(
        &text
            .replace("{ROOT}", &sandbox)
            .replace("\"{CREATED_AT}\"", "1758132000"),
    )
    .unwrap()
}

#[test]
fn identifiers_are_thirty_two_lowercase_hexadecimal_characters() {
    assert!(valid_identifier(TRANSACTION));
    assert!(!valid_identifier("0123456789ABCDEF0123456789abcdef"));
    assert!(!valid_identifier("0123456789abcdef"));
    assert!(!valid_identifier(""));
}

#[test]
fn a_record_written_by_python_is_read_back_with_its_payload_and_inventory() {
    let scratch = tempfile::tempdir().unwrap();
    let (store, directory) = store_at(scratch.path());
    let record = baseline_record();
    let path = directory.join(format!("{TRANSACTION}.json"));
    fs::write(&path, serde_json::to_vec(&record).unwrap()).unwrap();
    fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();

    let read = store.read(TRANSACTION).expect("frozen record");
    assert_eq!(read.record_id, TRANSACTION);
    assert_eq!(
        read.definition_id(),
        record["definitionId"].as_str().unwrap()
    );
    assert_eq!(read.payload(), &record["payload"]);
    assert_eq!(read.context(), &record["context"]);
    assert_eq!(read.transaction_id(), TRANSACTION);
    assert!(!read.restored());
    assert_eq!(read.created_at(), 1758132000);

    let inventory = store.inventory();
    assert_eq!(inventory["ok"], true);
    assert_eq!(
        inventory["records"],
        json!([{
            "recordId": TRANSACTION,
            "createdAt": 1758132000,
            "restored": false,
            "trackedTransaction": true,
        }])
    );
    assert_eq!(store.live_records().unwrap(), 1);
    assert_eq!(
        store.find(&record["payload"]).unwrap().record_id,
        TRANSACTION
    );
}

#[test]
fn a_store_directory_that_is_not_private_is_refused() {
    let scratch = tempfile::tempdir().unwrap();
    let (store, directory) = store_at(scratch.path());
    fs::set_permissions(&directory, fs::Permissions::from_mode(0o755)).unwrap();
    let inventory = store.inventory();
    assert_eq!(inventory["ok"], false);
    assert_eq!(
        inventory["message"],
        "recovery directory must be owned by this user with mode 0700"
    );
}

#[test]
fn a_record_that_is_not_a_private_file_is_refused() {
    let scratch = tempfile::tempdir().unwrap();
    let (store, directory) = store_at(scratch.path());
    let path = directory.join(format!("{TRANSACTION}.json"));
    fs::write(&path, serde_json::to_vec(&baseline_record()).unwrap()).unwrap();
    fs::set_permissions(&path, fs::Permissions::from_mode(0o644)).unwrap();
    assert!(store.read(TRANSACTION).is_none());
    assert_eq!(store.inventory()["ok"], false);
}

#[test]
fn writing_dedupes_an_identical_payload_context_and_transaction() {
    let scratch = tempfile::tempdir().unwrap();
    let (store, _) = store_at(scratch.path());
    let payload = json!({"format": 2, "entry": {"command": "audit"}});
    let context = json!({"home": "/tmp/home"});
    let first = store.write(&payload, "definition", &context, "").unwrap();
    let second = store.write(&payload, "definition", &context, "").unwrap();
    assert_eq!(first, second);
    assert!(valid_identifier(&first));
    assert_eq!(store.records().unwrap().len(), 1);

    let other = store
        .write(&payload, "definition", &json!({"home": "/tmp/other"}), "")
        .unwrap();
    assert_ne!(other, first);
    assert_eq!(store.records().unwrap().len(), 2);
}

#[test]
fn a_transaction_id_becomes_the_record_id_and_cannot_be_reused() {
    let scratch = tempfile::tempdir().unwrap();
    let (store, _) = store_at(scratch.path());
    let payload = json!({"format": 2});
    let context = json!({});
    let id = store
        .write(&payload, "definition", &context, TRANSACTION)
        .unwrap();
    assert_eq!(id, TRANSACTION);
    let error = store
        .write(&json!({"format": 3}), "definition", &context, TRANSACTION)
        .unwrap_err();
    assert!(matches!(error, RecoveryError::Invalid(_)), "{error}");
    assert_eq!(
        error.to_string(),
        "recovery transaction id is already in use"
    );

    let refused = store
        .write(&payload, "definition", &context, "not-hex")
        .unwrap_err();
    assert_eq!(refused.to_string(), "invalid recovery transaction id");
}

#[test]
fn the_live_record_limit_refuses_further_removals() {
    let scratch = tempfile::tempdir().unwrap();
    let (store, _) = store_at(scratch.path());
    for index in 0..MAX_RECORDS {
        store
            .write(&json!({"index": index}), "definition", &json!({}), "")
            .unwrap();
    }
    let error = store
        .write(&json!({"index": "overflow"}), "definition", &json!({}), "")
        .unwrap_err();
    assert!(matches!(error, RecoveryError::Full(_)), "{error}");
    assert_eq!(
        error.to_string(),
        "the undo store is full; restore or discard earlier removals before removing another"
    );
}

#[test]
fn marking_a_record_restored_replaces_it_through_a_staged_rename() {
    let scratch = tempfile::tempdir().unwrap();
    let (store, directory) = store_at(scratch.path());
    let payload = json!({"format": 2});
    let id = store.write(&payload, "definition", &json!({}), "").unwrap();
    store.mark_restored(&id);
    let record = store.read(&id).expect("restored record");
    assert!(record.restored());
    assert_eq!(store.live_records().unwrap(), 0);
    assert_eq!(store.inventory()["records"][0]["restored"], true);
    let staged: Vec<_> = fs::read_dir(&directory)
        .unwrap()
        .filter_map(|entry| entry.ok())
        .filter(|entry| entry.file_name().to_string_lossy().ends_with(".staged"))
        .collect();
    assert!(staged.is_empty(), "{staged:?}");
    store.mark_restored(&id);
    assert_eq!(
        store.read(&id).unwrap().document["restoredAt"],
        record.document["restoredAt"]
    );
}

#[test]
fn discarding_takes_either_a_record_id_or_a_matching_payload() {
    let scratch = tempfile::tempdir().unwrap();
    let (store, _) = store_at(scratch.path());
    let payload = json!({"format": 2, "name": "one"});
    let id = store.write(&payload, "definition", &json!({}), "").unwrap();
    assert_eq!(
        store.discard_payload(&id, Some(&json!({"format": 9})))["message"],
        "recovery record does not match this payload"
    );
    assert_eq!(store.discard_payload(&id, Some(&payload))["ok"], true);
    assert!(store.records().unwrap().is_empty());

    let id = store.write(&payload, "definition", &json!({}), "").unwrap();
    assert_eq!(store.discard_payload("", Some(&payload))["ok"], true);
    assert!(store.records().unwrap().is_empty());
    assert_eq!(
        store.discard_payload("", None)["message"],
        "discard needs a recovery record id or a matching payload"
    );

    let id_again = store.write(&payload, "definition", &json!({}), "").unwrap();
    assert_ne!(id_again, id);
    store.discard(&id_again).unwrap();
    assert!(store.records().unwrap().is_empty());
    assert_eq!(
        store.discard("short").unwrap_err().to_string(),
        "invalid recovery record id"
    );
}

#[test]
fn an_unrecognized_entry_stops_the_scan() {
    let scratch = tempfile::tempdir().unwrap();
    let (store, directory) = store_at(scratch.path());
    let stray = directory.join("notes.txt");
    fs::write(&stray, "stray").unwrap();
    fs::set_permissions(&stray, fs::Permissions::from_mode(0o600)).unwrap();
    let error = store.records().unwrap_err();
    assert!(matches!(error, RecoveryError::Full(_)), "{error}");
    assert_eq!(
        error.to_string(),
        "the undo store contains an unrecognized entry"
    );
}

#[test]
fn a_missing_store_reads_as_empty() {
    let scratch = tempfile::tempdir().unwrap();
    let store = RecoveryStore::new(scratch.path().join("absent"));
    assert!(store.records().unwrap().is_empty());
    assert_eq!(store.inventory()["records"], json!([]));
    assert!(store.read(TRANSACTION).is_none());
    store.expired().unwrap();
}
