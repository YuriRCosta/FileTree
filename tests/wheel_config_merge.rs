use fileblade::drop_target::config;
use serde_json::{Value, json};

fn defaults() -> Vec<Value> {
    vec![
        json!({"id":"open","label":"Open","key":"o","placements":[]}),
        json!({"id":"terminal","label":"Terminal","key":"t","icon":"old","icon_source":"file:///old.png","placements":[]}),
        json!({"id":"mux-open","label":"herdr","key":"h","placements":[{"id":"right","label":"Right"},{"id":"down","label":"Down"}]}),
    ]
}

fn merge(doc: Value) -> (Vec<Value>, Vec<String>) {
    config::merge(
        &defaults(),
        &doc,
        &json!({"kind":"desktop"}),
        &json!({"paths":["/tmp/a.txt"],"mimes":{"/tmp/a.txt":"text/plain"}}),
    )
}

#[test]
fn orders_hides_relabels_and_replaces_inherited_icon() {
    let (rows, errors) = merge(
        json!({"version":1,"actions":[{"id":"terminal","label":"Shell","key":"s","icon":"utilities-terminal"},{"id":"open","hidden":true}]}),
    );
    assert!(errors.is_empty(), "{errors:?}");
    assert_eq!(
        rows.iter()
            .map(|r| r["id"].as_str().unwrap())
            .collect::<Vec<_>>(),
        ["terminal", "mux-open"]
    );
    assert_eq!(rows[0]["label"], "Shell");
    assert_eq!(rows[0]["key"], "s");
    assert_eq!(rows[0]["icon_source"], "");
    assert_eq!(rows[0]["icon_override"], true);
}

#[test]
fn custom_action_can_be_ordered_before_builtins_and_has_two_layers() {
    let doc = json!({"version":1,"actions":[{"id":"custom:inspect"}],"customActions":[{"id":"custom:inspect","label":"Inspect","placements":[{"id":"format","label":"Format","placements":[{"id":"numbered","label":"Numbered","command":["cat","-n","{paths}"],"runMode":"terminal"}]}]}]});
    let (rows, errors) = merge(doc);
    assert!(errors.is_empty(), "{errors:?}");
    assert_eq!(rows[0]["id"], "custom:inspect");
    assert_eq!(
        rows[0]["placements"][0]["placements"][0]["command_route"],
        json!(["custom:inspect", "format", "numbered"])
    );
}

#[test]
fn malformed_sibling_is_skipped_and_valid_sibling_survives() {
    let (rows, errors) = merge(
        json!({"version":1,"customActions":[{"id":"custom:bad","label":"Bad","command":"cat {paths}"},{"id":"custom:ok","label":"OK","command":["cat","{paths}"]}]}),
    );
    assert!(rows.iter().any(|r| r["id"] == "custom:ok"));
    assert!(!rows.iter().any(|r| r["id"] == "custom:bad"));
    assert!(errors.iter().any(|e| e.contains("argument array")));
}

#[test]
fn builtin_cannot_be_redefined() {
    let (rows, errors) = merge(
        json!({"version":1,"customActions":[{"id":"open","label":"Replace","command":["false"]}],"actions":[{"id":"terminal","command":["false"]}]}),
    );
    assert_eq!(rows[0]["id"], "open");
    assert!(rows.iter().any(|r| r["id"] == "terminal"));
    assert_eq!(errors.len(), 2);
}

#[test]
fn conditions_require_every_selected_path_and_known_mime() {
    let doc = json!({"version":1,"customActions":[{"id":"custom:text","label":"Text","command":["cat","{paths}"],"targetKinds":["desktop"],"conditions":{"mime":["text/*"],"path":["/tmp/*.txt"]}}]});
    let (rows, errors) = merge(doc.clone());
    assert!(errors.is_empty());
    assert!(rows.iter().any(|r| r["id"] == "custom:text"));
    for target in [json!({"kind":"terminal"}), json!({"kind":"desktop"})] {
        let (rows, errors) = config::merge(
            &defaults(),
            &doc,
            &target,
            &json!({"paths":["/tmp/a.txt","/tmp/b.png"],"mimes":{"/tmp/a.txt":"text/plain"}}),
        );
        assert!(errors.is_empty());
        assert!(!rows.iter().any(|r| r["id"] == "custom:text"));
    }
}

#[test]
fn fourth_layer_is_rejected_with_a_diagnosis() {
    let (rows, errors) = merge(
        json!({"version":1,"customActions":[{"id":"custom:a","label":"A","placements":[{"id":"b","label":"B","placements":[{"id":"c","label":"C","placements":[{"id":"d","label":"D","command":["true"]}]}]}]}]}),
    );
    assert!(!rows.iter().any(|r| r["id"] == "custom:a"));
    assert!(errors.iter().any(|e| e.contains("two placement layers")));
}

#[test]
fn placement_overrides_and_builtin_aliases_keep_dispatch_identity() {
    let (rows, errors) = merge(
        json!({"version":1,"actions":[{"id":"mux-open","placements":[{"id":"down","label":"Below"},{"id":"right","hidden":true},{"id":"custom:shell","label":"Shell","builtin":{"action":"terminal"}}]}]}),
    );
    assert!(errors.is_empty(), "{errors:?}");
    let children = rows[0]["placements"].as_array().unwrap();
    assert_eq!(children.len(), 2);
    assert_eq!(children[0]["builtin_action"], "mux-open");
    assert_eq!(children[0]["builtin_placement"], "down");
    assert_eq!(children[1]["builtin_action"], "terminal");
    assert_eq!(children[1]["icon"], "old");
    assert_eq!(children[1]["icon_source"], "file:///old.png");
    assert_eq!(
        children[1]["command_route"],
        json!(["mux-open", "custom:shell"])
    );
    assert!(children[0].get("command_route").is_none());
}

#[test]
fn application_aliases_keep_separate_configured_identities() {
    let defaults = [
        json!({"id":"open-with","placements":[{"id":"application","desktop_id":"viewer.desktop","label":"Viewer"}]}),
    ];
    let mut document = json!({"version":1,"customActions":[
        {"id":"custom:first","label":"First","builtin":{"action":"open-with","placement":"viewer.desktop"}},
        {"id":"custom:second","label":"Second","builtin":{"action":"open-with","placement":"viewer.desktop"}}
    ]});
    let (rows, errors) = config::merge(&defaults, &document, &json!({}), &json!({}));
    assert!(errors.is_empty(), "{errors:?}");
    for (index, id) in [(1, "custom:first"), (2, "custom:second")] {
        assert_eq!(rows[index]["command_route"], json!([id]));
        assert_eq!(rows[index]["desktop_id"], "viewer.desktop");
        assert_eq!(rows[index]["builtin_action"], "application");
    }
    document["actions"] = json!([{"id":"custom:first","hidden":true}]);
    let (rows, errors) = config::merge(&defaults, &document, &json!({}), &json!({}));
    assert!(errors.is_empty(), "{errors:?}");
    assert!(!rows.iter().any(|row| row["id"] == "custom:first"));
    assert_eq!(rows[1]["command_route"], json!(["custom:second"]));
}

#[test]
fn future_fields_are_never_modified_and_future_version_uses_defaults() {
    let doc =
        json!({"version":1,"future":{"keep":42},"actions":[{"id":"terminal","future":"retain"}]});
    let before = doc.clone();
    config::merge(&defaults(), &doc, &json!({"kind":"desktop"}), &json!({}));
    assert_eq!(doc, before);
    let (rows, errors) = merge(json!({"version":99,"actions":[{"id":"open","hidden":true}]}));
    assert_eq!(rows[0]["id"], "open");
    assert_eq!(errors.len(), 1);
}

#[test]
fn substitutions_preserve_literal_arguments_and_refuse_ambiguous_path() {
    let facts =
        json!({"paths":["/tmp/a '$() #%.txt","/tmp/b.txt"],"folder":"/tmp","git_root":"/tmp/repo"});
    let args = config::expand(
        &json!([
            "tool",
            "{paths}",
            "{cwd}",
            "{git_root}",
            "$(touch /tmp/never)"
        ]),
        &facts,
    )
    .unwrap();
    assert_eq!(
        args,
        [
            "tool",
            "/tmp/a '$() #%.txt",
            "/tmp/b.txt",
            "/tmp",
            "/tmp/repo",
            "$(touch /tmp/never)"
        ]
        .map(std::ffi::OsString::from)
    );
    assert!(config::expand(&json!(["tool", "{path}"]), &facts).is_err());
    assert!(config::expand(&json!(["tool", "prefix={path}"]), &facts).is_err());
    assert!(config::expand(&json!(["tool", "{git_root}"]), &json!({})).is_err());
    assert!(config::expand(&json!(["{path}"]), &facts).is_err());
}

#[test]
fn hiding_every_builtin_placement_removes_the_parent_without_a_fallback_launch() {
    let (rows, errors) = merge(
        json!({"version":1,"actions":[{"id":"mux-open","placements":[{"id":"right","hidden":true},{"id":"down","hidden":true}]}]}),
    );
    assert!(errors.is_empty(), "{errors:?}");
    assert!(!rows.iter().any(|row| row["id"] == "mux-open"));
}

#[test]
fn a_glyph_override_replaces_an_inherited_application_icon() {
    let (rows, errors) = merge(json!({"version":1,"actions":[{"id":"terminal","glyph":"S"}]}));
    assert!(errors.is_empty());
    assert_eq!(rows[0]["glyph"], "S");
    assert_eq!(rows[0]["icon"], "");
    assert_eq!(rows[0]["icon_source"], "");
}

#[test]
fn path_substitution_preserves_non_utf8_bytes() {
    use std::os::unix::ffi::OsStrExt;
    let args = config::expand(
        &json!(["tool", "{path}"]),
        &json!({"paths":["file:///tmp/odd-%FF.txt"]}),
    )
    .unwrap();
    assert_eq!(args[1].as_os_str().as_bytes(), b"/tmp/odd-\xff.txt");
}

#[test]
fn visible_rings_are_bounded_even_with_too_many_custom_entries() {
    let custom = (0..20)
        .map(|index| json!({"id":format!("custom:item{index}"),"label":"Item","command":["true"]}))
        .collect::<Vec<_>>();
    let (rows, errors) = merge(json!({"version":1,"customActions":custom}));
    assert_eq!(rows.len(), 12);
    assert!(!errors.is_empty());
    let keys = rows
        .iter()
        .map(|row| row["key"].as_str().unwrap())
        .collect::<std::collections::HashSet<_>>();
    assert_eq!(keys.len(), rows.len());
}
