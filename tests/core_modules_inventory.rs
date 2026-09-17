use fileblade::common::{parse_path, path_text};
use fileblade::core_modules::emit::{MAX_OUTPUT_BYTES, encoded, encoded_items, encoded_results};
use fileblade::core_modules::metrics::artifact_metrics;
use fileblade::core_modules::text::{clean, estimated_tokens, word_count};
use fileblade::core_modules::watch::{
    MAX_WATCH_BYTES, MAX_WATCH_PATHS, SCOPES, WatchPlan, lane_rows,
};
use serde_json::{Map, Value, json};
use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

fn tracked(plan: &WatchPlan) -> BTreeSet<PathBuf> {
    plan.paths().into_iter().collect()
}

fn canonical(path: &Path) -> PathBuf {
    fs::canonicalize(path).expect("canonical fixture path")
}

#[test]
fn a_missing_directory_tracks_its_existing_parent_then_the_new_directory() {
    let scratch = tempfile::tempdir().unwrap();
    let root = canonical(scratch.path());
    let missing = root.join("agent/rules");
    let mut plan = WatchPlan::new();
    plan.watch_path(&missing, true);
    assert_eq!(tracked(&plan), BTreeSet::from([root.clone()]));
    fs::create_dir_all(&missing).unwrap();
    let mut plan = WatchPlan::new();
    plan.watch_path(&missing, true);
    assert_eq!(
        tracked(&plan),
        BTreeSet::from([missing.clone(), missing.parent().unwrap().to_path_buf()])
    );
}

#[test]
fn a_symlink_target_and_its_alias_parent_both_refresh() {
    let scratch = tempfile::tempdir().unwrap();
    let root = canonical(scratch.path());
    let target = root.join("elsewhere");
    fs::create_dir(&target).unwrap();
    let source = target.join("instructions.md");
    fs::write(&source, "body").unwrap();
    let alias = root.join("AGENTS.md");
    std::os::unix::fs::symlink(&source, &alias).unwrap();
    let mut plan = WatchPlan::new();
    plan.watch_path(&alias, false);
    assert_eq!(tracked(&plan), BTreeSet::from([root, target]));
}

#[test]
fn native_byte_directories_survive_the_wire_without_colliding() {
    use std::ffi::OsStr;
    use std::os::unix::ffi::OsStrExt;
    let scratch = tempfile::tempdir().unwrap();
    let root = canonical(scratch.path());
    let raw = root.join(OsStr::from_bytes(b"raw-\xff"));
    let replacement = root.join("raw-\u{fffd}");
    fs::create_dir(&raw).unwrap();
    fs::create_dir(&replacement).unwrap();
    let mut plan = WatchPlan::new();
    plan.watch_path(&raw, true);
    plan.watch_path(&replacement, true);
    let mut document = Map::new();
    plan.finish(&mut document);
    let decoded: BTreeSet<PathBuf> = document["watchPaths"]
        .as_array()
        .unwrap()
        .iter()
        .map(|value| parse_path(value.as_str().unwrap()).unwrap())
        .collect();
    assert_eq!(decoded, BTreeSet::from([root, raw, replacement]));
}

#[test]
fn the_plan_is_bounded_by_its_path_count() {
    let scratch = tempfile::tempdir().unwrap();
    let root = canonical(scratch.path());
    let mut plan = WatchPlan::new();
    for number in 0..=MAX_WATCH_PATHS {
        let child = root.join(number.to_string());
        fs::create_dir(&child).unwrap();
        plan.watch_path(&child, true);
    }
    assert_eq!(plan.paths().len(), MAX_WATCH_PATHS);
    assert!(plan.truncated());
}

#[test]
fn escaped_watch_paths_fit_the_wire_byte_budget() {
    let scratch = tempfile::tempdir().unwrap();
    let root = canonical(scratch.path());
    let long = "é".repeat(120);
    let mut plan = WatchPlan::new();
    for index in 0..140 {
        let child = root.join(format!("{long}{index}"));
        fs::create_dir(&child).unwrap();
        plan.watch_path(&child, true);
    }
    let mut document = Map::new();
    plan.finish(&mut document);
    assert!(document["watchTruncated"].as_bool().unwrap());
    let encoded = serde_json::to_string(&document["watchPaths"]).unwrap();
    assert!(encoded.len() <= MAX_WATCH_BYTES, "{}", encoded.len());
    for value in document["watchPaths"].as_array().unwrap() {
        assert_eq!(
            path_text(&parse_path(value.as_str().unwrap()).unwrap()),
            value.as_str().unwrap()
        );
    }
}

#[test]
fn lane_rows_split_project_and_user_scopes() {
    let rows = vec![
        json!({"scope": "project"}),
        json!({"scope": "local"}),
        json!({"scope": "user"}),
        json!({"other": 1}),
    ];
    let project = ["project", "local"];
    assert_eq!(SCOPES, ["all", "user", "project"]);
    assert_eq!(lane_rows(rows.clone(), "all", &project, "scope").len(), 4);
    assert_eq!(
        lane_rows(rows.clone(), "project", &project, "scope").len(),
        2
    );
    assert_eq!(lane_rows(rows, "user", &project, "scope").len(), 2);
}

#[test]
fn bounded_emission_bisects_items_to_its_byte_budget() {
    let mut payload = Map::new();
    payload.insert("ok".to_string(), json!(true));
    payload.insert(
        "items".to_string(),
        Value::Array(
            (0..500)
                .map(|index| json!({"id": index, "body": "x".repeat(64)}))
                .collect(),
        ),
    );
    let full = encoded(&payload, MAX_OUTPUT_BYTES);
    let document: Value = serde_json::from_slice(&full[..full.len() - 1]).unwrap();
    assert_eq!(document["count"], 500);
    assert_eq!(document["truncated"], false);
    assert_eq!(full.last(), Some(&b'\n'));

    let trimmed = encoded(&payload, 4096);
    assert!(trimmed.len() <= 4096);
    let document: Value = serde_json::from_slice(&trimmed[..trimmed.len() - 1]).unwrap();
    assert_eq!(document["truncated"], true);
    let kept = document["count"].as_u64().unwrap() as usize;
    assert!(kept > 0 && kept < 500, "{kept}");
    assert_eq!(document["items"].as_array().unwrap().len(), kept);
}

#[test]
fn emission_without_items_falls_back_to_an_empty_listing() {
    let mut payload = Map::new();
    payload.insert("ok".to_string(), json!(true));
    payload.insert("truncated".to_string(), json!(0));
    let data = encoded_items(&payload);
    let document: Value = serde_json::from_slice(&data[..data.len() - 1]).unwrap();
    assert_eq!(document["items"], json!([]));
    assert_eq!(document["count"], 0);
    assert_eq!(document["truncated"], false);
}

#[test]
fn oversized_results_are_replaced_by_a_refusal() {
    let mut payload = Map::new();
    payload.insert("ok".to_string(), json!(true));
    payload.insert(
        "results".to_string(),
        Value::Array(
            (0..40_000)
                .map(|index| json!({"id": index, "note": "n".repeat(32)}))
                .collect(),
        ),
    );
    let data = encoded_results(&payload);
    let document: Value = serde_json::from_slice(&data[..data.len() - 1]).unwrap();
    assert_eq!(document["ok"], false);
    assert_eq!(document["results"], json!([]));
    assert_eq!(document["message"], "apply output exceeded the size bound");
}

#[test]
fn text_cleaning_collapses_whitespace_and_drops_control_bytes() {
    assert_eq!(clean("  a\t \nb\u{7}c  ", 64), "a bc");
    assert_eq!(clean("keep", 2), "ke");
    assert_eq!(clean("\u{c}\u{b}\u{1f}", 64), "");
    assert_eq!(estimated_tokens(""), 0);
    assert_eq!(estimated_tokens("abcd"), 1);
    assert_eq!(estimated_tokens("abcde"), 2);
    assert_eq!(word_count("one two_three 4"), 3);
}

#[test]
fn artifact_metrics_report_counts_only_for_bounded_files() {
    let scratch = tempfile::tempdir().unwrap();
    let path = scratch.path().join("SKILL.md");
    let body = "hello world\n";
    fs::write(&path, body).unwrap();
    let metrics = artifact_metrics(&path, body, body.len() as u64);
    assert_eq!(metrics["bytes"], json!(body.len()));
    assert_eq!(metrics["characters"], json!(body.chars().count()));
    assert_eq!(metrics["words"], json!(2));
    assert_eq!(metrics["tokens"], json!(3));
    assert_eq!(metrics["updated"].as_str().unwrap().len(), 16);

    let large = artifact_metrics(&path, body, 1024 * 1024);
    assert_eq!(large["characters"], Value::Null);
    assert_eq!(large["words"], Value::Null);
    assert_eq!(large["tokens"], Value::Null);

    assert!(artifact_metrics(&scratch.path().join("absent"), "", 0).is_empty());
}
